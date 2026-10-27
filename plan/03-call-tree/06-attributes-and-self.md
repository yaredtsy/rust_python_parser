# 03.06 — Attributes, `self`, and object state

You named this explicitly: *"it works for callback, attribute calls, stuff."*
This is the attribute-call chapter. It is the second-hardest thing after
argument binding, and the component the fork shrinks most
([`01-crates/04`](../01-crates/04-public-vs-private-api.md)): its lookup half
becomes one wrapper call, and its identity half stays entirely yours.

---

## The shape of the problem

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler          # ← state written here
        self.name = "pipe"

    def run(self, data):
        self.handler.process(data)      # ← read here, in a DIFFERENT method

def main():
    Pipeline(JsonHandler()).run(x)      # → JsonHandler.process
    Pipeline(CsvHandler()).run(y)       # → CsvHandler.process
```

Three separate mechanisms must cooperate:

1. **`self` is bound** to a specific instance (`bind_parameters`, done in [`05`](05-binding-arguments.md)).
2. **`__init__` writes** `self.handler` — that write must be *recorded*.
3. **`run` reads** `self.handler` — must see the write from *this* path's `__init__`.

Jedi gets this via `TreeInstance` holding the constructing `arguments`, so
attribute access re-infers `self.handler` in a context where `handler` is bound.

---

## The design: an attribute store keyed on origin

From [`04-value-domain.md`](04-value-domain.md), `Env` carries:

```rust
attrs: rpds::HashTrieMap<(OriginId, Name), Values<'db>>
```

`OriginId` identifies *which* instance. Two `Pipeline(...)` calls get different
origins, so their `handler` attributes never collide.

### Writing: scan `__init__` for `self.X = ...`

**Called from exactly one place:** `construct_instance()` in
[`10`](10-return-values-and-state.md#2-return-value-flow), the first time a
given `OriginId` is touched. Not from `resolve_call`'s tree-walk branch
directly, and not inline wherever a constructor call happens to be seen —
see [`04`](04-value-domain.md#-origin_attrs--where-a-constructors-writes-actually-live)
for why a second, independent caller is exactly the bug this chapter used to
have: two callers mint two origins for one `Foo()` expression, and whichever
one runs `record_self_assignments` leaves the other with empty attrs.

```rust
fn record_self_assignments(
    &self,
    init: Definition<'db>,
    init_env: &Env<'db>,
    instance_origin: OriginId,
) -> rpds::HashTrieMap<(OriginId, Name), Values<'db>> {
    // ★ No `is_name(&attr.value, self_param)` filter. That filter capped writes
    //   at depth 1 (`self.x = v`) and silently dropped `self.a.b = v`. Every
    //   attribute target goes through `bind_attribute` (below), the SAME write
    //   path used outside `__init__`, so any depth works and the two
    //   cannot drift apart.
    //
    // `init_env` already binds `self` -> Instance{C, instance_origin}, so
    // `self.x = v` is just the receiver `self` evaluating to that origin.
    let mut env = init_env.clone();
    env.attrs = self.origin_attrs.clone();       // the Interp-level cache, not init_env's
    // The SAME walker as every other body (10 § walk_function): sequential
    // writes replace, `if`/`else` arms fork and join. In practice this runs
    // as part of the tree walk of `__init__` — see 10's construct_instance —
    // and only runs with node output off for a construction the tree didn't reach.
    match self.walk_function(init, &env, Mode::value()) {
        Some(end) => end.attrs,
        None      => env.attrs,    // every path returns early — keep the baseline
    }
}
```

**Deliberately not a fixed-point analysis.** One pass: a later write
replaces an earlier one, and `if self.json: self.h = JsonHandler() else:
self.h = CsvHandler()` gives `self.h` **both** — every arm from the state
before the `if`, unioned at the join, conditions never evaluated. That is
the "no branch analysis" scope limit from
[`01`](01-what-jedi-actually-does.md#what-it-does-not-do-scope-limits--respect-them),
and it keeps this O(size of `__init__`). (An earlier draft did "one linear
pass, last write wins" through the arms too, which kept only the `else`
handler. Retired with the old `body_statements` loop.)

Known limit: writes made on an arm that then `return`s early
(`if not cfg: self.h = None; return`) don't reach the end env, so they're
lost. Only the paths that reach the end of `__init__` count.

### Reading: `member()` on a value

```rust
fn member(self, cx: &Cx<'db>, name: &str) -> Values<'db> {
    match self {
        AbstractValue::Instance { class, origin } => {
            // 1. ★ path-specific instance state — the precise answer.
            //    Two layers, checked in order: `Env.attrs` first (sequential
            //    writes AFTER construction — 10 §3b's `p.handler = X()`,
            //    which really is path-and-order-dependent and lives in the
            //    env that's threaded statement by statement), then
            //    `origin_attrs` (the constructor's baseline, memoised once
            //    per origin by `construct_instance()` — 04, 10 §2). A later write
            //    shadows the constructor's value for the same name; neither
            //    layer is consulted if the other already answered.
            //    Skipped for OPAQUE: that one origin is shared by every
            //    instance ty handed us, so it never holds state (04).
            if origin != OriginId::OPAQUE {
                if let Some(v) = cx.env.attrs.get(&(origin, name.into()))
                    .or_else(|| cx.origin_attrs.get(&(origin, name.into())))
                {
                    return v.clone();
                }
            }

            // 2. class body: methods, class vars, MRO — ty's own lookup via
            //    vnoc::member, with OUR receiver bound on a method. Full code in
            //    § "Bind your receiver, not ty's" below. (`class` is a
            //    Definition, 04; `pylspt` can't name ty's ClassLiteral, so it
            //    can't walk the MRO by hand — the wrapper is the only route.)
            if let Some(v) = cx.class_member_with_receiver(self, class, name) {
                return v;
            }

            // 3. give up gracefully — ty knows about descriptors, __getattr__,
            //    dataclass fields, properties, slots, metaclass attrs
            cx.ty_member(Type::instance(class), name)
        }

        AbstractValue::Class(c)  => /* unbound: methods stay unbound */,
        AbstractValue::Module(m) => /* module-level symbol lookup */,
        AbstractValue::Ty(t)     => cx.ty_member(t, name),
        _                        => smallvec![AbstractValue::Unknown],
    }
}
```

**The order is the design.** Instance state (path-specific) beats class members
(path-free) beats ty (fully general). Each fallback loses precision and gains
coverage.

Step 2 producing a `BoundMethod` with an interned receiver is what makes
`self.handler.process(data)` work end to end: `self` → `Instance`,
`.handler` → step 1 hit → the `JsonHandler` instance, `.process` → step 2 →
`BoundMethod{process, receiver: that JsonHandler}`, and `bind_parameters` then
binds `self` correctly inside `process`.

---

## Attribute chains: any depth, read and write

`Env` maps names to `Values`, and an `Instance` value carries its own
`OriginId`, so every hop of `a.b.c.f` is the same one-step lookup. Depth is
not a property of the store. It falls out of `eval` being recursive on
`attr.value`.

### Read: the `Expr::Attribute` arm of `eval`

```rust
Expr::Attribute(attr) => {
    // recurse FIRST: `a.b.c` evaluates `a.b`, which evaluates `a`
    let recvs = self.eval(&attr.value, env, model);
    let mut out = Values::new();
    for recv in recvs.into_iter().take(MAX_CHAIN_FANOUT) {     // cap per hop
        out.extend(recv.member(cx, &attr.attr.id));            // tiers 1-3
        if out.len() >= MAX_CHAIN_FANOUT { self.stats.chain_fanout_hits += 1; break; }
    }
    dedup(&mut out);
    if out.is_empty() { smallvec![AbstractValue::Unknown] } else { out }
}
```

- **Mid-chain `Unknown`:** `Unknown.member(..)` is `[Unknown]` (the `_` arm of
  `member`), so the rest of the chain stays `Unknown` and the call at the end
  is simply unresolved. Nothing panics, nothing is guessed.
- **Cap:** applied on every hop, after dedup. Duplicates (two paths reaching the
  same instance) cost nothing. Only genuinely distinct values count toward the
  cap.
- **A `@property` hop:** `member` yields a property marker and this arm
  invokes the getter ([`06`-classmethod exercise](../../experience/12-call-tree/02-classes/06-classmethod-staticmethod-property.md)),
  so `s.active_handler.handle(3)` is also a chain.
- **A call mid-chain** (`make().b.c()`): `attr.value` is a `Call`, which `eval`
  already resolves via return-value flow ([`10`](10-return-values-and-state.md)).

### Write: `bind_attribute`, one function for every depth

```rust
fn bind_attribute(&mut self, attr: &ast::ExprAttribute, v: Values<'db>, mut env: Env<'db>) -> Env<'db> {
    // `a.b.c = v` : the receiver is `a.b`, evaluated by the chain rule above.
    let recvs: Vec<_> = self.eval(&attr.value, &env, model).into_iter()
        .filter_map(|r| match r {
            // OPAQUE is one origin shared by every instance ty gave us, so a
            // write keyed on it would show up on unrelated objects (04).
            AbstractValue::Instance { origin, .. } if origin != OriginId::OPAQUE => Some(origin),
            _ => None,
        })
        .collect();
    let strong = recvs.len() == 1;                       // exactly one candidate -> replace
    for origin in recvs {
        env = env.bind_attr(origin, &attr.attr.id, v.clone(), strong);   // else union with old
    }
    env    // no traced Instance in the receiver values -> write dropped (deliberate, 10 §3b)
}
```

This is the **only** `bind_attribute`. [`10` §3b](10-return-values-and-state.md#b-any-receiver-not-just-self)
uses this one; it doesn't define its own.

Only the LAST name is written. Everything before it is a read. `a.b.c = X()`
writes `(origin_of(a.b), "c")`, and a later `a.b.c.f()` reads it back through
the same origin, so write and read agree at any depth.

Two points that differ from a naive version:

- **Write to every Instance receiver**, not just the first. If `a.b` is ambiguous
  between two instances, either may have been mutated. Binding both is a sound
  over-approximation, and it keeps the "no sound way to invalidate" refusal for
  *unknown* receivers separate from the *ambiguous but known* case. This is a
  weak update (union with the old value), not a replacement, unless exactly one
  Instance matched.
- **Intermediate hops are never created.** `a.b.c = v` where `a.b` has no
  entry does NOT mint a `b`. `a.b` evaluates to `Unknown`, so the write drops.
  Python raises `AttributeError` there too.

### Module chains: `pkg.sub.mod.f()`

`member` on `Module(m)`:

1. a symbol `name` defined in `m` -> `lift` it (function, class, constant);
2. else a **submodule** named `m.name` that the program has imported
   (`import pkg.sub.mod` binds `pkg`, and makes `pkg.sub` and `pkg.sub.mod`
   reachable as attributes) -> `Module(that)`;
3. else `cx.ty_member(Type::module(m), name)`.

Tier 2 is what makes a 3-deep module chain work. Without it `pkg.sub`
resolves to nothing, because `sub` is not a symbol *in* `pkg/__init__.py`.
Whether a submodule is reachable depends on the importing statements in
scope. `import pkg` alone does NOT expose `pkg.sub` unless `pkg/__init__.py`
itself imports it, and the lookup must not either.

Module and instance hops mix freely (`config.SERVICE.client.send()`), since
each hop only looks at its own receiver's variant.

> **Decision — weak updates.** `bind_attr(.., strong)` replaces the old value
> only when exactly one Instance matched. With several candidates, python may
> have written only one of them, so each gets `old ∪ new`.

---

## Attributes written outside `__init__`

```python
def configure(self):
    self.handler = OtherHandler()      # not in __init__
