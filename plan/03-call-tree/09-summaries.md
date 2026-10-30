# 03.09 — Summaries: what a call did

A walk ends in a `FunctionResult`: the call edges it reached, what it returns,
what it wrote on objects the caller can see, and the objects it built that
escape. Every value in it is written relative to the inputs, so one summary
serves every caller with the same key.

---

## The result

```rust
pub struct FunctionResult<'db> {
    pub calls:   Vec<CallEdge<'db>>,          // one per call site reached, evaluation order
    pub returns: SVals<'db>,                  // union of every `return`
    pub effects: Vec<Effect<'db>>,            // writes on objects the caller can see, sorted
    pub objects: Vec<NewObject<'db>>,         // objects built in this call that escape it
    pub status:  Status,
}

pub struct Status {
    pub gaps: Gaps,                           // precision gaps. Reusable: they depend only on the key
    pub cut:  Option<Cut>,                    // a cost limit was hit. Never reused
}
// Finished = no gaps and no cut.

bitflags! {
    pub struct Gaps: u8 {
        const CYCLE      = 1;   // a recursion group hit max_rounds (12)
        const DEEP_WRITE = 2;   // a write past max_path became a weak Unknown effect (07)
    }
}

pub enum Cut { Stack, Contexts, ContextsPerFunc, Deadline }

pub struct CallEdge<'db> { pub site: SiteId, pub targets: SmallVec<[Target<'db>; 2]> }

/// The call expression's range, relative to the start of the enclosing `def`.
/// Stable under edits above the function. The start alone is not unique:
/// `f()()` and `a.b().c()` begin two calls at the same offset.
pub struct SiteId { pub start: TextSize, pub end: TextSize }

pub enum Target<'db> {
    Call(ContextId),                                                // also a recursive call
    Construct { class: Definition<'db>, init: Option<ContextId> },  // None: synthesised __init__
    External(Definition<'db>),                                      // stdlib or site-packages
    Unresolved,
}
```

A recursive call is an ordinary `Target::Call(ctx)`. Its context is known when
the call is reached, even though its value is not, so the edge is recorded at
once. Recursion shows in the graph as a cycle, and in the tree as a marked leaf.

A property access is a call site too: its `SiteId` is the attribute
expression's range ([`07`](07-attributes-and-self.md#properties)).

---

## Values relative to the inputs

```rust
/// A value in a summary. No variant can hold a walk-local id.
pub enum SVal<'db> {
    Plain(PlainVal<'db>),
    Ref(Path),                                // the caller's object at this path when the call started
    Obj(u16),                                 // objects[j]
    Bound   { func: Definition<'db>, recv: Box<SVal<'db>> },
    Closure { func: Definition<'db>, free: SmallVec<[(Name, SVals<'db>); 2]>,
              defaults: SmallVec<[(Name, SVals<'db>); 2]> },   // values computed when its `def` ran (06)
    Back(u8),                                 // the closure k levels up: one that captures itself (11)
}
pub type SVals<'db> = SmallVec<[SVal<'db>; 2]>;

/// Values with no identity. Rule 2 as a type: nothing here can carry an
/// OriginId, ActivationId, ValueId or DeferId, so a leak does not compile.
pub enum PlainVal<'db> {
    Function(Definition<'db>),
    Class(Definition<'db>),
    Module(Type<'db>),
    Ty(Type<'db>),
    Unknown,
    Opaque(Definition<'db>),                  // an instance with no tracked identity: class only
}

pub struct Path { pub root: Root, pub steps: SmallVec<[Step; 2]> }   // steps.len() <= max_path
pub enum Root { Param(u8), Free(u8) }
pub enum Step { Attr(Name), Free(u8), Recv }
```

