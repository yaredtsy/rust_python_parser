# 00.01 — The solver: one request

The top of the picture. Everything the analysis builds for one request hangs
off one `Solver`, and is dropped with it when the request ends.

**Reading:** [`plan/03-call-tree/13-solver-and-limits.md`](../../../plan/03-call-tree/13-solver-and-limits.md)
§ "The solver".

---

## The structure

```rust
pub struct Solver<'db> {
    sem:      Sem<'db>,                              // the only door into ty
    policy:   KeyPolicy,                             // the precision rules for keys
    limits:   Limits,                                // every limit, precision and cost
    contexts: Interner<Context<'db>, ContextId>,     // every key seen this request
    fps:      Interner<Fp<'db>, FpId>,               // every fingerprint seen this request
    slots:    IndexVec<ContextId, Slot<'db>>,        // one slot per context
    stack:    Vec<ContextId>,                        // walks in progress, innermost last
    heads:    UnionFind<ContextId>,                  // recursion group heads
    fixpoint: Option<Fixpoint>,                      // the group being resolved, if any
    per_func: FxHashMap<Definition<'db>, u32>,       // contexts per function, for a cost limit
    stats:    Stats,                                 // one counter per limit and stage
}
```

| field | what it holds | covered in |
|---|---|---|
| `contexts` | every distinct call shape: a function plus fingerprints of its arguments. `ContextId` is its index, in discovery order | [`02`](02-the-context.md) |
| `fps` | every distinct fingerprint. A context refers to fingerprints by `FpId` | [`02`](02-the-context.md) |
| `slots` | per context: its state, its summary when done, its walk while running | [`03`](03-the-slot.md) |
| `stack` | the contexts whose walks are in progress right now. A walk that calls a new context pushes it; when that walk ends, it pops | [`03`](03-the-slot.md) |
| `heads`, `fixpoint` | recursion groups and the one being resolved | [`02-function-summaries/05`](../02-function-summaries/05-recursion-pending-values.md) and [`06`](../02-function-summaries/06-recursion-groups.md) |
| `sem` | ty: parsed modules, semantic index, types, `vnoc::member` | — |
| `policy`, `limits` | `fp_depth`, `max_path`, `split_cap`, `max_rounds`, fan-out caps; `max_stack`, `max_contexts`, `contexts_per_func`, deadline; render limits | [`plan/03-call-tree/13`](../../../plan/03-call-tree/13-solver-and-limits.md#limits) |
| `stats` | the counters returned with the answer | same |

**Interning.** `contexts` and `fps` are interners: give one a value, get back
a small id; give it an equal value again, get back the same id. That is how
"have I seen this call shape before?" becomes a table lookup. In memory, keys
are compared as full values, never by hash.

**Ids are request-local.** `ContextId(3)` means "the fourth context discovered
in this request". It means nothing in the next request. Anything that outlives a
request uses stable IDs instead ([`10`](10-the-output.md)).

**The solver and the walk are separate structs.** Functions take
`(s: &mut Solver, w: &mut Walk)` side by side. A walk asking the solver for a
callee's summary can make the solver start another walk without the borrow
checker getting in the way.

---

## The running example, at this level

After the request for `main` finishes, the solver holds:

**`fps`** (fingerprints, [`02`](02-the-context.md)):

| id | fingerprint |
|---|---|
| fp#0 | `Instance { Pipeline, attrs: [] }` |
| fp#1 | `Instance { JsonWriter, attrs: [] }` |
| fp#2 | `Instance { Pipeline, attrs: [writer: fp#1] }` |
| fp#3 | `Data(int)` |
| fp#4 | `Unknown` |
| fp#5 | `Instance { CsvWriter, attrs: [] }` |
| fp#6 | `Instance { Pipeline, attrs: [writer: fp#5] }` |

**`contexts`** (keys):

| id | function | args |
|---|---|---|
| ctx#0 | `main` | — |
| ctx#1 | `read_json` | — |
| ctx#2 | `Pipeline.__init__` | self: fp#0, writer: fp#1 |
| ctx#3 | `Pipeline.run` | self: fp#2, data: fp#3 |
| ctx#4 | `JsonWriter.write` | self: fp#4, data: fp#4 |
| ctx#5 | `read_csv` | — |
| ctx#6 | `Pipeline.__init__` | self: fp#0, writer: fp#5 |
| ctx#7 | `Pipeline.run` | self: fp#6, data: fp#3 |
| ctx#8 | `CsvWriter.write` | self: fp#4, data: fp#4 |

**`slots`**: nine, all `Ready`, each with its summary ([`04`](04-the-summary.md)).

**`stack`**: empty again. At its deepest it was
`[main, read_json, Pipeline.run, JsonWriter.write]`.

**`stats`**: `contexts 9, walks 9, reuse_hits 1` — `w.write(2)` in
`read_json` had the same key as the call inside `run` (ctx#4), and reused its
summary. Nothing was cut.

---

## Exercise

1. Why is `JsonWriter.write`'s key `{self: Unknown, data: Unknown}` and not
   `{self: Instance{JsonWriter}, …}`?
2. Why is there no context for `JsonWriter.__init__`?
3. If `read_json` also did `Pipeline(JsonWriter()).run(9)`, which tables would
   grow?

<details><summary>Answers</summary>

1. `write` uses neither parameter: neither reaches a call, a receiver, a
   return, an attribute write or a nested `def`. Parameters that don't matter
   are `Unknown` in the key, so every caller shares one summary
   ([`02`](02-the-context.md)).
2. `JsonWriter` has no `__init__` body anywhere in its MRO. Constructing it
   records an edge to the class and walks nothing.
3. None. `Pipeline(JsonWriter())` gives ctx#2 again and `.run(9)` gives ctx#3
   again (`9` is `Data(int)` like `1`). Two more reuse hits, no new contexts or
   fingerprints. The walk of `read_json` records two more call sites, though.

</details>

---

→ Next: [`02-the-context.md`](02-the-context.md)
