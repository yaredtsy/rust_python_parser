# 03.04 — Values

What flows through one walk. Everything in this chapter is walk-local: it
lives while one body is walked, and summaries never contain it
([`09`](09-summaries.md)).

---

## Why not just use `ty::Type<'db>`

1. **`Type` is a set, not a value.** `Type::NominalInstance(JsonWriter)` means
   "some JsonWriter". The walk needs "this JsonWriter, built at that site", to
   keep two objects apart.
2. **Unions lose alternatives.** `Type::Union([A, B])` is exactly the
   flattening the call tree must avoid. The walk keeps alternatives
   enumerable, so each one produces its own edges.
3. **ty can't express path facts**: "the object at parameter 0's `.app`", "a
   bound method whose receiver is this instance", "the value of a recursive
   call that hasn't resolved yet".

So: **an own enum, with ty types embedded as an escape hatch.**

---

## The type

Functions and classes are identified by `Definition<'db>`, not by ty's
`FunctionType` / `ClassLiteral`. Those can't be named outside
`ty_python_semantic` (`ClassLiteral` is `pub(crate)`, `FunctionType` a private
`use`). `Definition<'db>` is public, a 4-byte salsa id, and is what the qname,
docstring ID and project check start from anyway.

```rust
/// A value inside one walk. Small and `Copy`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(in crate::interp) enum AbstractValue<'db> {
    /// A function with nothing captured: a module-level def, or one ty handed
    /// us. Its free names are module globals, so ty answers them. For a
    /// decorated function, still the `def`'s own Definition.
    Function(Definition<'db>),

    /// A nested def, bound by its `def` statement running inside a walk.
    /// `activation` is that run's activation: where the def ran, not where it
    /// is called. → 11
    Closure { func: Definition<'db>, activation: ActivationId },

    /// A class object: the thing you call to construct.
    Class(Definition<'db>),

    /// One object. `origin` says which one (below).
    Instance { class: Definition<'db>, origin: OriginId },

    /// `receiver.method`, receiver already bound. Interned to stay `Copy`.
    BoundMethod { func: Definition<'db>, receiver: ValueId },

    /// A module object, for `mod.func()`. Keeps ty's module literal type, not
    /// just the module: for a package, the type records the importing file,
    /// which decides whether `pkg.sub` is an attribute. `vnoc::member` on it
    /// answers that the way Python would.
    Module(Type<'db>),

    /// ty's answer, for everything the walk doesn't model: literals, stdlib
    /// returns, comprehensions, arithmetic.
    Ty(Type<'db>),

    /// The value of an operation waiting on a recursive call. → 12
    Pending(DeferId),

    /// No information. Distinct from `Ty(Unknown)`: this means "nobody asked",
    /// that means "ty said it doesn't know".
    Unknown,
}

/// Several possibilities, kept enumerable. Never collapsed into a ty Union.
/// SmallVec because one value is by far the common case.
pub type Values<'db> = SmallVec<[AbstractValue<'db>; 2]>;
```