`Ref(path)` means "the object at this path when the call started". Paths can be
longer than `fp_depth`: the key stops there, but the walk can still name deeper
input objects by their location ([`07`](07-attributes-and-self.md#objects-the-key-does-not-reach)).

`Step::Free` and `Step::Recv` reach into a closure or a bound method passed as an
argument. So a callback that writes to its own receiver still produces an effect
the caller can apply ([`11`](11-closures-and-callbacks.md)).

A closure in a summary carries its free values directly. A finished run's
assignments never change again, so activations only have to live inside their
walk. The one case this does not cover is a captured variable rebound through
`nonlocal` by another closure ([`15`](15-checks-and-build-order.md#known-limits)).

```rust
pub struct Effect<'db> {
    pub target: Path,                         // the object written to, as it was at call start
    pub attr:   Name,
    pub value:  SVals<'db>,
    pub strong: bool,                         // written on every path that exits normally
}

pub struct NewObject<'db> {
    pub class: Definition<'db>,
    pub site:  SiteId,                        // where it was built, inside this function
    pub attrs: Vec<(Name, SVals<'db>)>,       // final state, sorted
}
```

---

## Finishing a walk

`finish()` turns a walk into a `FunctionResult`.

1. **Exit env.** Join every env in `exits` with `Env::join`
   ([`05`](05-the-walk.md#statements)).
2. **Returns.** Pass the walk's returns through `cap_fanout`, then convert each
   value with `to_sval`:
   - an `Entry(path)` origin becomes `Ref(path)`
   - a `New` origin becomes `Obj(j)`, and the object is added to `objects`
   - an `OPAQUE` or `Beyond` instance becomes `Plain(Opaque(class))`
   - a closure becomes `Closure { func, free, defaults }`, free values read
     from its activation at this exit and converted by these same rules (a
     `Pending` inside is resolved too). A closure already being converted
     (it captures itself) becomes `Back(k)`, with the same closure stack as
     fingerprinting ([`11`](11-closures-and-callbacks.md#a-closure-that-captures-itself)).
     A closure inside `fp_depth` other closures becomes `Plain(Unknown)`, or a
     recursion fixpoint never ends ([`12`](12-recursion.md#why-it-stops))
   - a bound method becomes `Bound { func, recv }`
   - a `Pending(id)` becomes its resolved values ([`12`](12-recursion.md))
   - everything else becomes `Plain(..)`

   A generator returns `[Plain(Unknown)]`, whatever its `return`s say: calling
   it gives a generator, not the yielded values.
3. **Effects.** Read them off the exit env's `written` set. For every
   `(origin, name)` in it whose origin is `Entry(path)`: an `Effect` with
   target `path`, the exit env's value for that attribute, and `strong` when
   the entry is `Must`. For every `Beyond { at, attr }` written to: the
   fallback effect, target `at`, attribute `attr`, value `[Plain(Unknown)]`,
   not strong, and the `DEEP_WRITE` gap. The `written` set is already tracked,
   and unlike comparing exit attributes with the materialized ones, it does not
   miss a write whose value happens to match the old one.
4. **Objects.** Every `New` origin reachable from returns, effect values,
   closure free values or other escaping objects becomes a `NewObject`, with
   attributes from the exit env. Unreachable ones are dropped.
5. **Order.** Sort `effects` and `objects` canonically, so two summaries can be
   compared with `==`. The fixpoint needs that ([`12`](12-recursion.md)).

An effect with an empty value is not emitted: no value reached the write.

---

## Fixtures

```python
# 1. Ref keeps identity: the caller gets ITS object back
def ident(x): return x
def main():
    h = JsonHandler()
    ident(h).process()          # JsonHandler.process; ident's returns are [Ref(Param(0))]

# 2. Obj: a built object escapes
def make(): return JsonHandler()
def main(): make().process()    # make's returns are [Obj(0)], objects[0] is JsonHandler

# 3. an effect on an input
def install(p, h): p.handler = h
def main():
    p = Pipeline(None)
    install(p, JsonHandler())   # effect: Param(0).handler = Ref(Param(1)), strong
    p.run()                     # JsonHandler.process

# 4. a weak effect: written on one path only
def maybe(p, h):
    if c: p.handler = h
def main():
    p = Pipeline(CsvHandler())
    maybe(p, JsonHandler())
    p.run()                     # BOTH CsvHandler.process and JsonHandler.process
```

---

→ Next: [`10-one-call.md`](10-one-call.md)
