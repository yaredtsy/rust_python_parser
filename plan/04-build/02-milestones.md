# 04.02 — Milestones

Ordered so that each one is independently verifiable and each one leaves you
with something that works. **Do not reorder** — later milestones depend on the
test harnesses that earlier ones build.

---

## M0 — Golden files from the Python driver ★ do this first

**Before writing any Rust.**

1. Assemble a corpus: ≥200 real `.py` files from projects v-noc actually
   analyses, plus the fixture files from
   [`03-call-tree/01`](../03-call-tree/01-what-jedi-actually-does.md#the-invariants-to-test-against),
   [`05`](../03-call-tree/05-binding-arguments.md#test-fixtures-for-this-chapter),
   [`06`](../03-call-tree/06-attributes-and-self.md#fixtures),
   [`07`](../03-call-tree/07-callbacks-and-higher-order.md#fixtures).
2. Run the existing Python driver over all of it. Record every RPC
   request/response pair as JSON on disk.
3. **Record timings too** — per-file, per-method. This is your baseline; without
   it "faster" is an opinion.

Deliverable: `tests/golden/{requests,responses,timings}/`, plus the per-fixture
`.expected.json` files that [`00-dev-cli.md`](00-dev-cli.md)'s `test` command
diffs against. **Generate those from the Python driver, not from Rust output.**

> The open question from
> [`03-call-tree/09`](../03-call-tree/09-path-identity.md#two-calls-to-the-same-function-from-the-same-frame)
> — whether two calls to `emit` with different arguments merge into one node or
> produce two — **is answered here, by observation.** Do not design `Frame`
> until you have looked at that JSON.

## M1 — Build wiring + version report

[`01-wiring-cargo.md`](01-wiring-cargo.md). The smoke test compiles and prints
the resolved Python version — i.e. `pylspt-dev version` from
[`00-dev-cli.md`](00-dev-cli.md). Build the CLI skeleton here; every later
milestone extends it rather than adding a new way to run things.

**Gate:** version printed for 3 different projects (bare dir, `.venv`,
`pyproject.toml` with `requires-python`). Compare each against what Jedi's
`InterpreterEnvironment()` reports. Document every difference now.

## M2 — Syntax layer: `parse_file` without IDs, without MRO

[`02-mapping/01`](../02-mapping/01-parso-to-ruff-ast.md).
Node tree with correct positions, names, `call_index`, `call_col_pos`.

**Gate:** byte-identical `nodes` JSON to golden for the 200-file corpus with
`resolve_mro=false` and pre-injected IDs. Diff on the full tuple, not just
counts.

*This is the milestone that proves your position/LineIndex handling. Everything
downstream reports positions; get it exact here.*

## M3 — ID injection

[`02-mapping/02`](../02-mapping/02-id-injection.md).

**Gate:** idempotent (2nd run = 0 edits); byte-identical output to libcst on the
edge-case corpus (no-docstring, `r"""`, one-line body, tabs, non-ASCII);
`File::sync_path` correctly invalidates so a second `parse_file` sees the IDs.

## M4 — MRO

[`02-mapping/03`](../02-mapping/03-jedi-mro-to-ty-mro.md).

**Gate:** `base_classes` matches golden for the corpus. Expect qualified-name
divergences on nested classes and generics — resolve each one deliberately.

> **At this point you have a working driver for 3 of 6 RPC methods, and should
> already be dramatically faster.** Ship it behind a flag if you can; real usage
> will surface parity issues faster than any test suite.

## M5 — Call tree, context-free

[`03-call-tree/03`](../03-call-tree/03-the-abstract-interpreter.md), with `Env`
always empty. Every callee resolved via ty. Effectively a recursive
`outgoing_calls` with your schema, `is_ancestor` guard, budget, and dedup.

**Gate:** produces a *plausible* tree with the right shape, `target_id`s,
`call_count`s, and merge behaviour. Fixtures 3–12 from
[`03-call-tree/01`](../03-call-tree/01-what-jedi-actually-does.md#the-invariants-to-test-against)
pass. Fixtures 1–2 (context-sensitive) **fail** — expected.

*Deliberately a separate milestone. It exercises all the plumbing — traversal,
frames, budget, serialisation, project filtering, builtin filtering, ID lookup —
without the hard part. When M6 misbehaves you will know the bug is in the
environment, not the scaffolding.*

## M6 — Context sensitivity ★ the project

[`04`](../03-call-tree/04-value-domain.md) →
[`05`](../03-call-tree/05-binding-arguments.md) →
[`06`](../03-call-tree/06-attributes-and-self.md) →
[`07`](../03-call-tree/07-callbacks-and-higher-order.md).

Sub-steps, each independently testable:

| | | Gate |
|---|---|---|
| M6.1 | `AbstractValue` + `lift` from ty | `size_of ≤ 16`; unions explode, don't collapse |
| M6.2 | `Env` + parameter binding | the 9 binding fixtures |
| M6.3 | callee resolution from env | fixtures 1, 2 (cross-scope `emit`, 3-deep pass-through) |
| M6.4 | attributes + `self` + origin | the 8 attribute fixtures, esp. #2 (two live instances) |
| M6.5 | callbacks | the 8 callback fixtures |
| M6.6 | local assignment tracking | ch.10 fixtures 7, 8 |
| M6.7 | return-value flow | ch.10 fixtures 1–6, 12 |
| M6.8 | generalised object state | ch.10 fixtures 9, 10, 11 |

M6.6–M6.8 are [`03-call-tree/10`](../03-call-tree/10-return-values-and-state.md)
— value flow. **Not optional**: Jedi performs this natively today, so omitting
it regresses rather than matching. M6.6 before M6.7 — returns need somewhere to
land or they're unobservable.

**Gate:** full `resolve_calls` parity against golden on the fixture set, then
on the real corpus.

## M7 — Transport

[`03-transport-and-parity.md`](03-transport-and-parity.md). JSON-RPC over HTTP,
`READY port=<n>`, all 6 methods, drop-in for the Python process.

**Gate:** v-noc runs against it unmodified.

*The dev CLI does not go away here — it stays as the fixture runner and the
debugging entry point, sharing `lib.rs` with the server.*

## M8 — Performance

[`03-call-tree/08`](../03-call-tree/08-termination-and-cycles.md) layer 5
(memoising context-independent subtrees), parallelism via `db` snapshots,
profiling.

**Gate:** p50/p95/p99 vs the M0 baseline, on the same corpus, same machine.
Publish the numbers.

*Deliberately last. You cannot optimise what you cannot measure against a
correct baseline, and the memoisation in layer 5 is easy to get subtly wrong
in a way that only shows up as wrong output.*

---

## Effort shape

```
M0  ██                          harness — pays for itself in M2
M1  █                           wiring
M2  ████                        syntax
M3  ███                         IDs
M4  █                           MRO
M5  ████                        scaffolding
M6  ██████████████████████      ★ the project
M7  ██                          transport
M8  ████                        perf
```

If M6 is not the largest bar in your actual schedule, re-read
[`02-mapping/04`](../02-mapping/04-jedi-inference-to-ty.md) — something has been
underestimated.

---

## Risk register

| Risk | Milestone | Mitigation |
|---|---|---|
| Python version divergence changes which files parse | M1 | measure at M1, document, decide policy |
| Column semantics (UTF-8 vs char) break positions | M2 | non-ASCII fixture, early |
| Qualified names don't match Jedi's | M4 | table-driven test on 4 known cases |
| ~~Merge-by-qname semantics misunderstood~~ | — | **resolved:** frame identity is `(parent, qname)`; see [`03-call-tree/09`](../03-call-tree/09-path-identity.md#the-merge-rule) |
| `pub(crate)` blocks member lookup | M6.4 | Option A decided at M1, not M6 |
| Exponential blowup on real input | M6/M8 | budget from day one, not retrofitted |
| Non-deterministic output | M5+ | 10× repeat test in CI from M5 |
| ty inference gaps → `Unknown` | M6 | fallbacks everywhere; count `Unknown`s and report |
| Value flow silently dropped → regression vs Jedi | M6.6–6.8 | ch.10 fixtures; watch the "jedi resolved, ty didn't" row in the divergence log |
| Return-value query double-counts factories in the tree | M6.7 | ch.10 fixture 12 |

The two that actually sink projects: **deciding Option A/B too late** (M6.4 is
the wrong time to discover you need a fork) and **adding the budget too late**
(retrofitting cancellation into a recursive descent is miserable).

---

→ Next: [`03-transport-and-parity.md`](03-transport-and-parity.md)
