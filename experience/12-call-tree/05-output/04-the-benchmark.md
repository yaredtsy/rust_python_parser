# 05.04 — The benchmark

**You build:** one run of pylspt and of the Jedi driver on the same real
repository, with the numbers written down. This is the plan's S5: the point
where real code says whether this design gives v-noc what it needs, before
the rest is polished.
**Plan:** `plan/03-call-tree/15-checks-and-build-order.md` § "Build order"
(S5), `plan/04-build/02-milestones.md` § "M8"

---

## When

The plan puts S5 right after calls, returns, effects and recursion work:
here, that is the end of part 04. CPA (04.06) and closures across summaries
(02.07) are already in, so fewer edges are expected to be missing than the
plan's order assumed. Run it again after anything that changes precision.

## What to record

Pick one real repository that v-noc users actually open: mid-sized, with
classes, factories, callbacks and some recursion. Run both drivers on every
file, and record:

| number | why |
|---|---|
| wall time, per file and total | the improvement v-noc needs, or not |
| peak memory | per request; the plan has no figure yet |
| contexts, walks, reuse hits (reused gets per walk) | whether summaries pay on real code |
| edges missing and extra, as caller → callee pairs | parity, at the level that matters to a user |
| `contexts_per_func` cuts | whether 64 is enough |
| group sizes, rounds per group, contexts created per round | whether recursion is cheap in practice |
| `CYCLE` gaps | whether `max_rounds` is ever reached |
| `deep_write_fallbacks` | whether `max_path = 6` is enough |
| `types_dropped` | which `StableType` variant to add next, if any |
| `annotation_fallbacks`, `cpa_collapses`, fan-out hits | where precision is spent |

Compare edges after v-noc's merge (`--merged`), since that is what users see.

## How to read it

- **Every missing edge gets a reason**: a known limit (and which), a counter,
  or a bug. Group them by reason and count. The largest group is the next
  thing to work on.
- **Extra edges** are fine when they come from things Jedi gives up on (its
  execution limits, recursive values). Spot-check a sample by hand against
  the code.
- **Measure the open precision questions** on this repository: `fp_depth = 2`
  against 3 (edges found, contexts, time), `max_path`, `max_rounds`, the
  fan-out caps. The plan's defaults are guesses until these numbers exist.
- **Decide what's next** from the numbers, not from the order of the plan: the
  cross-request cache (v1.1) is worth it only if a project scan spends real
  time re-walking shared code; module globals (`Root::Global`) only if the
  registry pattern shows up in the missing edges.

Write the results into the plan, in `15-checks-and-build-order.md`, next to
S5, with the commit you measured.

---

## The end of the tutorial

You built, in order:

1. a walker that follows every call, like the Jedi driver, and saw it
   explode;
2. summaries for functions, with the two rules that make reuse safe and the
   check that keeps them honest;
3. objects, in the simplest setting;
4. summaries for objects: keys, references, effects, the two phases, deep
   writes, splitting, aliases, and recursion;
5. the output and the checks.

Every structure in `00-values/` is now code you wrote, and every decision in
`plan/03-call-tree/` is one you've seen a fixture fail without. From here, the
plan's remaining milestones (transport, parity, performance) are about
shipping it.

---

## Exercise

1. The benchmark shows 2% of edges missing, all from `fp_depth`. What do you
   measure before changing the default?
2. Reuse hits are low: about one reuse per walk. What does that tell you, and
   which counter do you look at next?
3. Which single number would make you stop and rethink the design?

<details><summary>Answers</summary>

1. The same run with `fp_depth = 3`: how many of the missing edges it
   recovers, how many contexts it adds, and the time. Then decide on the
   trade, with the numbers in the plan.
2. That most keys are distinct: summaries are walked about twice each. Look at
   what makes keys differ: `contexts_per_func` per function, and which
   parameters are relevant. Over-marking in `relevant_params` is the usual
   suspect.
3. Wall time not much better than the Jedi driver's, with reuse low and
   contexts close to walks. That would mean real code doesn't share keys, and
   the design's central bet hasn't paid off.

</details>