```

Options:
- **(a)** Only scan `__init__`. Simple, matches most code.
- **(b)** ★ When walking *any* method body with a bound `self`, record
  `self.X = ...` into the env going forward. The assignment scan is already
  written — just call it during `walk_body` too and let it extend the env for
  subsequent statements in that body.
- **(c)** Whole-class fixed point. No.

**Build (b).** It is a small delta on (a), and
[`10`](10-return-values-and-state.md#3-object-state-beyond-__init__)
generalises it further — to *any* receiver, not just `self`, so
`p.handler = X()` from outside the class works through the same code path.
This writes into `Env.attrs` (the sequential layer `member()` checks first —
see above), **not** `origin_attrs` — it's a later, order-dependent write on
top of the constructor's memoised baseline, not a correction to it. Note it
makes the env
mutable *during* a body walk, which conflicts with the "envs are immutable"
invariant from [`03`](03-the-abstract-interpreter.md). Resolve it by threading
the env through the statement loop by value:

That loop is [`10` §1](10-return-values-and-state.md#1-local-assignment-tracking-cheapest-do-first)'s
`walk_stmts`: calls in a statement first, using the env before it, then
the statement's bindings; `if`/`else` arms forked from the same "before"
and joined after. (An earlier sketch here recorded bindings *before*
resolving the statement's calls, the opposite order. Use `10`'s.)

That keeps each env immutable once shared with a child frame, while allowing
sequential refinement within one body. Statement order becomes meaningful —
which is correct.

---

## Inherited attributes

```python
class Base:
    def __init__(self): self.log = Logger()
