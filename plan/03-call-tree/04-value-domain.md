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

**Functions and classes are identified by `Definition<'db>`**, not by ty's
`FunctionType` / `ClassLiteral`. Those two can't be named outside
`ty_python_semantic`: `ClassLiteral` is re-exported `pub(crate)`
(`types.rs:118`) and `FunctionType` is a private `use` (`types.rs:85`), so
`pylspt` can't write them as field types — `types/vnoc.rs` rule 3 says the
same. `Definition<'db>` (`ty_python_core::definition`) is public, a 4-byte
salsa id, and is what the qname, docstring ID and project check start from
anyway. No fork change needed.

```rust
/// A value as seen by the interpreter on one specific path.
/// Small and `Copy` — a few words. Never owns strings or Vecs directly.
/// ★ Private to `interp::value`. Only `lift` and a few helpers there build
///   or match on it; everything else uses those helpers.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(in crate::interp) enum AbstractValue<'db> {
    /// A function with nothing captured: a module-level def, or a function
    /// ty handed us. Its free names are module globals → ty answers them.
    /// The `def` statement's Definition — for a DECORATED function too (below).
    Function(Definition<'db>),

    /// A nested def, as bound by the `def` statement running inside another
    /// function's body. `activation` is THAT run of the enclosing function —
    /// where the def ran, not where it is later called. Lookup reads the run's
    /// current env, so later assignments are seen. See `05` § `enclosing`.
    Closure { func: Definition<'db>, activation: ActivationId },

    /// A class object, i.e. the thing you call to construct.
    Class(Definition<'db>),

    /// An instance of a class. `origin` distinguishes two instances of the same
    /// class constructed at different sites — this is what keeps paths apart.
    Instance { class: Definition<'db>, origin: OriginId },

    /// `receiver.method` — a method with its receiver already bound.
    /// The receiver is interned so this stays Copy.
    BoundMethod { func: Definition<'db>, receiver: ValueId },

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
[`10`](10-return-values-and-state.md)'s value read (which constructs
`Pipeline(...)` because something needs its *value*, e.g. as the receiver of
`.run()`) mint two *different* origins for what is syntactically one
expression, and nothing either of them records about `self.handler` is ever
visible to the other. That was an actual bug in an earlier draft of this
plan, caught by tracing a two-line fixture by hand — see
[`06`](06-attributes-and-self.md)'s fixture 1.

The fix is to make "attrs for this origin" a cache keyed by the origin
itself, computed once, the first time anything asks — the same shape as
`10`'s return store (`ReturnKey`) in [`10`](10-return-values-and-state.md#2-return-value-flow),
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
    class: Definition<'db>,     // the class being built at that site
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
closure handling gives such a class an enclosing run (an `ActivationId`,
[`05`](05-binding-arguments.md#the-environment)), that run is an input too
and goes in the key. Module-level classes (nearly all of them) have
nothing captured.

**Why the two code paths can't disagree.** Only `construct_instance()` builds
an `OriginKey`, and both the tree walk and the value read call it with
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
    p = Pipeline(JsonHandler())   # tree walk AND value read both build this
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

### `ActivationId` — which run a `Closure` belongs to

`Closure` points at one *run* of its enclosing function — one walk of that
body ([`10` §1](10-return-values-and-state.md#1-local-assignment-tracking-cheapest-do-first)).
Two calls merged into one tree frame are two runs.
`Interp.activations` is a per-request `Vec<Env<'db>>`, one slot per run,
holding that run's current env; `ActivationId` is a `u32` index into it.
Slots are never freed before the request ends, because a stored return
value can still point at one ([`10` §2, point 6](10-return-values-and-state.md#the-things-to-get-right)).
Why per run and not per tree frame: [`05`](05-binding-arguments.md#why-the-slot-is-per-run-not-per-tree-frame).

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
    Function(Definition<'db>),
    Closure { func: Definition<'db>, activation: ActivationId },
    Class(Definition<'db>),
    Bound { func: Definition<'db>, receiver: ValueId },
}
```

**Going back to ty.** `vnoc::member` and `vnoc::instance_of` take a
`Type`, so a `Class(def)` or `Instance { class: def, .. }` needs the class's
`Type` again. That's `binding_type(db, def)` — `pub(crate)`
(`types.rs:235`), so one more `vnoc` wrapper, one internal call:
`vnoc::type_of_definition(db, def) -> Type`. **[check]** add it with the
others below.

**Edge-case rules** (checked against the Jedi driver):

- **A decorated function points at its `def`.** ty types a decorated name as
  whatever the decorator returns (`@lru_cache` → a wrapper object), so
  `lift` on ty's type would point at the decorator's class. When the name's
  binding is a `def` statement, use that statement's `Definition` directly.
- **A dataclass gives an edge to the class.** Its `__init__` is synthesised,
  so there's no body: the class node is added, nothing is walked, and the
  instance's attrs stay empty (`member()` falls through to ty's field
  types). Same for `NamedTuple` and other synthesised `__init__`s.

`Closure` behaves exactly like `Function` everywhere (qname, project check,
docstring ID, `definition()`) except one place: `bind_parameters` uses
`activation` as the callee's `enclosing`
([`05`](05-binding-arguments.md#the-environment)). `callee.enclosing()`
returns `Some(activation)` for a `Closure` and `None` for everything else.

`definition()` is the join point with everything in
[`01-what-jedi-actually-does.md`](01-what-jedi-actually-does.md): from a
`Definition` you get the file (→ project check), the name and scope chain
(→ qname), and the body (→ docstring ID, and the AST to walk).

---

## Converting from ty — the boundary

When the environment misses and you ask ty, you get a `Type<'db>` and must lift
it back into your domain:

```rust
const MAX_LIFT_DEPTH: u8 = 8;   // only recursive type aliases get near this

fn lift(&mut self, ty: Type<'db>) -> Values<'db> { self.lift_at(ty, 0) }

/// `Type::definition` is PUBLIC (`types.rs:9216`) and returns a public
/// `TypeDefinition`. It already maps: a function or bound method → the
/// function's def; a class, `Box[int]`, or `type[X]` → the class's def; an
/// instance → its class's def; a module → the module. `lift` leans on it so
/// almost nothing here needs a wrapper.
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

        // A class, or a class-ish type that isn't a ClassLiteral. `Box[int]()`
        // → Box's def → an edge to Box.__init__. (`list[Foo]()` → list,
        // stdlib, so no edge, which is right.)
        Type::ClassLiteral(_) | Type::GenericAlias(_)
                                   => or_ty(self.class_def(ty).map(Class)),

        // `type[Base]` — usually a parameter annotation, reached through the
        // annotation fallback (experience/12-call-tree/00-values/04):
        // `def make(cls: type[Base]): cls()` with no known caller → Class(Base),
        // so `cls()` still gets a `Base.__init__` edge. It means "Base or a
        // subclass"; we take Base, the same way `x: Base` gives a Base instance.
        // `type[Any]` → no def, `type[T]` → a TypeVar def: both Unknown.
        Type::SubclassOf(_) => match self.class_def(ty) {
            Some(d) => smallvec![Class(d)],
            None    => smallvec![Unknown],
        },

        Type::ModuleLiteral(_) => match self.def_of(ty) {
            Some(TypeDefinition::Module(m)) => smallvec![Module(m)],
            _                               => smallvec![Ty(ty)],
        },

        // `obj.method` that ty already bound. Without this arm it becomes
        // Ty(BoundMethod), `as_callable` says no, and the call gets NO edge.
        // The receiver here is ty's `self_instance`, so it lifts to an OPAQUE
        // instance. That's right for a value ty handed us; `member()` on one
        // of OUR instances does not come through here — it binds its own
        // receiver (06 § "Bind your receiver, not ty's").
        Type::BoundMethod(_) => {
            let (Some(func), Some(recv)) = (self.function_def(ty), vnoc::bound_method_self(self.db, ty))
                else { return smallvec![Ty(ty)] };
            self.lift_at(recv, depth + 1).into_iter()      // a union receiver → one each
                .map(|r| BoundMethod { func, receiver: self.intern(r) })
                .collect()
        }

        // ★ the important case: keep alternatives SEPARATE
        // (`elements` is a pub field, set_theoretic.rs:24)
        Type::Union(u)             => u.elements(self.db).iter()
                                        .flat_map(|t| self.lift_at(*t, depth + 1))
                                        .collect(),

        // No origin — this instance didn't come from a construction site we
        // saw. Use a sentinel origin so it can't be confused with one we did
        // see. ⚠ Every opaque instance shares this ONE sentinel, so it must
        // never key attribute state — see below.
        // ⚠ Known gap: a SPECIALISED instance (`b: Box[Foo]`) becomes plain
        // `Instance{Box}` — the `[Foo]` is lost, so `b.get().handle()` has no
        // edge. Later fix, no enum change: keep a specialised opaque instance
        // as `Ty(ty)`, where ty's own member lookup keeps the specialisation.
        Type::NominalInstance(_)   => or_ty(self.class_def(ty)
                                        .map(|class| Instance { class, origin: OriginId::OPAQUE })),

        // `type Handlers = A | B` — lift what it stands for. A union alias
        // explodes through the Union arm like any other union. `value_type`
        // is already `pub` (`types/type_alias.rs:454`), no wrapper needed.
        // Recursive aliases are what MAX_LIFT_DEPTH is for.
        Type::TypeAlias(a)         => self.lift_at(a.value_type(self.db), depth + 1),

        Type::Dynamic(_) | Type::Never | Type::Divergent(_)
                                   => smallvec![Unknown],

        // Deliberately Ty for now: KnownBoundMethod (stdlib special methods),
        // Callable (`cb: Callable[...]` — no definition to point at), TypeVar,
        // Intersection, ProtocolInstance, PropertyInstance (06 handles
        // properties at `member()`), literals. Add an arm only with a fixture
        // that shows a lost project edge.
        other                      => smallvec![Ty(other)],
    }
}
```

**[check]** — two `vnoc` wrappers this needs don't exist yet. Add them
when `lift` is written, one internal call each, public types only (rule 3 at
the top of `types/vnoc.rs`):

| wrapper | wraps | visibility today |
|---|---|---|
| `bound_method_self(db, ty) -> Option<Type>` | `BoundMethodType::self_instance` | `pub(super)` (`types/method.rs:29`) — reachable from `types::vnoc` |
| `type_of_definition(db, def) -> Type` | `binding_type(db, def)` — the way back, for `vnoc::member` | `pub(crate)` (`types.rs:235`) |

(An earlier version of this section needed four wrappers to dig class
literals and functions out of `GenericAlias`, `SubclassOf` and
`BoundMethod`. `Type::definition` already does all of that publicly.)

Fixtures — one per arm, each one a call edge that silently disappears if
the arm is missing:

```python
# 1. BoundMethod from ty (not from our member()) — edge to Handler.handle
handler = Handler()
dispatch = handler.handle      # module global: env never sees it, ty types it
def use():
    dispatch(1)                # ty: Type::BoundMethod → lift → callable

# 2. GenericAlias — edge to Box.__init__
class Box(Generic[T]):
    def __init__(self, v: T): ...
def make(): return Box[int](1)

# 3. SubclassOf, no known caller — edge to Base.__init__
def build(cls: type[Base]):
    return cls()

# 4. TypeAlias to a union — edges to A.run AND B.run
type H = A | B
def go(h: H):
    h.run()

# 5. KNOWN GAP — specialisation lost on an annotated value. Today: no
#    Foo.handle edge. Pins the gap; flips when lift keeps `Ty(Box[Foo])`.
def run(b: Box[Foo]):
    b.get().handle()

# 6. decorated function → edge to the `def`, not the decorator's wrapper class
@lru_cache
def load(): ...
def use(): load()

# 7. dataclass → edge to the class; no __init__ body to walk
@dataclass
class Job: handler: Handler
def make(): Job(Handler())
```

**The `Union` arm is the one that matters.** ty hands you a set; you must
explode it into separate values so each produces its own subtree. Flatten it
into `Type::Union` again anywhere and you've silently rebuilt
`ty_ide::outgoing_calls`.

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
`eval_call`, in `walk_function`
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
| `Instance` | `Definition` + `OriginId` | 8 |
| `BoundMethod` | `Definition` + `ValueId` | 8 |
| `Closure` | `Definition` + `ActivationId` | 8 |
| `Module` | `Module` (2-variant enum) | ~8 |
| others | one salsa id, or nothing | ≤ 8 |

`Ty(Type)` alone fills 16 bytes, so `AbstractValue` stays at 16 only if rustc
stores the outer tag in `Type`'s unused tag values and fits the other
variants around it (niche filling). That is likely here — `Type`'s tag byte
has spare values and the other payloads (8 bytes at most, now that
functions and classes are `Definition`s) fit in the remaining 12 — but
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
pattern as `ValueArena` below. Then the largest payload is 8 bytes and the
enum fits in 16 without help from the compiler. The cost is
one array lookup when reading a `Ty` value, and `Ty` is the "we don't model
this" case, not the hot path.

---

→ Next: [`05-binding-arguments.md`](05-binding-arguments.md)
