# 00.03 — The slot: what the solver keeps per context

Every context has one slot. The slot says where that context is in its life,
and holds its summary once it has one.

**Reading:** [`plan/03-call-tree/13-solver-and-limits.md`](../../../plan/03-call-tree/13-solver-and-limits.md)
§ "The solver" and § "`get`".

---

## The structures

```rust
struct Slot<'db> {
    state:   State,
    result:  Option<Arc<FunctionResult<'db>>>,   // the summary: set when Ready (or Cut)
    current: Option<Arc<FunctionResult<'db>>>,   // latest summary while its recursion group is resolving
    walk:    Option<Box<Walk<'db>>>,             // the walk, kept only while waiting on a recursion group
    lowest:  usize,                              // smallest stack depth of a running context its walk read
    joins:   Option<ContextId>,                  // a resolving group its walk reached
    cut:     Option<Cut>,                        // its walk read a truncated result
}

enum State {
    New,                          // interned, never walked
    Running { depth: usize },     // its one walk is on the stack, at this depth
    Parked { head: ContextId },   // walked; waits for its recursion group
    Resolving,                    // its group's fixpoint is running
    Ready,                        // done. Its summary is reused by every later call
    Cut,                          // done, but truncated by a cost limit. Walked again next time
}

pub enum Got<'db> {
    Ready(Arc<FunctionResult<'db>>),   // here is the summary
    Pending,                           // recursion: record the call, use a placeholder value
}
```

`result`, `state` and `cut` are what most contexts ever use. `current`, `walk`,
`lowest` and `joins` only matter for recursion
([`02-functions/08`](../02-functions/08-recursion.md)).

## The life of a slot

```
             get(ctx)
   New ─────────────────► Running { depth }
                              │
            walk ends, ───────┼──────── walk ends, it read a running context further down
            no cycle          │                      │
                              ▼                      ▼
                  ┌────── Ready              Parked { head }
                  │         ▲                        │ the head's walk ends
     a cost limit │         │ fixpoint converges     ▼
     was read     │         └───────────────── Resolving
                  ▼
                 Cut ──── reached again ──► Running (walked again)
```

What `get(ctx)` does depends on the state:

| state | `get` returns | and |
|---|---|---|
| `New` | `Ready(summary)` | pushes ctx on the stack, walks it, pops it, stores the summary |
| `Running` | `Pending` | the walk asking is calling into a walk still in progress: recursion |
| `Parked`, `Resolving` | `Pending`, or the latest summary inside the fixpoint | recursion |
| `Ready` | `Ready(summary)` | nothing else: **reuse** |
| `Cut` | `Ready(summary)` | walks it again, since this caller may have more room |

`depth` in `Running` is the stack length when the walk started. It is how the
solver tells "calling myself" from "calling something new".

A cost limit (stack depth, too many contexts, the deadline) gives the asking
walk a truncated result `[Unknown]` that is never stored, and marks the asker
`cut`. A slot that read a cut result ends `Cut`, not `Ready`, so its truncated
summary is never handed to a later caller.

---

## The running example, at this level

Step by step, the stack and the slots that change:

| step | event | stack after | slot changes |
|---|---|---|---|
| 1 | request: `get(ctx#0)` (main) | `[main]` | ctx#0 `Running{0}` |
| 2 | `read_json()` → `get(ctx#1)` | `[main, read_json]` | ctx#1 `Running{1}` |
| 3 | `JsonWriter()` | same | none: no `__init__`, no context |
| 4 | `Pipeline(...)` → `get(ctx#2)`, walked, returns | `[main, read_json]` | ctx#2 `Running{2}` → `Ready` |
| 5 | `p.run(1)` → `get(ctx#3)` | `[main, read_json, run]` | ctx#3 `Running{2}` |
| 6 | inside run: `self.writer.write(data)` → `get(ctx#4)`, walked, returns | `[main, read_json, run]` | ctx#4 `Running{3}` → `Ready` |
| 7 | run's walk ends | `[main, read_json]` | ctx#3 → `Ready` |
| 8 | `w.write(2)` → `get(ctx#4)` | same | none. **Reuse**: `Ready` |
| 9 | read_json's walk ends | `[main]` | ctx#1 → `Ready` |
| 10 | `read_csv()` → ctx#5, then ctx#6, ctx#7, ctx#8, as steps 2–7 | … | all → `Ready` |
| 11 | main's walk ends | `[]` | ctx#0 → `Ready` |

No `Parked`, `Resolving` or `Cut` here: the example has no recursion and hits no
limit.

---

## Exercise

1. At step 6, what is `ctx#4`'s `depth`, and why?
2. Suppose `JsonWriter.write` called `self.write(data)` again. At which step
   would `get` return `Pending`, and what would ctx#4's slot do when its walk
   ends?
3. Run the example with `max_stack = 3`. Which `get` is cut, and which slots end
   `Cut`?

<details><summary>Answers</summary>

1. 3: the stack was `[main, read_json, run]` (length 3) when its walk started.
2. Inside ctx#4's own walk: its recursive call has the same key, and ctx#4 is
   `Running`. When the walk ends, it read only itself, so it is the head of its
   own group, and resolves it right away
   ([`02-functions/08`](../02-functions/08-recursion.md)).
3. `get(ctx#4)` from inside `run`: the stack length is 3. `run` read a cut
   result, so ctx#3 ends `Cut`, and so do `read_json` and `main`, which read it
   in turn. In `read_csv`, `run` asks for ctx#8 at the same depth, so the same
   happens there. Every truncated node is marked in the output.

</details>

---

→ Next: [`04-the-summary.md`](04-the-summary.md)
