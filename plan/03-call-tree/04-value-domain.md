# 03.04 — The value domain

What flows through the interpreter. Get this wrong and everything downstream is
awkward; get it right and the rest writes itself.

---

## Why not just use `ty::Type<'db>`

Tempting — it's already there, it's `Copy`, it has all the class machinery. But:

1. **`Type` is a set, not a value.** `Type::NominalInstance(JsonWriter)` means
   "some JsonWriter". You need "*this* JsonWriter, constructed at *that* call
   site" to keep two paths distinct.
2. **`Type`'s operations are `pub(crate)`.** Even under Option A you'd be
   fighting for access to `bindings`, `static_member`, `member_lookup_with_policy`.
3. **Unions lose path information.** `Type::Union([A, B])` is exactly the
   flattening you're trying to avoid. Your domain must keep alternatives
   *enumerable* so you can produce distinct subtrees.
4. **You need to represent things ty can't**: "the value of parameter `writer`
   on this path", "a bound method whose receiver is this specific instance".

So: **your own enum, with ty types embedded as an escape hatch.**

---

## The type

```rust
/// A value as seen by the interpreter on one specific path.
/// Small and `Copy` — a few words. Never owns strings or Vecs directly.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AbstractValue<'db> {
    /// A specific function object (possibly a nested def or a lambda).
    Function(FunctionType<'db>),

    /// A class object, i.e. the thing you call to construct.
    Class(ClassLiteral<'db>),

    /// An instance of a class. `origin` distinguishes two instances of the same
    /// class constructed at different sites — this is what keeps paths apart.
    Instance { class: ClassLiteral<'db>, origin: OriginId },

    /// `receiver.method` — a method with its receiver already bound.
    /// The receiver is interned so this stays Copy.
    BoundMethod { func: FunctionType<'db>, receiver: ValueId },

    /// A module object, for `mod.func()` call chains.
    Module(Module<'db>),

    /// Fall back to ty's answer. Everything the interpreter doesn't model:
    /// literals, stdlib returns, comprehensions, arithmetic.
    Ty(Type<'db>),

    /// Genuinely no information. Distinct from `Ty(Unknown)` for debugging:
    /// this means "we didn't ask", that means "ty said it doesn't know".
    Unknown,
}

/// Several possibilities, kept ENUMERABLE (never collapsed into a ty Union).
/// SmallVec because 1 is the overwhelmingly common case.
pub type Values<'db> = SmallVec<[AbstractValue<'db>; 2]>;
```

### `OriginId` — the thing that makes it a tree

```rust
/// Identifies a construction site: which `Foo()` expression, on which path.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct OriginId(u32);   // index into an arena of (File, TextRange, PathHash)
```

Without `origin`, two `JsonWriter()` instances are identical values and your
attribute store (next chapter) merges their state. With it, they stay distinct.

**[check]** — Jedi's `TreeInstance` has exactly this property (it holds the
constructing `arguments`), which is why your current tool works. Verify with a
fixture: two instances of the same class configured differently in `__init__`,
each calling a different handler.

### `ValueId` — interning, to stay `Copy`

`BoundMethod` needs a receiver, which is itself an `AbstractValue`. Boxing makes
the enum non-`Copy` and adds allocation on a hot path. Intern instead:

```rust
pub struct ValueArena<'db> {
    values: Vec<AbstractValue<'db>>,
    dedup: FxHashMap<AbstractValue<'db>, ValueId>,
}
```

One arena per request. `ValueId` is a `u32`. Deduping is worth it — receivers
repeat constantly.

---

## The operations

```rust
impl<'db> AbstractValue<'db> {
    /// Is this something you can call? → drives resolve_call.
    fn as_callable(self, db: &'db dyn Db) -> Option<Callable<'db>>;

    /// The Definition this value points at — for qname, docstring ID, project check.
    fn definition(self, db: &'db dyn Db) -> Option<Definition<'db>>;

    /// Attribute access.  → 06-attributes-and-self
    fn member(self, cx: &Cx<'db>, name: &str) -> Values<'db>;

    /// Constructing: Class → Instance, everything else → Unknown.
    fn construct(self, cx: &Cx<'db>, origin: OriginId) -> Values<'db>;

    /// Escape hatch into ty, for when you must hand off.
    fn to_ty(self, db: &'db dyn Db) -> Type<'db>;
}

pub enum Callable<'db> {
    Function(FunctionType<'db>),
    Class(ClassLiteral<'db>),
    Bound { func: FunctionType<'db>, receiver: ValueId },
}
```

