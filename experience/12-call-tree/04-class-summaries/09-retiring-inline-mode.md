# 04.09 — Retiring inline mode

**You build:** no new feature. You run the final gate, list every difference
between the two modes, and decide what inline mode becomes now that summary
mode does its job.
**Fixtures:** every fixture of parts 01–04
**Plan:** `plan/03-call-tree/15-checks-and-build-order.md` § "Permanent
checks", § "Known limits"

---

## The gate

```rust
#[test]
fn every_fixture_in_summary_mode() {
    for fixture in ALL_FIXTURES {          // every file with an `# expect` block, parts 01–04
        check_both(fixture);               // inline, summary, summary without reuse
    }
}
```

All three runs must pass on every fixture. If one fails now, it is a bug
from an earlier step that a later one exposed. Find the first step whose
fixture fails, and fix it there.

## Every difference, named

Each mode-specific block in the fixtures is one of three kinds.

**Summary mode finds more:**

| fixture | why |
|---|---|
| `02/pending.py` `walk` | a recursive call's value is found by the fixpoint; inline mode stops at the guard with `Unknown` |
| `02/groups.py` `ping_entry`, `merge_entry` | the same, across a group |
| `04/split.py` | CPA keeps each handler with its own key; inline mode walks `build` once with both |
| `04/recursion_objects.py` `use_walk` | a pending object's method, called in the fixpoint |

**Inline mode finds more**, because a key or a summary is bounded:

| fixture | the bound | listed as |
|---|---|---|
| `04/in_keys.py` `deep` | `fp_depth = 2`: a read three steps deep | known limit, measure K = 2 vs 3 |
| `04/deep_write_capped.py` | `max_path`: a write past it is a weak `Unknown`, marked | `DEEP_WRITE` gap |
| `04/aliases.py` `nested` | aliasing through an attribute is not in the key | known limit, alias pairs of paths |

**Summary mode marks what it cut:**

| fixture | |
|---|---|
| `02/cuts.py` | `max_stack`: `[incomplete]`, never reused |

Everything else is the same tree in both modes. That is the claim this part
set out to prove: summaries change the cost, not the answer, except where a
bound says so, and every bound is named, counted and pinned.

## The known limits, both modes

These fixtures give the same tree in both modes, and Python finds an edge
neither does. They are pinned so that a fix shows up as a changed expected
tree:

| fixture | limit |
|---|---|
| `01/branches.py` `loop` | a loop body is walked once |
| `03/isolation.py` `global_write` | writes to module globals are dropped |
| `04/recursion_objects.py` `identity_limit` | an object doesn't keep its identity around a cycle |
| `04/recursion_objects.py` `tree_fill` | effects inside a cycle reach only the exit state |

The plan lists each one, with its fix, in `15` § "Known limits" and § "Growth
path".

---

## What inline mode becomes

Two mechanisms that answer "what does this call do" must not both serve
requests. The request path uses summary mode only: `Options::default()` is
`Mode::Summary` from now on. Tests that measure inline mode already say
`inline()` (step 02.02), so none of them changes.

What to do with inline mode itself is a choice:

| | keep it as a test oracle | delete it |
|---|---|---|
| what stays | `walk_inline`, the inline node merge, `Mode::Inline` behind `#[cfg(test)]` (or a dev-CLI flag) | nothing |
| what it gives | a second, independent answer on every fixture. A difference that isn't in the lists above is a bug in one of the two | one way to follow a call |
| what it costs | every change to the walker must keep inline mode working; it is exponential, so small fixtures only | the cross-check |

**My recommendation: keep it, test-only.** It is small (one function and a
merge), it shares everything else with summary mode, and it is the only check
that compares summaries against an answer that has no keys at all. No-reuse
mode can't do that: it compares summaries with summaries. If you keep it,
write the rule down where the code is:

```rust
/// Follow every call into the callee's body, like the Jedi driver.
/// TEST ORACLE ONLY: exponential, no limits, never on the request path.
/// Its trees must equal summary mode's, except for the differences named in
/// experience/12-call-tree/04-class-summaries/09-retiring-inline-mode.md.
#[cfg(any(test, feature = "oracle"))]
Inline,
```

If you delete it instead, the mode-specific blocks keep only their `summary`
half, and `check_both` becomes summary with and without reuse.

Either way, this is a decision about the code that the plan doesn't describe
yet. Record it in `plan/03-call-tree/15-checks-and-build-order.md`, under
"Permanent checks", in the same commit.

---

## Where you are: the end of part 04

The analysis is complete. Compare your `src/interp/` with
`00-values/README.md`: every structure there exists, filled in by your own
code, for reasons you have seen fail without it.

What is left is the output: stable identities, the wire format v-noc reads,
the checks that run on every change, and the benchmark against the Jedi
driver. That is part 05.

---

## Exercise

1. Pick one fixture where summary mode finds more. Explain the difference in
   one sentence, naming the mechanism inline mode lacks.
2. Pick one where inline mode finds more. Which limit, and what would raising
   it cost?
3. A new fixture shows a difference that is in neither list. What are the
   possible causes, and which do you check first?

<details><summary>Answers</summary>

1. `split.py`: summary mode walks `build` once per handler class (CPA), so
   each `Job` keeps the key that matches its handler; inline mode walks it once
   with both handlers, and the union loses which goes with which.
2. `in_keys.py` `deep`: `fp_depth = 2`. Raising it to 3 makes every key hold
   one more level of attributes: bigger keys, fewer shared summaries, more
   contexts. The plan measures K = 2 against K = 3 before settling it.
3. A bug in summary mode (a conversion that loses something: check the key,
   the starting env, the summary and the resolved values, in that order), a
   bug in inline mode, or a new kind of bound that should be named and added to
   a list. Check summary mode first: it is the one that ships.

</details>

---

→ Next part: [`../05-output/README.md`](../05-output/README.md)