An empty `Values` means "no value reaches here": every path to it raised, or it
waits on a cycle that hasn't produced anything yet. It is not `Unknown`.
[`10`](10-one-call.md#a-call-with-no-value) says what a call does with one.

### `OriginId`: which object

```rust
pub struct OriginId(u32);   // index into the walk's origin interner

enum OriginKey {
    /// An object the caller passed in, reached by this path from the inputs.
    /// From `materialize` (08), or read on demand past the key (07).
    Entry(Path),
    /// An input object deeper than `max_path`. Holds no state. A write to it
    /// becomes the fallback effect (07).
    Beyond { at: Path, attr: Name },
    /// An object built in this walk: by this site's constructor, or one of a
    /// callee's `objects[j]`.
    New { site: SiteId, t: u8, combo: u8, made: Made },
}
enum Made { Instance, Obj(u16) }
```

`Path` is defined in [`09`](09-summaries.md#values-relative-to-the-inputs). `t`
is the callee's index among the site's callees, and `combo` the CPA combo index
([`10`](10-one-call.md)). Both are needed: without `combo`, two contexts split
from one site would merge their returned objects into one origin, and later
calls would see attribute pairs that never existed together.

Interned per walk, so the same key always gives the same `OriginId`. A site is
evaluated once per walk, so one `New` key never stands for two objects.

Two `Pipeline(...)` expressions are two origins, so their attributes never
collide. That is the property the two-instance fixture checks
([`07`](07-attributes-and-self.md#fixtures)).

### `OriginId::OPAQUE`

One reserved origin for instances ty handed the walk: a module global, a stdlib
return, an annotation. Every such instance shares it, so it can never hold
state:

```python
a = get_logger()     # Instance{Logger,   OPAQUE}
b = get_db()         # Instance{Database, OPAQUE}
a.handler = FileHandler()
b.handler.write()    # if (OPAQUE, "handler") were stored, b would see a's write
```

**Rule: writes to an `OPAQUE` receiver are dropped, and reads skip the attribute
store for it.** `member` goes straight to the class body and ty.

Objects the caller passed in are not `OPAQUE`. They have `Entry` origins, named
by their path, so writes to them are tracked and reported
([`07`](07-attributes-and-self.md#objects-the-key-does-not-reach)).

### `ValueId`: interning, to stay `Copy`

`BoundMethod` needs its receiver, which is itself an `AbstractValue`. A `Box`
would make the enum non-`Copy`. Intern instead:

```rust
pub struct ValueArena<'db> {
    values: Vec<AbstractValue<'db>>,
    dedup:  FxHashMap<AbstractValue<'db>, ValueId>,
}
```

One arena per walk. Receivers repeat constantly, so the dedup pays.

### `ActivationId` and `DeferId`

`ActivationId` indexes the walk's activations: one per run of a body inside
this walk (the walk's own body, and each closure body materialized for it).
[`05`](05-the-walk.md#activations). `DeferId` indexes the walk's deferred
operations ([`12`](12-recursion.md)). Both are `u32`.

---

## The operations

```rust
impl<'db> AbstractValue<'db> {
    /// Can this be called? Drives eval_call.
    fn as_callable(self, db: &'db dyn Db) -> Option<Callable<'db>>;

    /// The Definition this value points at: qname, docstring ID, project check.
    fn definition(self, db: &'db dyn Db) -> Option<Definition<'db>>;

    /// Escape hatch into ty.
    fn to_ty(self, db: &'db dyn Db) -> Type<'db>;
}

pub enum Callable<'db> {
    Function(Definition<'db>),
    Closure { func: Definition<'db>, activation: ActivationId },
    Class(Definition<'db>),
    Bound { func: Definition<'db>, receiver: ValueId },
}
```

`member` is [`07`](07-attributes-and-self.md). Calling is
[`10`](10-one-call.md).

**Going back to ty.** `vnoc::member` takes a `Type`, so a `Class(def)` or
`Instance { class: def, .. }` needs the class's `Type` again:
`vnoc::type_of_definition(db, def)`, a wrapper over `binding_type`
(`pub(crate)`, `types.rs:235`). **[check]**

Two edge-case rules:

- **A decorated function points at its `def`.** ty types a decorated name as
  whatever the decorator returns (`@lru_cache` gives a wrapper object). When
  the name's binding is a `def` statement, use that statement's `Definition`.
- **A synthesised `__init__` has no body.** A dataclass, `NamedTuple` or
  attrs class still gets its class edge. Nothing is walked, the instance has
  no attributes, and `member` falls through to ty's field types.

---

## `lift`: from ty back into values

When the env has nothing and ty answers, its `Type<'db>` is lifted:

```rust
const MAX_LIFT_DEPTH: u8 = 8;   // only recursive type aliases get near this

fn lift(&mut self, ty: Type<'db>) -> Values<'db> { self.lift_at(ty, 0) }

/// `Type::definition` is public (`types.rs:9216`). It maps a function or
/// bound method to the function's def; a class, `Box[int]` or `type[X]` to
/// the class's def; an instance to its class's def; a module to the module.
fn def_of(&self, ty: Type<'db>) -> Option<TypeDefinition<'db>> { ty.definition(self.db, self.env()) }
fn class_def(&self, ty: Type<'db>) -> Option<Definition<'db>> {
    match self.def_of(ty)? { TypeDefinition::StaticClass(d) | TypeDefinition::DynamicClass(d) => Some(d), _ => None }
}
fn function_def(&self, ty: Type<'db>) -> Option<Definition<'db>> {
    match self.def_of(ty)? { TypeDefinition::Function(d) => Some(d), _ => None }
}

fn lift_at(&mut self, ty: Type<'db>, depth: u8) -> Values<'db> {
    use AbstractValue::*;
    if depth > MAX_LIFT_DEPTH { return smallvec![Unknown] }
    let or_ty = |v: Option<AbstractValue<'db>>| smallvec![v.unwrap_or(Ty(ty))];
    match ty {
        Type::FunctionLiteral(_)   => or_ty(self.function_def(ty).map(Function)),

        // `Box[int]()` → Box's def → an edge to Box.__init__.
        Type::ClassLiteral(_) | Type::GenericAlias(_)
                                   => or_ty(self.class_def(ty).map(Class)),

        // `cls: type[Base]` → Class(Base), so `cls()` gets a Base.__init__ edge.
        // `type[Any]`, `type[T]` → Unknown.
        Type::SubclassOf(_) => match self.class_def(ty) {
            Some(d) => smallvec![Class(d)],
            None    => smallvec![Unknown],
        },

        Type::ModuleLiteral(_) => smallvec![Module(ty)],

        // A bound method ty already bound (`f = get_obj().method`). Without
        // this arm it is Ty(BoundMethod), not callable, and the call has no
        // edge. Its receiver is ty's, so it lifts to an OPAQUE instance.
        // `member` on the walk's own instances does not come through here:
        // it binds its own receiver (07).
        Type::BoundMethod(_) => {
            let (Some(func), Some(recv)) = (self.function_def(ty), vnoc::bound_method_self(self.db, ty))
                else { return smallvec![Ty(ty)] };
            self.lift_at(recv, depth + 1).into_iter()
                .map(|r| BoundMethod { func, receiver: self.intern(r) })
                .collect()
        }

        // Keep alternatives separate. This is the arm that matters.
        Type::Union(u)             => u.elements(self.db).iter()
                                        .flat_map(|t| self.lift_at(*t, depth + 1))
                                        .collect(),

        // Known gap: a specialised instance (`b: Box[Foo]`) becomes plain
        // Instance{Box}, so `b.get().handle()` has no edge. Later fix, no
        // enum change: keep it as Ty(ty), where ty keeps the specialisation.
        Type::NominalInstance(_)   => or_ty(self.class_def(ty)
                                        .map(|class| Instance { class, origin: OriginId::OPAQUE })),

        // `type Handlers = A | B`: lift what it stands for.
        Type::TypeAlias(a)         => self.lift_at(a.value_type(self.db), depth + 1),

        Type::Dynamic(_) | Type::Never | Type::Divergent(_)
                                   => smallvec![Unknown],

        // Deliberately Ty: KnownBoundMethod, Callable, TypeVar, Intersection,
        // ProtocolInstance, PropertyInstance (07 handles properties), literals.
        // Calling a Ty(Callable) uses ty's return type (10). Add an arm only
        // with a fixture that shows a lost project edge.
        other                      => smallvec![Ty(other)],
    }
}
```

Two `vnoc` wrappers this needs, one internal call each, public types only:

| wrapper | wraps | visibility today |
|---|---|---|
| `bound_method_self(db, ty) -> Option<Type>` | `BoundMethodType::self_instance` | `pub(super)` (`types/method.rs:29`) |
| `type_of_definition(db, def) -> Type` | `binding_type(db, def)` | `pub(crate)` (`types.rs:235`) |

Fixtures, one per arm. Each is an edge that silently disappears if the arm is
missing:

```python
# 1. BoundMethod from ty: edge to Handler.handle
handler = Handler()
dispatch = handler.handle      # module global: the env never sees it
def use():
    dispatch(1)

# 2. GenericAlias: edge to Box.__init__
def make(): return Box[int](1)

# 3. SubclassOf, no known caller: edge to Base.__init__
def build(cls: type[Base]):
    return cls()

# 4. TypeAlias to a union: edges to A.run and B.run
type H = A | B
def go(h: H):
    h.run()

# 5. KNOWN GAP: specialisation lost. Today: no Foo.handle edge.
def run(b: Box[Foo]):
    b.get().handle()

# 6. decorated function: edge to the def, not the decorator's wrapper
@lru_cache
def load(): ...
def use(): load()

# 7. dataclass: edge to the class, no __init__ body to walk
@dataclass
class Job: handler: Handler
def make(): Job(Handler())
```

`lift` itself does not cap. Its caller passes the result through `cap_fanout`.

---

## The union cap: `cap_fanout`

One function, applied wherever `Values` are produced: after `lift`, in
`eval_call`, and on a walk's returns. No other code checks the union size.

```rust
fn cap_fanout(&mut self, mut vals: Values<'db>) -> Values<'db> {
    dedup(&mut vals);
    let Some(max) = self.limits.max_union_fanout else { return vals };   // None: no cap
    if vals.len() <= max { return vals }

    // Over the cap: first drop what can never produce a tree node, then re-check.
    vals.retain(|v| self.is_project_value(*v) || matches!(v, AbstractValue::Pending(_)));
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
handler = pick()    # ty: A | B | C | D | E | F, cap is 4
handler.run()
```

If only `A` and `B` are project classes, the others could never produce a node,
because non-project callees are not shown. Dropping them first keeps `A.run`
and `B.run`.

`is_project_value(v)` is the project check on `v.definition()`: true for a
`Function`, `Closure`, `BoundMethod`, `Class`, `Instance` or `Module` defined
in a project file. What pruning can lose: a stdlib object holding a project
callback. ty's types rarely express that, and the counter records each prune.

The cap is a precision rule. It acts on values the walk computed from its key,
so it obeys rule 3. Default `Some(4)`, set in
[`13`](13-solver-and-limits.md#limits).

---

## What is not modelled

| not modelled | instead |
|---|---|
| container contents (`list[Handler]`) | `Ty(...)`. ty answers element types if asked |
| dict and tuple destructuring | `Unknown` for the targets |
| `global` / `nonlocal` | `Unknown` |
| generator values, coroutine objects | a call to a generator is `Unknown`; the body is still walked. `await` is transparent |
| branch conditions | every arm, joined |

---

## Sizing

Target: `AbstractValue` ≤ 16 bytes and `Copy`. It is copied into envs and
passed by value constantly.

| variant | payload | size |
|---|---|---|
| `Ty(Type)` | a whole ty `Type` | **16**. ty asserts this (`types.rs:11192`, release + 64-bit only) |
| `Instance`, `BoundMethod`, `Closure` | `Definition` + a `u32` id | 8 |
| `Module` | 2-variant enum | ~8 |
| `Pending` | `DeferId` | 4 |
| others | one salsa id, or nothing | ≤ 8 |

16 bytes holds only if rustc stores the outer tag in `Type`'s spare tag values.
Likely, not promised. Test it under the same conditions ty does:

```rust
#[cfg(all(not(debug_assertions), target_pointer_width = "64"))]
#[test]
fn value_is_small() {
    assert!(std::mem::size_of::<AbstractValue>() <= 16);
}
```

Run with `cargo test --release value_is_small`. If it fails: `Ty(TyId)`, a
`u32` into a per-walk `Vec<Type>` with a dedup map, the same pattern as
`ValueArena`.

---

→ Next: [`05-the-walk.md`](05-the-walk.md)
