# 00.05 — The walk: one body under one context

Below the summary is the thing that makes it. A `Walk` exists while one function
body is evaluated under one context. It starts from the key, ends as a summary,
and is dropped.

**Reading:** [`plan/03-call-tree/05-the-walk.md`](../../../plan/03-call-tree/05-the-walk.md)
§ "The walk state", [`08-keys.md`](../../../plan/03-call-tree/08-keys.md) § "Materializing",
[`09-summaries.md`](../../../plan/03-call-tree/09-summaries.md) § "Finishing a walk".

---

## The structure

```rust
struct Walk<'db> {
    ctx:         ContextId,                          // which context this walk is
    env:         Env<'db>,                           // what it knows right now           → 06
    origins:     Interner<OriginKey, OriginId>,      // which object is which, in here    → 07
    values:      ValueArena<'db>,                    // interned receivers                → 07
    activations: IndexVec<ActivationId, Env<'db>>,   // runs, for closures                → 06
    calls:       Vec<CallEdge<'db>>,                 // every call site reached, in order
    exits:       Vec<Env<'db>>,                      // the env at every `return` and at the end
    returns:     Values<'db>,                        // every value returned
    deferred:    Vec<Deferred<'db>>,                 // operations waiting on recursion; usually empty
    gaps:        Gaps,                               // precision gaps found here (e.g. DEEP_WRITE)
}
```

Everything in a walk is **walk-local**: origins, value ids, activations and
deferred-operation ids are numbers that mean something only in this walk. That
is why a walk must be converted before anything leaves it.

## When a walk exists

```
get(ctx) on a New slot
  └─► materialize(ctx)   key → starting env                 (start)
      walk_stmts(body)   statements, calls, returns         (during)
      finish()           walk → FunctionResult              (end)
  └─► the walk is dropped. The slot keeps only the summary.
```

The one exception: a walk that took part in recursion is kept in its slot
(`Parked`, then `Resolving`) until its group is resolved, because its deferred
operations are replayed ([`02-functions/08`](../02-functions/08-recursion.md)).

## Starting: `materialize`

The walk's starting env is built from the key, and from nothing else. That is
rule 1: the walk can't see anything its key doesn't hold.

| fingerprint in the key | the walk starts with |
|---|---|
| `Instance { class, attrs }` | an object with origin `Entry(path)`; each attribute written into the env at `path + [Attr(name)]` |
| `Opaque(class)` | an object with origin `Entry(path)` and no attributes |
| `Bound`, `Closure` | the method or closure, its receiver or captured values at `path + [Recv]` / `[Free(n)]` |
| `Function`, `Class`, `Module`, `Data` | that value |
| `Unknown` | **the parameter's annotation**, lifted, if it has one; else `Unknown` |

Parameter `i` sits at path `Param(i)`. Aliased parameters share one origin.

### The annotation fallback

The last row: when the key holds nothing for a parameter, its annotation is still
known.

```python
def process(h: Handler):
    h.handle(1)          # h is Unknown in the key → starts as a Handler → Handler.handle
```

It only fires for `Unknown`, so a value a caller actually passed always wins. It
depends only on the code, so the walk is still a function of its key. Every use
is counted (`Stats.annotation_fallbacks`), because an annotation is a claim, not
a proof. Entry functions rely on it: their keys are all `Unknown`.
`python/annotated.py` has a case for each situation.

## During

Statements are evaluated in order. Each call expression asks the solver for a
summary and records a `CallEdge`. Each `return` adds to `returns` and pushes the
env onto `exits`; falling off the end does too, returning `None`. Branches fork
the env and join it ([`06`](06-the-env.md)).

## Ending: `finish`

`finish()` turns the walk into a `FunctionResult`:

1. **Join the exits**: one exit env.
2. **Returns**: each value converted to an `SVal`: an input object → `Ref(path)`,
   an object built here → `Obj(j)`, a closure → `Closure { func, free }`,
   anything else → `Plain(..)` ([`09`](09-the-four-conversions.md)).
3. **Effects**: read off the exit env's `written` set. A write to an input object
   becomes an `Effect` at its path; `strong` if written on every path.
4. **Objects**: every object built here and reachable from the returns, the
   effects or other escaping objects becomes a `NewObject`. The rest are dropped.
5. **Sort** effects and objects, so two summaries can be compared with `==`.

---

## The running example, at this level

**ctx#3, `Pipeline.run {self: fp#2, data: fp#3}`.**

Start (`materialize`):

```
origins:  e0 = Entry(Param(0))                 self
          e1 = Entry(Param(0).writer)          self.writer, from fp#2's attrs
env:      locals  self → [Instance{Pipeline, e0}]
                  data → [Ty(int)]
          attrs   (e0, writer) → [Instance{JsonWriter, e1}]
          written (empty)
```

During:

```
self.writer.write(data)   callee: self.writer → [Instance{JsonWriter, e1}]
                          .write → BoundMethod{JsonWriter.write, receiver: v0}   v0 = Instance{JsonWriter, e1}
                          key ctx#4 → get → walked (New) → Ready
                          calls += CallEdge { site, [Call(ctx#4)] }
return self.writer        returns += [Instance{JsonWriter, e1}];  exits += env
```

End (`finish`):

```
returns: Instance{JsonWriter, e1} → e1 is Entry(Param(0).writer) → Ref(Param(0).writer)
effects: written is empty → []
objects: nothing was built → []
```

That is ctx#3's summary from [`04`](04-the-summary.md). The walk, with `e0`, `e1`
and `v0`, is dropped.

---

## Exercise

1. Write `materialize` for ctx#2 (`Pipeline.__init__ {self: fp#0, writer: fp#1}`).
2. In ctx#2's walk, `self.writer = writer` writes `(e0, writer)`. Which `finish`
   step turns it into the effect, and why is it `strong`?
3. `python/annotated.py`: as an entry, what does `needs_the_annotation`'s `h`
   start as? And `run_unresolved`'s `opaque`?

<details><summary>Answers</summary>

1. `e0 = Entry(Param(0))`, `e1 = Entry(Param(1))`. `self → [Instance{Pipeline, e0}]`,
   `writer → [Instance{JsonWriter, e1}]`, no attributes (both fingerprints have
   empty `attrs`).
2. Step 3: `(e0, writer)` is in `written`, and `e0` is `Entry(Param(0))`, so it
   becomes `Effect { target: Param(0), attr: writer, value: [Ref(Param(1))] }`.
   Strong, because `__init__` writes it on its only path, so `written` says
   `Must`.
3. `h` starts as a `Handler` (an `Entry(Param(0))` object), from its annotation.
   `opaque` has no annotation, so it stays `Unknown`.

</details>

---

→ Next: [`06-the-env.md`](06-the-env.md)
