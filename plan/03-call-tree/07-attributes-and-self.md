# 03.07 — Attributes and `self`

Attribute reads, attribute writes, and the one wrapper into ty that answers
"what is `.name` on this type".

---

## The shape of the problem

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler          # written here
    def run(self, data):
        self.handler.process(data)      # read here, in a different method

def main():
    Pipeline(JsonHandler()).run(x)      # JsonHandler.process
    Pipeline(CsvHandler()).run(y)       # CsvHandler.process
```

Three things cooperate:

1. `self` is bound to a specific object ([`06`](06-binding-arguments.md)).
2. `__init__`'s write to `self.handler` reaches the caller as an effect on the
   new object ([`10`](10-one-call.md#constructors)).
3. `run`'s key holds that object's `handler`, so `run`'s walk starts with it
   ([`08`](08-keys.md)).

---

## Where attribute state lives

Only in `Env.attrs`, keyed `(OriginId, Name)`. It holds:

- the attributes the key carried, written by `materialize` ([`08`](08-keys.md#materializing));
- writes made in this walk;
- effects and new objects applied from callees ([`10`](10-one-call.md#phase-2-write)).

There is no other store. A constructor is a summary like any other, and its
writes to `self` arrive as effects.

`OPAQUE` and `Beyond` origins hold no state ([`04`](04-values.md#originid-which-object)).

---

## Reading: `member`

```rust
fn member(s: &mut Solver<'db>, w: &mut Walk<'db>, recv: AbstractValue<'db>,
          name: &Name, site: SiteId, env: &Env<'db>) -> Values<'db> {
    match recv {
        AbstractValue::Instance { class, origin } => {
            // 1. state this walk knows. The precise answer.
            if w.holds_state(origin) {
                if let Some(v) = env.attrs.get(&(origin, name.clone())) { return v.clone() }
            }
            // 2. the class side: methods, class variables, properties, declared
            //    instance attributes. ty's own lookup, with OUR receiver bound.
            let found = class_member_with_receiver(s, w, recv, class, name, site, env);
            // 3. an input object's attribute the key didn't hold: name it by its path.
            w.track_entry_attr(origin, name, found)
        }
        AbstractValue::Class(c)   => class_attribute(s, w, c, name),     // methods stay unbound
        AbstractValue::Module(m)  => module_member(s, w, m, name),       // below
        AbstractValue::Ty(t)      => s.lift(w, ty_member(t, name)),
        AbstractValue::Pending(_) => unreachable!("a pending receiver is a deferred Member (12)"),
        _                         => smallvec![AbstractValue::Unknown],
    }
}
```

**The order is the design.** Path-specific state beats the class, and the class
beats ty's general answer. Each step down trades precision for coverage.

Tracing `self.handler.process(data)` inside `run`: `self` is the entry object
for parameter 0. `.handler` hits step 1, because `run`'s key carried it.
`.process` is a second `member` call on that handler, and hits step 2.

### Bind your receiver, not ty's

Step 2 is `vnoc::member`. For a method, it returns a `Type::BoundMethod` whose
receiver is ty's idea of the instance: a plain type, no origin. Lifting that
would give an `OPAQUE` receiver, and inside the method `self.handler` would find
no state. So when step 2's answer is a bound method, keep only its function and
bind the receiver `member` was called on:

```rust
// step 2, recv = Instance { class, origin }
let class_ty = vnoc::type_of_definition(s.db, class);
let Some(recv_ty) = vnoc::instance_of(s.db, s.env(), class_ty)
    else { return smallvec![AbstractValue::Unknown] };
match vnoc::member(s.db, s.env(), recv_ty, name) {
    Some(t @ Type::BoundMethod(_)) => match s.function_def(t) {
        Some(func) if s.is_property(func) =>
            call_getter(s, w, func, recv, site, env),                            // properties, below
        Some(func) =>
            smallvec![AbstractValue::BoundMethod { func, receiver: w.intern(recv) }],   // OUR receiver
        None => s.lift(w, t),
    },
    Some(t) => s.lift(w, t),        // class variable, nested class, declared attribute
    None    => smallvec![AbstractValue::Unknown],
}
```

`lift`'s own `BoundMethod` arm is for bound methods ty hands the walk without a
receiver of its own ([`04`](04-values.md#lift-from-ty-back-into-values)).

### Properties

`s.active_handler` has no parentheses, but it is a call. When step 2 finds a
`@property`, `member` calls the getter right there, as a project call with the
receiver bound ([`10`](10-one-call.md)), at the attribute expression's site. The
call is recorded as an edge, and the value is the getter's returns. Without
this, `s.active_handler.handle(3)` would look for `.handle` on the uncalled
getter and find nothing.

### Objects the key does not reach

A key holds a caller's objects to `fp_depth` steps ([`08`](08-keys.md)). An
attribute the key didn't hold falls to step 2, and ty's answer for it lifts to
an `OPAQUE` instance. A write to that object would be dropped, and the caller
would never hear of it:

```python
class Sink:
    def __init__(self, h):
        self.h = h

def rewire(svc):
    svc.app.config.sink.h = JsonHandler()

def main():
    svc = Service(App(Config(Sink(CsvHandler()))))
    rewire(svc)
    svc.app.config.sink.h.process()     # Python: JsonHandler.process
```

With `fp_depth = 2`, `rewire`'s key stops at `svc.app.config` as
`Opaque(Config)`. Dropping the write leaves `main` with `CsvHandler`: the tree
shows an edge Python never takes and misses the one it does, with no mark.

So step 3: when the receiver is an input object (origin `Entry(path)`) and the
answer is an `OPAQUE` instance, the value gets the origin
`Entry(path + [Attr(name)])` instead. It still holds nothing the key didn't
give. It only names where the object lives. A write to it is tracked like any
write to an input object, and the summary reports it as an effect at that path
([`09`](09-summaries.md#finishing-a-walk)). `main` follows the path in its own
state, finds its real `Sink`, and writes `h`.

This obeys rule 1. `rewire` never reads `main`'s deep values: the class comes
from ty, and the path is a location, not a value. Returning such an object gives
`Ref(path)`, so the caller gets its own object back, attributes included.

Paths are capped at `max_path` steps ([`13`](13-solver-and-limits.md#limits)).
Past the cap, the object gets `Beyond { at, attr }`: `at` is the deepest path
that fits, `attr` the first step past it. It holds no state, and every object
below it shares that origin. A write to a `Beyond` object becomes the fallback
effect: a weak `Unknown` written to `at.attr`. The summary gets the
`DEEP_WRITE` gap, and `Stats.deep_write_fallbacks` counts it. The caller then
sees "old value or Unknown", which is incomplete but never silently wrong, and
the node is marked.

`track_entry_attr` does nothing for any other origin: an object built in this
walk has all its state in the env, and `OPAQUE` stays `OPAQUE`.

---

## Attribute chains: any depth

`Env` maps names to `Values`, and an `Instance` carries its own origin, so each
hop of `a.b.c.f` is the same one-step lookup. Depth falls out of `eval` being
recursive on `attr.value`.

```rust
fn eval_attribute(s: &mut Solver<'db>, w: &mut Walk<'db>, attr: &ast::ExprAttribute,
                  env: &Env<'db>) -> Values<'db> {
    let recvs = eval(s, w, &attr.value, env);          // `a.b.c` evaluates `a.b`, which evaluates `a`
    if recvs.is_empty() { return Values::new() }       // no value reaches here, so none reaches `.name` either
    if has_pending(&recvs) { return smallvec![w.defer_member(&recvs, &attr.attr.id, env)] }   // 12
    let mut out = Values::new();
    for recv in recvs.into_iter().take(s.limits.max_chain_fanout) {
        out.extend(member(s, w, recv, &attr.attr.id, w.site_of(attr), env));
        if out.len() >= s.limits.max_chain_fanout { s.stats.chain_fanout_hits += 1; break }
    }
    dedup(&mut out);
    if out.is_empty() { smallvec![AbstractValue::Unknown] } else { out }
}
```

- **Mid-chain `Unknown`:** `member` on `Unknown` is `[Unknown]`, so the rest of
  the chain stays `Unknown` and the final call has no target. Nothing panics,
  nothing is guessed.
- **The cap** applies per hop, after dedup. Two paths to one object cost
  nothing.
- **A call mid-chain** (`make().b.c()`): `attr.value` is a call, which `eval`
  resolves through its summary.

### Module chains: `pkg.sub.mod.f()`

`member` on `Module(m)`:

1. a symbol `name` defined in `m`: lift it;
2. else a submodule `m.name` that the program imported (`import pkg.sub.mod`
   makes `pkg.sub` and `pkg.sub.mod` reachable): `Module(that)`;
3. else ty's member of the module type.

Step 2 is what makes a three-deep module chain work: `sub` is not a symbol in
`pkg/__init__.py`. `import pkg` alone does not expose `pkg.sub` unless
`pkg/__init__.py` imports it, and the lookup must not either. Module and
instance hops mix freely (`config.SERVICE.client.send()`).

---

## Write: `bind_attribute`

One function, for every depth and every receiver.

```rust
fn bind_attribute(s: &mut Solver<'db>, w: &mut Walk<'db>, attr: &ast::ExprAttribute,
                  v: Values<'db>, mut env: Env<'db>) -> Env<'db> {
    // `a.b.c = v`: the receiver is `a.b`, evaluated by the chain rule above.
    let recvs = eval(s, w, &attr.value, &env);
    if has_pending(&recvs) { w.defer_write(&recvs, &attr.attr.id, v, &env); return env }   // 12
    if v.is_empty() { return env }                        // no value reaches here: nothing is written

    let targets: SmallVec<[OriginId; 2]> = recvs.iter().filter_map(|r| match r {
        AbstractValue::Instance { origin, .. } if w.holds_state(*origin) => Some(*origin),
        AbstractValue::Instance { origin, .. } if w.is_beyond(*origin) => { w.deep_write(*origin); None }
        _ => None,                                       // OPAQUE, Unknown, Ty: dropped
    }).collect();

    let strong = targets.len() == 1;                     // one candidate → replace
    for origin in targets {
        env = env.bind_attr(origin, &attr.attr.id, v.clone(), strong);   // records Must or May
    }
    env
}
```

Only the last name is written. Everything before it is a read. `a.b.c = X()`
writes `(origin_of(a.b), "c")`, and a later `a.b.c.f()` reads it back through
the same origin.

- **Several candidates → weak update.** If `a.b` may be two objects, Python
  wrote only one of them, so each gets `old ∪ new` and the write is `May`.
  Replace only when exactly one object matched.
- **Intermediate hops are never created.** If `a.b` is `Unknown`, the write is
  dropped. Python raises `AttributeError` there too.
- **Unknown receivers are dropped on purpose.** There is no sound way to model
  "some object of unknown identity changed" without invalidating everything,
  and guessing would corrupt unrelated paths. `OPAQUE` counts as unknown
  identity, because it is shared.
- **A write anywhere is the same write.** In `__init__`, in another method, or
  from outside the class (`p.handler = X()`). `self.x = v` is just the case
  where the receiver is parameter 0. There is no `__init__` special case.

---

## Inherited attributes and `super()`

```python
class Base:
    def __init__(self): self.log = Logger()
class Child(Base):
    def __init__(self): super().__init__(); self.x = 1
    def go(self): self.log.write("hi")       # written by Base.__init__
```

`super().__init__()` is an ordinary method call on the same receiver. `super()`
in a method evaluates to a lookup handle: `super().name` looks `name` up in the
receiver's MRO after the method's own class, and binds the result to the same
receiver value. `Base.__init__`'s write to `self.log` arrives as an effect on
parameter 0, which is the `Child` object, so `Child.go` finds it.

**[check]** that ty's lookup on a `super()` type returns a bound method whose
function is `Base.__init__`, and what the Jedi driver produces for this fixture.
If Jedi produces nothing, produce nothing.

---

## `vnoc::member`: the class side in one call

```rust
// crates/ty_python_semantic/src/types/vnoc.rs
/// `getattr(<value of type ty>, name)`, as ty resolves it.
pub fn member<'db>(db: &'db dyn Db, env: &ProgramEnvironment<'db>, ty: Type<'db>, name: &str)
    -> Option<Type<'db>>
{
    ty.member(db, env, name).ignore_possibly_undefined()
}
```

**[verified at `ac201b8`]** `Type::member` is private to `crate::types`
(`types.rs:4929`), and `PlaceAndQualifiers::ignore_possibly_undefined` is
`pub(crate)` (`place.rs:923`). A child module of `types` reaches both.
`ProgramEnvironment::from_file(program_file)` is `pub` (`types/context.rs:42`).

It is ty's own lookup: MRO, descriptors, properties, `__getattr__`, metaclasses,
dataclass fields. Writing that again would give a second copy of Python's
attribute semantics that disagrees with ty exactly where ty is right.

| question | answered by |
|---|---|
| which object is `self.handler` on this path? | **your** env, keyed on origin |
| what is `.handle` on that object's type? | `vnoc::member` |
| is that definition project code, with an `ID:`? | **your** filters, at render |

If the fork ever becomes untenable, the public fallback is
`definitions_for_attribute` on the AST node, filtered by whether each result's
class is in the value's MRO. It is also a useful cross-check while the wrapper
is new.

---

## Fixtures

```python
# 1. constructor-injected handler, two instances
Pipeline(JsonHandler()).run(x)          # JsonHandler.process
Pipeline(CsvHandler()).run(y)           # CsvHandler.process

