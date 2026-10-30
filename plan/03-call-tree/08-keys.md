# 03.08 — Keys: what a call is

A call to a project function is answered by the callee's summary for one
**context**. The context is the key. This chapter says what goes into a key, how
caller values become one, and how a walk starts from one.

---

## The key

```rust
pub struct ContextId(u32);     // interned per request, in discovery order
pub struct FpId(u32);          // interned per request

pub struct Context<'db> {
    pub func:  Definition<'db>,
    pub args:  SmallVec<[FpId; 4]>,          // one per parameter after binding, receiver first, defaults filled
    pub alias: SmallVec<[u8; 4]>,            // alias[i] = first parameter holding the same object as i
    pub free:  SmallVec<[(Name, FpId); 2]>,  // free variables of func, sorted by name. Empty unless func is a closure
}

pub enum Fp<'db> {
    Unknown,
    Data(Type<'db>),                         // only types StableType can encode (14)
    Function(Definition<'db>),
    Class(Definition<'db>),
    Module(Type<'db>),                       // ty's module literal type, as in 04
    Bound    { func: Definition<'db>, recv: FpId },
    Closure  { func: Definition<'db>, free: SmallVec<[(Name, FpId); 2]>,
               defaults: SmallVec<[(Name, FpId); 2]> },   // values computed when its `def` ran (06)
    Back(u8),                                // the closure k levels up in this fingerprint (below)
    Instance { class: Definition<'db>, attrs: SmallVec<[(Name, FpId); 4]> },  // sorted by name
    Opaque(Definition<'db>),                 // class known, attribute state not in the key
    Union(SmallVec<[FpId; 4]>),              // sorted, deduped
}
```

In memory, keys are full values, compared exactly. A hash collision inside a key
would be a silently wrong tree.

**Allocation sites are not in the key.** Two `Pipeline(JsonHandler())` objects
with the same attributes share one `run` summary. That is type sensitivity, and
it is where the sharing comes from. Identity across the call is kept by `alias`
and by `Ref` paths in the summary ([`09`](09-summaries.md)), not by the key.

---

## Fingerprinting

`key()` turns the caller's values into a `Context`. It reads the caller's state
after the function and the arguments are evaluated and before the call runs:
the snapshot `before` ([`10`](10-one-call.md)).

```rust
fn key(&mut self, w: &Walk<'db>, func: Definition<'db>, combo: &Combo<'db>,
       callee: AbstractValue<'db>, before: &Env<'db>) -> ContextId {
    let relevant = relevant_params(self.db, func);
    let mut closures = ClosureStack::starting_with(callee);       // the callee itself, if a closure
    let args = combo.slots.iter().enumerate()
        .map(|(i, v)| if relevant.contains(i) { self.fingerprint(w, v, before, 0, &mut closures) }
                      else { self.fp(Fp::Unknown) })
        .collect();
    let free = w.free_values(callee).iter()                       // the closure's OWN activation
        .map(|(n, v)| (n.clone(), self.fingerprint(w, v, before, 0, &mut closures)))
        .collect();
    self.contexts.intern(Context { func, args, alias: alias_vector(combo), free })
}
```

Every rule below is a `KeyPolicy` rule. The walker never decides precision.

- **Instances.** Class plus attributes from `before.attrs`, sorted by name, to
  `fp_depth` steps. An instance at the depth limit becomes `Opaque(class)`. An
  `OPAQUE` or `Beyond` instance is always `Opaque(class)`.
- **Data.** Literals are widened to their class (`Literal[3]` becomes `int`).
  **[check]** ty's accessor for the widened type. A type that `StableType`
  cannot encode becomes `Fp::Unknown`, and `Stats.types_dropped` counts it.