`definition()` is the join point with everything in
[`01-what-jedi-actually-does.md`](01-what-jedi-actually-does.md): from a
`Definition` you get the file (→ project check), the name and scope chain
(→ qname), and the body (→ docstring ID, and the AST to walk).

---

## Converting from ty — the boundary

When the environment misses and you ask ty, you get a `Type<'db>` and must lift
it back into your domain:

```rust
fn lift(db: &'db dyn Db, env: &ProgramEnvironment<'db>, ty: Type<'db>) -> Values<'db> {
    match ty {
        Type::FunctionLiteral(f)   => smallvec![AbstractValue::Function(f)],
        Type::ClassLiteral(c)      => smallvec![AbstractValue::Class(c)],
        Type::BoundMethod(m)       => { /* Function + interned receiver */ }
        Type::ModuleLiteral(m)     => smallvec![AbstractValue::Module(m.module(db))],

        // ★ the important case: keep alternatives SEPARATE
        Type::Union(u)             => u.elements(db).iter()
                                        .flat_map(|t| lift(db, env, *t))
                                        .collect(),

        Type::NominalInstance(i)   => {
            // No origin — this instance didn't come from a construction site
            // we saw. Use a sentinel origin so it can't be confused with one
            // we did see.
            smallvec![AbstractValue::Instance {
                class: i.class(db, env).class_literal(db),
                origin: OriginId::OPAQUE,
            }]
        }

        Type::GenericAlias(_) | Type::SubclassOf(_) | Type::TypeAlias(_)
                                   => { /* unwrap to the underlying class */ }

        Type::Dynamic(_) | Type::Never | Type::Divergent(_)
                                   => smallvec![AbstractValue::Unknown],

        other                      => smallvec![AbstractValue::Ty(other)],
    }
}
```

**The `Union` arm is the one that matters.** ty hands you a set; you must
explode it into separate values so each produces its own subtree. Flatten it
into `Type::Union` again anywhere and you've silently rebuilt
`ty_ide::outgoing_calls`.

Cap the explosion: `if u.elements(db).len() > MAX_UNION_FANOUT { return Unknown }`.
A union of 40 members would multiply your path count by 40 at one node.
`MAX_UNION_FANOUT = 4` is a reasonable start — tune against real projects and
count how often you hit it. **[check]** exact accessor names (`elements`,
`class_literal`) at your revision.

---

## What you deliberately do not model

Keeping the domain small is a feature. From
[`01`](01-what-jedi-actually-does.md#what-it-does-not-do-scope-limits--respect-them),
Jedi's resolver doesn't do these either:

| Not modelled | What you do instead |
|---|---|
| container contents (`list[Handler]`) | `Ty(...)`, let ty answer element types if asked |
| dict/tuple destructuring | `Unknown` for the bindings |
| `global` / `nonlocal` | `Unknown` |
| generators / async values | `Unknown`; the *body* is still walked |
| **branch conditions** | both arms unioned, never evaluated — ★ the line that keeps this finite |
| decorated function identity | resolve to the **undecorated** function; note as parity risk |

And two that **are** modelled, contrary to an earlier draft of this plan —
both are present in today's behaviour via Jedi, so omitting them regresses:

| Modelled | Where |
|---|---|
| return values from project functions | [`10`](10-return-values-and-state.md#2-return-value-flow) |
| local assignment + object state after construction | [`10`](10-return-values-and-state.md#1-local-assignment-tracking-cheapest-do-first), [`06`](06-attributes-and-self.md) |

That last row deserves a fixture. `@lru_cache def f()` — Jedi may resolve `f` to
the decorator's return value, ty resolves to a wrapper type. Both may miss the
underlying function. Test what your current driver does and match it.

---

## Sizing

Target: `AbstractValue` ≤ 16 bytes, `Copy`. Add a test:

```rust
#[test]
fn value_is_small() {
    assert!(std::mem::size_of::<AbstractValue>() <= 16);
}
```

It will be passed by value millions of times. Salsa handles are `u32`-ish
(`salsa::Id`), so this is achievable — but only if you resist putting a `String`
or `Vec` in there. The moment `AbstractValue` grows a heap allocation, your
env clones become the bottleneck.

---

→ Next: [`05-binding-arguments.md`](05-binding-arguments.md)
