# 01 — Summaries: how walks are tied together

**Goal:** you can say, for any call, which context it creates, what the callee's
summary contains, how the caller uses it, when it is reused, and how the tree
v-noc shows comes out of it all.

The previous folder was one walk. This one is everything between walks. It is
the idea that makes the analysis scale: a context-sensitive call tree is
exponential in depth, but the number of distinct contexts is not.

---

## How this folder is organised

| | file | what it gives you |
|---|---|---|
| 1 | [`01-why-summaries.md`](01-why-summaries.md) | the exponential tree, one walk per context, and the three rules |
| 2 | [`02-the-key.md`](02-the-key.md) | what a call is: the context, fingerprints, and how a walk starts from one |
| 3 | [`03-the-result.md`](03-the-result.md) | what a call did: returns, effects, new objects, all relative to the inputs |
| 4 | [`04-one-call.md`](04-one-call.md) | ★ resolve against the snapshot, then write. Split by argument type |
| 5 | [`05-the-solver.md`](05-the-solver.md) | reuse, cuts, and the no-reuse check |
| 6 | [`06-graph-and-tree.md`](06-graph-and-tree.md) | the graph pylspt answers with, and the tree rendered from it |

Then [`exam.md`](exam.md). Fixtures are in `python/`.

---

## Read first

- [`plan/03-call-tree/03-design.md`](../../../plan/03-call-tree/03-design.md)
- [`plan/03-call-tree/08-keys.md`](../../../plan/03-call-tree/08-keys.md),
  [`09-summaries.md`](../../../plan/03-call-tree/09-summaries.md),
  [`10-one-call.md`](../../../plan/03-call-tree/10-one-call.md),
  [`13-solver-and-limits.md`](../../../plan/03-call-tree/13-solver-and-limits.md),
  [`14-output.md`](../../../plan/03-call-tree/14-output.md)

Recursion is its own lesson, later:
[`02-functions/08`](../02-functions/08-recursion.md).

---

## Done when

- [ ] you can state the three rules and give the bug each one prevents
- [ ] you can fingerprint a caller's object by hand to `fp_depth = 2`
- [ ] you can write a summary with `Ref`, `Obj` and an `Effect` for a
      three-line function
- [ ] you can trace `Box.replace` and say what goes wrong if the effect is
      applied before the returns are resolved
- [ ] you can say why a cut result is never reused, and what no-reuse mode
      does and does not prove
- [ ] you can render a tree from a graph by hand, including a `Recursive` leaf,
      and say what v-noc's merge does to it

---

→ Start: [`01-why-summaries.md`](01-why-summaries.md)