class Child(Base):
    def __init__(self): super().__init__(); self.x = 1
    def go(self): self.log.write("hi")     # ← from Base.__init__
```

**Does not fall out for free** under the memoised design above —
`record_self_assignments` only walks the `init` it was handed
(`body(init)`), so a plain call wouldn't see past
`Child.__init__`'s own body into `Base.__init__`'s. It needs one explicit
case: scanning `Child.__init__`'s body, a statement that is a call to
`super().__init__(...)` (receiver resolves, via `super()`, to the same
instance per [`03`](03-the-abstract-interpreter.md)'s `super()` handling)
recurses `record_self_assignments` on the resolved `Base.__init__`, folding
its assignments into the SAME `attrs` map before continuing the scan of
`Child.__init__`. Bounded by the MRO's length, so no fixed-point machinery —
still one pass per `__init__` in the chain, same join rules.

With that one case added, `Base.__init__`'s `self.log = Logger()` writes into
the same `(origin, "log")` slot and `Child.go` sees it — *provided* `super()`
resolves. **[check]** — verify what your current driver actually produces for
the fixture above before deciding how much to invest.

```rust
// eval: Call { func: Name("super"), args: [] } in a method body
//   → AbstractValue::Instance { class: <next in MRO>, origin: <same origin> }
```

**[check]** — Jedi handles `super()` via bound-method machinery; verify what your
current driver actually produces for the fixture above before deciding how much
to invest. If it produces nothing today, produce nothing.

---

## ★ Step 2 is now one wrapper call

Step 2 needs "member `m` of **this** value, walking the MRO". That is
`vnoc::member`, the second of the two wrappers in the fork
([`01-crates/04`](../01-crates/04-public-vs-private-api.md)):

```rust
// crates/ty_python_semantic/src/types/vnoc.rs
/// `getattr(<value of type ty>, name)`, as ty resolves it.
pub fn member<'db>(
    db: &'db dyn Db,
    env: &ProgramEnvironment<'db>,
    ty: Type<'db>,
    name: &str,
) -> Option<Type<'db>> {
    ty.member(db, env, name).ignore_possibly_undefined()
}
```

**[verified at `ac201b8`]** — `Type::member(self, db, env, name) ->
PlaceAndQualifiers` is **private** to `crate::types` (`types.rs:4929`), and
`PlaceAndQualifiers::ignore_possibly_undefined` is `pub(crate)`
(`place.rs:923`). A child module of `types` reaches both; nothing outside the
crate reaches either.

You construct the `env` yourself: `ProgramEnvironment::from_file(program_file)`
is `pub` (`types/context.rs:42`, re-exported at `types.rs:15` **[verified]**),
and `program_file` is what `db.program_file(file)` already gives you.

### What that deletes from this chapter

The `member.rs` this chapter budgeted — MRO walk, instance vs class attributes,
the descriptor protocol, properties, `__getattr__`, metaclasses — is a second
copy of Python's attribute semantics, and it would disagree with ty precisely
where ty is right. It is gone. `vnoc::member` is ty's own lookup, on a receiver
**you** chose.

### ⚠ Bind *your* receiver, not ty's

For a method, `vnoc::member` returns a `Type::BoundMethod` whose receiver is
ty's idea of the instance — a plain `Pipeline` type, no origin. Passing that
through `lift` ([`04`](04-value-domain.md#converting-from-ty--the-boundary))
would give `BoundMethod { func, receiver: Instance{Pipeline, OPAQUE} }`, and
inside `run`, `self.handler` would find no state: opaque instances have none.
So when step 2's answer is a bound method, keep only its function and bind
the receiver `member()` was called on:

```rust
// step 2, on AbstractValue::Instance { class, origin } = self — `class` is a Definition
let class_ty = vnoc::type_of_definition(cx.db, class);             // 04 § Going back to ty
let Some(recv_ty) = vnoc::instance_of(cx.db, cx.env(), class_ty)
    else { return smallvec![AbstractValue::Unknown] };