# 2. two instances alive at once: no cross-talk. Write this one first.
a = Pipeline(JsonHandler()); b = Pipeline(CsvHandler())
a.run(1); b.run(2)

# 3. chains: read, write then read, module chain
self.client.session.send(req)
a.b.c = X(); a.b.c.f()
pkg.sub.mod.f()

# 4. inherited attribute via super().__init__()
# 5. assignment outside __init__
class C:
    def setup(self): self.h = JsonHandler()
    def go(self):    self.h.process()
# 6. @property returning a handler:  s.active_handler.handle(3)
# 7. classmethod constructor:         Pipeline.create(JsonHandler()).run(x)
# 8. attribute on a module:           config.HANDLER.process(x)

# 9. opaque instances never share state
a = get_logger(); b = get_db()
a.handler = FileHandler()
b.handler.write()                       # must NOT resolve to FileHandler.write

# 10. one constructor expression, different arguments
def make(h): return Pipeline(h)
make(JsonHandler()).run(1)              # JsonHandler.process
make(CsvHandler()).run(2)               # CsvHandler.process

# 11. external mutation
p = Pipeline(); p.handler = JsonHandler(); p.run()

# 12. a write deeper than the key: the rewire example above
#     main → JsonHandler.process only. No CsvHandler.process.

# 13. a write past max_path (run with max_path = 2)
#     main keeps CsvHandler.process, gains nothing, and rewire is marked Incomplete.
```

Fixture 2 proves origins keep objects apart. Fixtures 9 and 10 are the same
bug from two other directions: too few distinct origins (every opaque instance
shares one), and a key missing an input.

---

→ Next: [`08-keys.md`](08-keys.md)
