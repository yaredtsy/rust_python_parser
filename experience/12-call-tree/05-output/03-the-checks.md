# 05.03 — The checks

**You build:** nothing new in the analysis. You gather every check this
tutorial built into the set that runs on every change, add the two that need
the whole pipeline (determinism and parity with the Jedi driver), and write
down what each check can and cannot see.
**Plan:** `plan/03-call-tree/15-checks-and-build-order.md` § "Permanent
checks", `14-output.md` § "Determinism", `04-build/00-dev-cli.md` § "`--repeat`",
§ "`--trace`"

---

## The checks

| check | catches | can't catch |
|---|---|---|
| **expected trees** (every fixture) | any change to what a fixture's tree is, including a known limit closing | a bug no fixture exercises |
| **no-reuse mode** (`check_both`) | a walk that reads something outside its key (rule 1) | both modes losing the same edge |
| **inline oracle** (if kept, step 04.09) | a summary that differs from a key-free walk, outside the named differences | a bug the two share (binding, `member`, branches) |
| **determinism** (`--repeat 10`) | hash-map iteration order reaching the output | a wrong answer given consistently |
| **parity** (golden files from the Jedi driver) | an edge Jedi finds and pylspt doesn't | an edge both miss |
| **counters** (`Stats`) | *why* an edge is missing: a cut, a cap, a dropped type, a fallback | nothing by themselves; they explain |

No single row is enough. A missing edge is a bug until one of them explains
it.

## Determinism

Same input, same output, ten runs in a row. Two things threaten it:

- **hash-map iteration order.** Never iterate an `FxHashMap` to produce
  output. Graph output is sorted by stable ids (step 01); tree children follow
  evaluation order, then definition order within one site (step 01.09). Neither
  depends on a map.
- **the deadline.** A slow run can cut where a fast one doesn't. The check
  runs with `deadline=off`, or asserts that no `Cut::Deadline` happened.

```bash
pylspt-dev calls big_file.py --repeat 10 --limits deadline=off
# → OK: 10 identical runs
# → FAIL: run 4 differs (children reordered under app.emit)
```

If parallelism is added later (rayon across files, `plan/04-build/02` M8), its
merge order is the third threat, and the same check covers it.

## Parity with the Jedi driver

Record the Jedi driver's output on a corpus as golden files, once
(`plan/04-build/02-milestones.md` § "M0"). Then compare pylspt's **merged**
tree (`--merged`, step 02) with it. Parity means: pylspt finds at least Jedi's
edges, and every difference is explained. The expected differences:

- Jedi stops after 6 executions of a function and 200 per inference state.
  Past those it returns nothing, so edges in busy code are missing from its
  output, and present in pylspt's.
- Jedi drops recursive calls. pylspt shows `[recursive]` leaves.
- Values of recursive calls resolve in pylspt (part 02, steps 05–06).
- Values flow through functions without IDs; Jedi skipped those.

An **extra** edge is not automatically a bug. A **missing** edge is a bug
until it is explained, by a known limit, by a counter, or by a fix.

Every **[check]** in this tutorial is a question for the golden files:
lambdas, comprehensions, property nodes, `super().__init__`, the order of
several callees at one site, setters.

## The counters

`Stats`, returned with every response and printed by `--stats`. One counter
per limit and per mechanism, so a missing edge can be explained by a number:

| counter | from step |
|---|---|
| `contexts`, `walks`, `reuse_hits` | 02.01, 02.03 |
| `parked`, `groups`, `group_sizes`, `rounds`, `contexts_per_round`, `joins`, `merges`, `cycle_gaps` | 02.05–02.06 |
| `cuts` (by kind) | 02.08 |
| `annotation_fallbacks`, `chain_fanout_hits` | 03.07 |
| `deep_write_fallbacks` | 04.05 |
| `cpa_collapses` | 04.06 |
| `types_dropped` | 05.01 |
| `render_cuts` | 05.02 |

## `--trace`

The checks say *that* something is wrong. The trace says *why*. Log every
decision, especially the negative ones, each with its own marker: a builtin
skipped, a callee that is not project code, a callee with no ID (shown: no,
walked: yes, the one that wastes your time), a reuse, a pending call, a cut, a
fan-out cap, an annotation fallback, a deep-write fallback. Print each key,
and whether each name was an env hit or went to ty.
`plan/04-build/00-dev-cli.md` § "`--trace`" has the format.

### Yours

- `--repeat`, `--stats`, `--trace` in the dev CLI.
- A test that runs every fixture twice and compares the JSON byte for byte.
- The parity differ over the golden files, with the expected differences
  above as named filters, never as a blanket "ignore".

---

## Exercise

1. A fixture passes in all three runs of `check_both`, but parity shows Jedi
   finds an edge pylspt doesn't. Which checks were blind to it, and why?
2. Why does the determinism check need the deadline off, but not `max_stack`
   off?
3. Write down, for one known limit of your choice, which counter would point
   at it on a real project, if any.

<details><summary>Answers</summary>

1. The expected tree (nobody wrote that edge into the fixture), no-reuse
   (both modes lose it the same way), and the inline oracle if the bug is in
   shared code. Only an outside answer (Jedi's) could see it.
2. `max_stack` depends on the code and the path, which are the same on every
   run, so it cuts in the same place every time. The deadline depends on the
   clock.
3. Examples: `deep_write_fallbacks` points at writes past `max_path`;
   `annotation_fallbacks` at edges that came from annotations;
   `cycle_gaps` at groups that hit `max_rounds`. The `fp_depth` read limit has
   no counter: a read that falls to ty looks like any other, which is one
   reason the plan measures K = 2 against K = 3 directly.

</details>

---

→ Next: [`04-the-benchmark.md`](04-the-benchmark.md)
