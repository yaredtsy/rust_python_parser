# 05 — Output

**Goal:** what leaves pylspt. The analysis of parts 01–04 is done; this part
turns it into something v-noc can read and store, and puts in place the checks
that run on every change from now on.

Until now the output was `to_text`, compared with expected trees in comments.
The real response carries two things: the **graph** of contexts, which is
pylspt's own answer and is linear in size, and the **tree** v-noc stores today,
rendered from it, which can be as big as the number of paths.

---

## The steps

| | step | you build | the decision |
|---|---|---|---|
| 1 | [`01-the-graph-and-stable-ids.md`](01-the-graph-and-stable-ids.md) | `GraphOut`, `StableDef`, `StableType`, `StableContextId` | an identity that outlives the request depends on neither discovery order nor salsa ids |
| 2 | [`02-the-wire-tree.md`](02-the-wire-tree.md) | `FrameJson`, the ID filter, the render budget, `call_count` | the tree is a view: filtered, budgeted, marked, unmerged |
| 3 | [`03-the-checks.md`](03-the-checks.md) | no-reuse, determinism, golden files, `Stats` | every check that keeps the answer honest, and what each one can't see |
| 4 | [`04-the-benchmark.md`](04-the-benchmark.md) | the S5 run on a real repository | the numbers that decide whether this design is what v-noc needs |

---

## Read first

- `plan/03-call-tree/14-output.md`
- `plan/03-call-tree/15-checks-and-build-order.md` § "Permanent checks",
  § "Build order" (S5, S9, S11)
- `plan/04-build/00-dev-cli.md`: `--graph`, `--merged`, `--repeat`, `--trace`

---

## Done when

- [ ] `pylspt-dev calls <file>` prints the graph and the tree, for the same
      `call_tree` the server calls
- [ ] ten runs in a row give identical output
- [ ] every fixture passes with no-reuse
- [ ] the S5 numbers are recorded, and every missing edge against the Jedi
      driver is explained

---

→ Start: [`01-the-graph-and-stable-ids.md`](01-the-graph-and-stable-ids.md)
