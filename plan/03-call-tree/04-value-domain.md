# 03.04 — The value domain

What flows through the interpreter. Get this wrong and everything downstream is
awkward; get it right and the rest writes itself.

---

## Why not just use `ty::Type<'db>`

Tempting — it's already there, it's `Copy`, it has all the class machinery. But:

1. **`Type` is a set, not a value.** `Type::NominalInstance(JsonWriter)` means
   "some JsonWriter". You need "*this* JsonWriter, constructed at *that* call
   site" to keep two paths distinct.
2. **`Type`'s operations are private.** The fork reaches them through
   `types/vnoc.rs` wrappers (`01-crates/04`), which is what makes `member` and
   the MRO usable — but reaching an operation is not the same as having a value
   model. Points 1, 3 and 4 are why you still need your own.
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
/// Identifies a construction: which `Foo()` expression, building which class,
/// from which argument values. See `OriginKey` below for the exact key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct OriginId(u32);   // index into an arena of OriginKey
```

Without `origin`, two `JsonWriter()` instances are identical values and your
attribute store merges their state. With it, they stay distinct.

**[check]** — Jedi's `TreeInstance` has exactly this property (it holds the
constructing `arguments`), which is why your current tool works. Verify with a
fixture: two instances of the same class configured differently in `__init__`,
each calling a different handler.

### ★ `origin_attrs` — where a constructor's writes actually live

The origin is content-addressed, on purpose: the SAME `Foo()` construction
must always mint the SAME `OriginId` — otherwise
[`03`](03-the-abstract-interpreter.md)'s tree-walk branch (which
constructs `Pipeline(...)` to walk `__init__` for the frame tree) and
[`10`](10-return-values-and-state.md)'s value-query branch (which constructs
`Pipeline(...)` because something needs its *value*, e.g. as the receiver of
`.run()`) mint two *different* origins for what is syntactically one
expression, and nothing either of them records about `self.handler` is ever
visible to the other. That was an actual bug in an earlier draft of this
plan, caught by tracing a two-line fixture by hand — see
[`06`](06-attributes-and-self.md)'s fixture 1.

The fix is to make "attrs for this origin" a cache keyed by the origin
itself, computed once, the first time anything asks — the same shape as
`eval_returns`'s `return_memo` in [`10`](10-return-values-and-state.md#2-return-value-flow),
and for the same reason: a pure function of `(class, args)`, safe to memoise,
with exactly one writer.

```rust
pub struct Interp<'db> {
    // ...
    origin_arena: OriginArena<'db>,   // interns OriginKey → OriginId
    origin_attrs: rpds::HashTrieMap<(OriginId, Name), Values<'db>>,   // same shape as Env.attrs
}
```

#### ★ `OriginKey` — exactly what goes in, and nothing else

The memo is only correct if the key holds **every input `__init__`'s result
depends on**. Miss one and two different objects share a cache entry; add
one the result doesn't depend on and nothing breaks, but you get two code
paths that must compute it identically. So the key is the inputs, stored as
values, not hashed:

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct OriginKey<'db> {
    file:  File,
    range: TextRange,           // the `Foo(...)` call expression
    class: ClassLiteral<'db>,   // the class being built at that site
    args:  EvaluatedArgs<'db>,  // eval_arguments' output, unchanged
}

impl<'db> OriginArena<'db> {
    /// `true` only the first time this exact key is seen — the caller's
    /// signal to actually run `__init__`, not just fetch a cached id.
    fn intern(&mut self, key: OriginKey<'db>) -> (OriginId, bool);
}
```

