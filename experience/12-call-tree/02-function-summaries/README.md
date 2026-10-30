# 02 — Function summaries

**Goal:** the same trees as part 01, with each function body walked once per
distinct input instead of once per path. At the end, `wall.py` walks 7
bodies instead of 15, and a 10 × 6 wall walks 61 instead of 1,111,111.

Part 01 ended with a question: what does a walk depend on? Its arguments, its
free variables, and the code. This part turns that answer into a **key**,
records what one walk found as a **summary**, and gives each key one walk.
Every step keeps the trees of part 01 unchanged, and says exactly where they
are allowed to differ.

Objects are not here. Every fixture in this part uses functions, closures and
data only. Part 04 does the same work for objects, once part 03 has built
them.

---

## The steps

| | step | you build | the decision |
|---|---|---|---|
| 1 | [`01-the-key.md`](01-the-key.md) | `Fp`, `Context`, interning | a call is its function plus a fingerprint of each argument. Literals widen to their class |
| 2 | [`02-the-summary.md`](02-the-summary.md) | `FunctionResult`, `SVal`, `materialize`, `finish`, `check_both` | a walk starts from its key alone, and its result holds nothing walk-local. Every fixture runs in both modes |
| 3 | [`03-the-solver-and-the-graph.md`](03-the-solver-and-the-graph.md) | `Solver`, slots, reuse, `render` | one walk per key. The tree is rendered from the graph. No-reuse joins the harness |
| 4 | [`04-relevant-parameters.md`](04-relevant-parameters.md) | a taint pass | a parameter the body can't be affected by is `Unknown` in the key |
| 5 | [`05-recursion-pending-values.md`](05-recursion-pending-values.md) | `Pending`, deferred operations, a fixpoint for one function | a recursive call is recorded, not entered. Its value is found afterwards |
| 6 | [`06-recursion-groups.md`](06-recursion-groups.md) | groups, park, join, merge | mutual recursion is one group, solved together, the same from any entry |
| 7 | [`07-closures-across-a-summary.md`](07-closures-across-a-summary.md) | `Fp::Closure`, `SVal::Closure`, `Back` | a closure crosses a call carrying its free values, never its activation |
| 8 | [`08-cost-limits-and-cuts.md`](08-cost-limits-and-cuts.md) | `max_stack`, context limits, the deadline | a cost limit only truncates, is marked, and is never reused |

**Steps 2, 3 and 5 are the core.** 2 holds the two rules that make reuse
safe. 3 is reuse itself. 5 is the hardest idea in the design.

---

## The three rules

Every decision in this part serves one of them. They are the plan's, and
`plan/03-call-tree/03-design.md` § "The rules" has the long form.

1. **A summary is a function of its key.** A walk reads nothing from its
   caller except the key. Step 2.
2. **Nothing walk-local leaves a summary.** No activation id, no pending id,
   later no object id. The types make it impossible. Step 2.
3. **Precision depends only on the key and the code. Cost limits only
   truncate.** A limit that depends on how a context was reached never
   changes a reusable answer. Step 8.

The check that keeps them honest is **no-reuse mode** (step 3): recompute
every summary instead of reusing it, and the output must not change.

---

## What changes from part 01

```
src/interp/
  key.rs      Fp, Context, fingerprint, key, materialize       ← new
  summary.rs  FunctionResult, CallEdge, SiteId, SVal, finish   ← new
  solver.rs   Solver, Slot, State, get                         ← new
  defer.rs    Pending, deferred operations, the fixpoint       ← new (steps 5–6)
  call.rs     call_project (summary mode) next to call_function (inline)
src/render.rs the tree, from the graph                         ← new
```

Everything else in `src/interp/` stays. The walker gains one new parameter in
step 3, `s: &mut Solver`, for a reason step 5 makes unavoidable.

---

## Read first

- `plan/03-call-tree/03-design.md` § "Why summaries", § "The rules"
- `plan/03-call-tree/08-keys.md`, `09-summaries.md`, `12-recursion.md`,
  `13-solver-and-limits.md`

---

## Done when

- [ ] every part 01 fixture passes in both modes, with only the differences
      the steps name
- [ ] `wall.py` reports 7 walks in summary mode
- [ ] no-reuse mode gives the same output on every fixture
- [ ] you can state the three rules, and for each, a fixture that breaks
      without it

---

→ Start: [`01-the-key.md`](01-the-key.md)