match vnoc::member(cx.db, cx.env(), recv_ty, name) {
    Some(t @ Type::BoundMethod(_)) => match cx.function_def(t) {     // public Type::definition
        Some(func) =>
            smallvec![AbstractValue::BoundMethod { func, receiver: cx.intern(self) }],  // ★ OUR origin
        None => cx.lift(t),
    },
    Some(t) => cx.lift(t),          // class var, nested class, property, …
    None    => smallvec![AbstractValue::Unknown],
}
```

`lift`'s own `BoundMethod` arm is for bound methods ty hands you *without*
a receiver of yours: `f = get_obj().method`, a bound method stored in a
module global, or a return value ty typed.

### What it does **not** delete

Everything above this section. The wrapper answers "what is `.handle` on a
`LoudHandler`?"; it has no opinion about *which* receiver `self.handler` holds
on this path. That is the attribute store, the `OriginId`, the per-object state
and the frame — the parts only you can write, and the reason Option C survives
the fork.

So the division is exactly:

| question | answered by |
|---|---|
| which object is `self.handler` on this path? | **your** attribute store, keyed on origin |
| what is `.handle` on that object's type? | `vnoc::member` |
| is that definition project code, and does it have an `ID:`? | **your** filters |

### The fallback, still worth knowing

If the fork ever becomes untenable, the best public-API strategy is:
`definitions_for_attribute` on the real AST node, then filter the returned
definitions by whether their enclosing class is in your value's MRO — let ty
over-approximate, then narrow with your path knowledge. It is also a reasonable
**cross-check** for step 3 while the wrapper is new: two routes, same answer.

The other public option — walking the class body AST and matching `def` names
yourself — loses properties, descriptors, dataclass fields, `__slots__`,
metaclass attributes and inherited C-level members. Do not.

---

## Fixtures

```python
# 1. constructor-injected handler, two instances
Pipeline(JsonHandler()).run(x)
Pipeline(CsvHandler()).run(y)