- **Unions.** A parameter holding several values is split by CPA first
  ([`10`](10-one-call.md#cpa-one-context-per-combination)). `Fp::Union` only
  appears when splitting is over `split_cap`, and inside attributes.
- **Empty values.** A slot with no value is keyed as `Unknown`. The call still
  gets its edge ([`10`](10-one-call.md#a-call-with-no-value)).
- **Closures.** Free values are read from the closure's own activation, the one
  in its `Closure { activation }` value. Never from the caller's env. A closure
  called from `main` must see the `h` that `make` captured, not `main`'s own
  `h` ([`11`](11-closures-and-callbacks.md)).
- **Closures that capture themselves.** A nested `def walk(n): walk(n.child)`
  has a free variable `walk` whose value is the closure itself, so a naive
  fingerprint never ends. While fingerprinting a closure, keep a stack of the
  closures in progress, compared by `(func, activation)`. A free value equal to
  one on the stack becomes `Back(k)`, where `k` is its distance up the stack.
  The same rule covers two nested defs that call each other. With it, the
  recursive call keys to the same context as the outer one, and recursion is
  handled as recursion ([`12`](12-recursion.md)).
- **Bound methods.** The receiver is fingerprinted like any argument.
- **Depth.** Every step counts against `fp_depth`: an attribute, a closure's
  free value, a bound method's receiver. A closure at the limit keeps its
  function and fingerprints every free value as `Unknown`.
- **Alias.** `alias[i] = j` when parameters `i` and `j` hold the same origin.
  Top-level parameters only ([`15`](15-checks-and-build-order.md#known-limits)).
- **Irrelevant parameters.** `relevant_params(db, func)` is a taint pass over
  the body. A parameter that reaches no callee, no receiver, no project-call
  argument, no `return`, no attribute write and no nested `def` is replaced by
  `Fp::Unknown` in the key. Seed every parameter (and every free variable),
  propagate through assignments (`x = w; x.write()` taints `x`), and be
  conservative: over-marking costs sharing, under-marking gives wrong trees. It
  depends only on the code, so it obeys rule 3. It is a `#[salsa::tracked]`
  function in pylspt; `src/defs.rs::definitions` already shows a tracked
  function in pylspt over ty's `Db` works, and `Definition<'db>` is a salsa
  tracked struct, which tracked functions accept.
- **Pending values.** A value still waiting on a recursive call has no
  fingerprint. A call whose key would read one is not keyed during the walk.
  "Read" means anything the fingerprint reaches, to `fp_depth`: an argument, an
  attribute of an argument, a closure's free value, a bound receiver. Such a
  call becomes a deferred `Call`, keyed in the fixpoint ([`12`](12-recursion.md)).

```rust
fn fp_one(&mut self, w: &Walk<'db>, v: AbstractValue<'db>, env: &Env<'db>,
          depth: u8, closures: &mut ClosureStack<'db>) -> FpId {
    use AbstractValue::*;
    let fp = match v {
        Instance { class, origin } if !w.holds_state(origin) || depth == self.policy.fp_depth
                                     => Fp::Opaque(class),
        Instance { class, origin }   => Fp::Instance { class, attrs: self.fp_attrs(w, origin, env, depth + 1, closures) },
        Closure { func, activation } => match closures.position(func, activation) {
            Some(k)                         => Fp::Back(k),
            None if depth == self.policy.fp_depth => Fp::Closure { func, free: self.unknown_free(func) },
            None => closures.with(func, activation, |cl|
                        Fp::Closure { func, free: self.fp_free(w, activation, env, depth + 1, cl) }),
        },
        BoundMethod { func, receiver } => Fp::Bound { func, recv: self.fp_one(w, w.values[receiver], env, depth + 1, closures) },
        Function(d) => Fp::Function(d),
        Class(d)    => Fp::Class(d),
        Module(m)   => Fp::Module(m),
        Ty(t)       => self.policy.data(t).map_or(Fp::Unknown, Fp::Data),   // widen, then StableType check
        Unknown     => Fp::Unknown,
        Pending(_)  => unreachable!("deferred before keying"),
    };
    self.fps.intern(fp)
}
```

---

## Materializing

`Walk::materialize(ctx)` builds the starting `Env` from the key, and from nothing
else (rule 1).

| fingerprint | becomes |
|---|---|
| `Instance { class, attrs }` | `Instance { class, origin: Entry(path) }`, attrs written into `Env.attrs`, each at `path + [Attr(name)]` |
| `Opaque(class)` | `Instance { class, origin: Entry(path) }` with no attrs. Reads fall through to the class side, and unknown attributes are named by path ([`07`](07-attributes-and-self.md#objects-the-key-does-not-reach)) |
| `Bound { func, recv }` | `BoundMethod { func, receiver }`, receiver at `path + [Recv]` |
| `Closure { func, free }` | `Closure { func, activation }` over a new activation holding the free values, each at `path + [Free(n)]` |
| `Back(k)` | the closure value built `k` levels up |
| `Function`, `Class`, `Module`, `Data` | the matching `AbstractValue` |
| `Unknown` | the parameter's annotation, lifted, if it has one (below). Else `Unknown` |
| `Union(xs)` | every member, at the same path |

Parameter `i` starts at `Path { root: Param(i), steps: [] }`. Aliased parameters
share the first one's origin. The function's own free variables go into a new
activation that becomes the walk's `enclosing`, with roots `Free(i)`, where `i`
indexes the sorted `free` list. In that list, `Back(0)` is the walked function
itself.

An `Opaque` fingerprint gets a real `Entry` origin, not the shared `OPAQUE`. It
stands for one specific caller object, so writes to it inside the walk are
tracked and reported as effects. Only instances that ty hands the walk use
`OPAQUE`.

### The annotation fallback

A parameter whose fingerprint is `Unknown` starts from its annotation, lifted,
with each `OPAQUE` instance given the parameter's `Entry` origin. An unannotated
`self` or `cls` uses the enclosing class. `Stats.annotation_fallbacks` counts
each use.

```python
def process(h: Handler):
    h.handle(1)          # h is Unknown in the key → Handler from the annotation

def run(make: Callable[[], JsonHandler]):
    make().process()     # Ty(Callable) from the annotation; calling it uses ty's return type
```

It depends only on the code, so it obeys rule 1. It never overrides a value the
key carries. An annotation is a claim, not a proof, which is why it is counted:
a leaf that came from an annotation should be explainable as such.

### Entry contexts

An entry is one requested function. Its context has every parameter `Unknown`,
so materializing gives each parameter its annotation. An entry context is the
same context as a call that passes nothing known, and shares its summary.

---

## Fixtures

```python
# 1. type sensitivity: ONE run summary for two objects with equal fingerprints
def main():
    Pipeline(JsonHandler()).run(1)
    Pipeline(JsonHandler()).run(2)        # Stats: one run context, one reuse

# 2. different fingerprints: TWO run summaries
def main():
    Pipeline(JsonHandler()).run(1)        # JsonHandler.process
    Pipeline(CsvHandler()).run(2)         # CsvHandler.process

# 3. an irrelevant parameter does not split contexts
def log_event(name, payload):
    logger.info(name)
def a(): log_event("x", JsonHandler())
def b(): log_event("y", CsvHandler())     # same log_event context as a's

# 4. a self-capturing closure keys to itself
def outer(root):
    def walk(n):
        walk(n.child)
    walk(root)
# outer > walk > walk [Recursive]. walk has ONE context.

# 5. annotation fallback
def process(h: Handler): h.handle(1)      # entry: Handler.handle
```

---

→ Next: [`09-summaries.md`](09-summaries.md)