| field | why it must be there | what breaks without it |
|---|---|---|
| `file`, `range` | two different `Foo()` expressions are two objects | fixture 2 in [`06`](06-attributes-and-self.md#fixtures): `a` and `b` share state |
| `class` | `(A if c else B)(h)` is one expression building two classes | the `B` instance gets `A`'s `__init__` attrs |
| `args` | `__init__`'s writes are a function of what was passed | `make(JsonHandler())` then `make(XmlHandler())` — see below |

`args` is the `EvaluatedArgs` value [`05`](05-binding-arguments.md)'s
`eval_arguments` returns: positional `Values` in order, keyword
`(name, Values)` pairs in source order, and the `has_splat` flag. Use the
values themselves, not a hash of them — a 64-bit collision here is a
silently wrong tree. `Values` is already `Hash + Eq`, so `EvaluatedArgs`
only needs `#[derive(Clone, PartialEq, Eq, Hash)]` added.

**The call path is deliberately not in the key.** An earlier draft keyed on
a "path hash" it never defined, which is how the `make()` bug below got in.
The path only changes `__init__`'s result *through* the arguments, and the
arguments are already in the key. Leaving it out costs nothing: two paths
that reach `Foo(h)` with equal `h` get the same baseline attrs, which is
right because `__init__` would compute the same thing. Later writes
(`p.handler = X()`) live in each path's own `Env.attrs`, so they still
don't leak between paths.

**[check]** — a class defined *inside* a function can read the enclosing
function's locals in its `__init__`. If [`07`](07-callbacks-and-higher-order.md)'s
closure handling gives such a class a captured env, that env is an input
too and goes in the key. Module-level classes (nearly all of them) have
nothing captured.

**Why the two code paths can't disagree.** Only `construct_instance()` builds
an `OriginKey`, and both the tree walk and the value query call it with
`eval_arguments(&call.arguments, env)` for the same `env` (the env *before*
the statement — [`10` §1](10-return-values-and-state.md#1-local-assignment-tracking-cheapest-do-first)).
Same expression, same env → same `EvaluatedArgs` → same key. Nothing else in the
interpreter is allowed to mint an `OriginId` except `OriginId::OPAQUE`.

Test both directions:

```python
# A. same object, two code paths — must SHARE an origin
class Pipeline:
    def __init__(self, h): self.h = h
    def run(self): self.h.process()

def main():
    p = Pipeline(JsonHandler())   # tree walk AND value query both build this
    p.run()                       # → JsonHandler.process (only if they agree)

# B. same expression, different args — must NOT share an origin
def make(h):
    return Pipeline(h)            # one expression, reached twice

def main2():
    make(JsonHandler()).run()     # → JsonHandler.process
    make(XmlHandler()).run()      # → XmlHandler.process, NOT JsonHandler
```

B is the case a path-only key gets wrong: both calls to `make` come from
`main2`, so the path is the same, the first call fills the cache with
`JsonHandler`, and the second reads it back. With `args` in the key, the
two `Pipeline(h)` constructions have different `h` values and get
different origins.

`construct_instance()` in [`10`](10-return-values-and-state.md#2-return-value-flow) is
the one function that interns an origin AND, on first touch, runs
`record_self_assignments` ([`06`](06-attributes-and-self.md#writing-scan-__init__-for-self-x--)
— unchanged) to fill this table. Everything downstream — `resolve_call`'s
tree-walk branch included — calls `construct_instance()` rather than minting its own
instance, so there is exactly one writer and every reader agrees.

This is deliberately **not** the same storage as `Env.attrs`
([`06`](06-attributes-and-self.md#the-design-an-attribute-store-keyed-on-origin)).
`origin_attrs` holds the constructor's baseline, computed once and shared
by every path that reaches this origin. `Env.attrs` holds *later*, sequential
writes — `p.handler = X()` after construction — which really are
path-and-order-dependent and have to be threaded through `Env` the way
[`10`](10-return-values-and-state.md#1-local-assignment-tracking-cheapest-do-first)
already does for local bindings. `member()`'s lookup checks both, in that
order — see [`06`](06-attributes-and-self.md#reading-member-on-a-value).

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
            // we did see. ⚠ Every opaque instance shares this ONE sentinel,
            // so it must never key attribute state — see below.
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
`ty_ide::outgoing_calls`. **[check]** exact accessor names (`elements`,
`class_literal`) at your revision.

`lift` itself does **not** cap. Producing 40 `AbstractValue`s is cheap; what
costs is the 40 subtrees below them. The cap is applied once, by the caller,
in `cap_fanout` — see [§ The union cap](#-the-union-cap-cap_fanout).

### ★ Opaque instances have no attribute slots

`OriginId::OPAQUE` is one value shared by *every* instance ty handed you.
Attribute state is keyed `(OriginId, Name)`, so if opaque instances could
hold state, every opaque instance would share it:

```python
a = get_logger()     # opaque Logger
b = get_db()         # opaque Database
a.handler = FileHandler()
b.handler.write()    # would read (OPAQUE, "handler") → FileHandler.write. WRONG.
```

`a` and `b` are different objects of different classes. Adding the class to
the key doesn't fix it: `get_logger()` called twice gives two `Logger`s that
would still share.

**Rule: writes to an opaque receiver are dropped, and reads skip the
attribute store for it.** `bind_attribute`
([`06`](06-attributes-and-self.md#write-bind_attribute-one-function-for-every-depth))
skips `OPAQUE` receivers the same way it skips `Unknown` ones, and `member()`
on an opaque instance goes straight to the class body and ty. This is the
same reasoning as [`10` §3b](10-return-values-and-state.md#b-any-receiver-not-just-self):
no write to an object of unknown identity.

What it costs: `a = get_logger(); a.handler = FileHandler(); a.handler.write()`
— `a.handler` falls through to ty's declared type for the attribute instead of
`FileHandler`. If a fixture shows today's Jedi driver resolves that case,
upgrade by minting a real origin for each place an opaque value enters the
env (key: the expression's file and range), never by sharing one.

### ★ The union cap: `cap_fanout`

One function, applied wherever `Values` are produced — after `lift`, in
`eval_call_result`, in `eval_returns`
([`10`](10-return-values-and-state.md#2-return-value-flow)). No other code
checks the union size.

```rust
fn cap_fanout(&mut self, mut vals: Values<'db>) -> Values<'db> {
    dedup(&mut vals);
    // `None` turns the cap off entirely — see 08's `Limits`.
    let Some(max) = self.limits.max_union_fanout else { return vals };
    if vals.len() <= max { return vals }

    // Over the cap: first drop what can never produce a tree node, then re-check.
    vals.retain(|v| self.is_project_value(*v));
    if !vals.is_empty() && vals.len() <= max {
        self.stats.union_pruned += 1;
        return vals;
    }
    self.stats.union_fanout_hits += 1;
    smallvec![AbstractValue::Unknown]
}
```

Why prune before giving up:

```python
handler = pick()    # ty: A | B | C | D | E | F — six arms, cap is 4
handler.run()
```

If only `A` and `B` are project classes and the rest are stdlib, a plain
length check returns `Unknown` and the tree shows nothing. But `C`..`F` could
never have produced a node: step (a) of `resolve_call`
([`03`](03-the-abstract-interpreter.md)) drops non-project targets anyway. So
dropping them first loses nothing and keeps `A.run` and `B.run`.

`is_project_value(v)` is `is_project_code` on `v.definition()`: true for a
`Function`, `BoundMethod`, `Class`, `Instance` or `Module` defined in a
project file; false for `Ty(..)`, `Unknown`, and anything from stdlib or
site-packages. Pruning only happens when the cap is exceeded, so under the
cap nothing changes.

What pruning can lose: a non-project value whose attribute leads back into
project code (a stdlib object holding a project callback). ty's types rarely
express that anyway, and the counter records each time pruning ran.

The default is `Some(4)`. Configure it in
[`08`'s `Limits`](08-termination-and-cycles.md#recommended-defaults) — a
number to change it, `None` to turn it off. Watch `union_fanout_hits` and
`union_pruned` on real projects before tuning.

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

Target: `AbstractValue` ≤ 16 bytes, `Copy`. It will be passed by value
millions of times, and the moment it grows a heap allocation (`String`,
`Vec`, `Box`), env clones become the bottleneck.

**16 bytes is not guaranteed.** What the variants hold at `ac201b8`:

| variant | payload | size |
|---|---|---|
| `Ty(Type)` | a whole ty `Type` | **16** — ty asserts this (`types.rs:11192`, release + 64-bit only) |
| `Instance` | `ClassLiteral` + `OriginId` | ~12 — `ClassLiteral` is a 4-variant enum (`types/class.rs:574`), so ~8, not 4 |
| `BoundMethod` | `FunctionType` + `ValueId` | 8 |
| `Module` | `Module` (2-variant enum) | ~8 |
| others | one salsa id, or nothing | ≤ 8 |

`Ty(Type)` alone fills 16 bytes, so `AbstractValue` stays at 16 only if rustc
stores the outer tag in `Type`'s unused tag values and fits the other
variants around it (niche filling). That is likely here — `Type`'s tag byte
has spare values and the other payloads fit in the remaining 12 bytes — but
rustc doesn't promise it, and the answer could change with a compiler
upgrade. Otherwise it lands at 20–24.

Test it under the **same conditions ty does**. ty only asserts 16 bytes in
release 64-bit builds; in a debug build `Type` may be bigger, and the test
would fail for a reason that has nothing to do with you:

```rust
#[cfg(all(not(debug_assertions), target_pointer_width = "64"))]
#[test]
fn value_is_small() {
    assert!(std::mem::size_of::<AbstractValue>() <= 16);
}
```

Run it with `cargo test --release value_is_small`.

**If it fails:** move `Type` out of the enum. `Ty(TyId)`, where `TyId` is a
`u32` index into a per-request `Vec<Type<'db>>` with a dedup map — the same
pattern as `ValueArena` below. Then the largest payload is `Instance`'s ~12
bytes and the enum fits in 16 without help from the compiler. The cost is
one array lookup when reading a `Ty` value, and `Ty` is the "we don't model
this" case, not the hot path.

---

→ Next: [`05-binding-arguments.md`](05-binding-arguments.md)