# 2. two instances alive at once, distinct state
a = Pipeline(JsonHandler()); b = Pipeline(CsvHandler())
a.run(1); b.run(2)                     # must NOT cross-contaminate

# 3. attribute chain, read
self.client.session.send(req)
# 3b. attribute chain, write then read, mid-chain Unknown, ambiguous link
a.b.c = X(); a.b.c.f()
# 3c. module chain
pkg.sub.mod.f()

# 4. inherited attribute via super().__init__()
# 5. assignment outside __init__
# 6. @property returning a handler
# 7. classmethod constructor:  Pipeline.create(JsonHandler()).run(x)
# 8. attribute on a module:  config.HANDLER.process(x)

# 9. opaque instances never share state
a = get_logger(); b = get_db()        # both opaque (ty-typed, not constructed by us)
a.handler = FileHandler()
b.handler.write()                     # must NOT resolve to FileHandler.write

# 10. one constructor expression, different args (04's OriginKey, case B)
def make(h): return Pipeline(h)
make(JsonHandler()).run(1)            # → JsonHandler.process
make(CsvHandler()).run(2)             # → CsvHandler.process, not JsonHandler
```

Fixture 2 is the one that proves `OriginId` is doing its job. If `a` and `b`
share state, your origin allocation is wrong. Make it the first test you write.
Fixtures 9 and 10 are the same bug from the other two directions: too few
distinct origins (all opaque instances share one) and a key missing an input
(the arguments).

---

→ Next: [`07-callbacks-and-higher-order.md`](07-callbacks-and-higher-order.md)
→ Generalised (any receiver, any method, plus return-value flow):
  [`10-return-values-and-state.md`](10-return-values-and-state.md)
