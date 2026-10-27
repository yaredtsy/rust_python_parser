# 04.02 — Milestones

Ordered so that each one is independently verifiable and each one leaves you
with something that works. **Do not reorder** — later milestones depend on the
test harnesses that earlier ones build.

---

## M0 — Golden files from the Python driver ★ do this first

**Before writing any Rust.**

1. Assemble a corpus: ≥200 real `.py` files from projects v-noc actually
   analyses, plus the fixture files from
   `plan/03-call-tree/` (each chapter ends with its fixtures; the index is in
   [`15`](../03-call-tree/15-checks-and-build-order.md#fixtures-for-the-core)).
2. Run the existing Python driver over all of it. Record every RPC
   request/response pair as JSON on disk.
3. **Record timings too** — per-file, per-method. This is your baseline; without
   it "faster" is an opinion.

Deliverable: `tests/golden/{requests,responses,timings}/`, plus the per-fixture
`.expected.json` files that [`00-dev-cli.md`](00-dev-cli.md)'s `test` command
diffs against. **Generate those from the Python driver, not from Rust output.**

> The goldens are the **saved** shape: the Jedi driver merges two calls to the
> same function from one frame into one node. pylspt sends one node per call
> site and v-noc merges on save
> ([`03-call-tree/14`](../03-call-tree/14-output.md#call_count-and-v-nocs-merge)),
> so the differ merges pylspt's output with the same rule before comparing.

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

## M5 — Call tree, part 1: the core and the benchmark

[`03-call-tree/`](../03-call-tree/03-design.md), build steps S1–S5 from
[`15`](../03-call-tree/15-checks-and-build-order.md#build-order): the summary
types, the walker, one call with snapshot resolve and apply, the solver with
no-reuse mode, recursion groups and their fixpoint, then a benchmark against
the Jedi driver on one real repository.

**Gate:** each step's own gate, and the S5 numbers recorded. S5 decides whether
the design gives v-noc the improvement it needs before the rest is built.

## M6 — Call tree, part 2 ★ the project

S6–S11: CPA combos, closures, the remaining cost limits, graph output and
stable IDs, the known-limit fixtures and the depth measurements, then the PyCG,
Jarvis and full Jedi parity runs.

**Gate:** full `resolve_calls` parity against golden on the fixture set, then on
the real corpus, with every difference explained
([`15`](../03-call-tree/15-checks-and-build-order.md#parity-with-the-jedi-driver)).

## M7 — Transport

[`03-transport-and-parity.md`](03-transport-and-parity.md). JSON-RPC over HTTP,
`READY port=<n>`, all 6 methods, drop-in for the Python process.

**Gate:** v-noc runs against it unmodified.

*The dev CLI does not go away here — it stays as the fixture runner and the
debugging entry point, sharing `lib.rs` with the server.*

## M8 — Performance

Parallelism via `db` snapshots, profiling, and the v1.1 cross-request summary
cache ([`15`](../03-call-tree/15-checks-and-build-order.md#growth-path)).
Summaries per context are already the main optimisation; they are the design,
not an add-on.

**Gate:** p50/p95/p99 vs the M0 baseline, on the same corpus, same machine.
Publish the numbers.

*Deliberately last. You cannot optimise what you cannot measure against a
correct baseline, and a cross-request cache is easy to get subtly wrong in a way
that only shows up as wrong output. The no-reuse check is what catches it.*

---

## Effort shape

```
M0  ██                          harness — pays for itself in M2
M1  █                           wiring
M2  ████                        syntax
M3  ███                         IDs
M4  █                           MRO
M5  ██████████                  call tree core + benchmark
M6  ██████████████████          ★ the project
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
| ~~Merge-by-qname semantics misunderstood~~ | — | **resolved:** pylspt sends one node per call site, v-noc merges by `(parent, qname)` on save; see [`03-call-tree/14`](../03-call-tree/14-output.md#call_count-and-v-nocs-merge) |
| ~~`pub(crate)` blocks member lookup~~ | — | **resolved:** forked, with `types/vnoc.rs` wrappers; see [`01-crates/04`](../01-crates/04-public-vs-private-api.md) |
| Exponential blowup on real input | M5 | summaries per context, cost limits from S3, measured at S5 |
| Non-deterministic output | M5+ | 10× repeat test in CI from M5, deadline off |
| ty inference gaps → `Unknown` | M5 | fallbacks everywhere; count `Unknown`s and report |
| Value flow silently dropped → regression vs Jedi | M5 | the walk and one-call fixtures; watch the "jedi resolved, ty didn't" row in the divergence log |
| Reuse changes the output (a walk read something its key doesn't hold) | M5 | no-reuse mode on every fixture, from S3 |
| Recursion-heavy code (parsers, visitors) re-walked or losing values | M5 | the S4 recursion fixtures; group sizes and rounds in the S5 numbers |

The two that actually sink projects: **deciding Option A/B too late** and
**adding the cost limits too late** (retrofitting cancellation into a
recursive descent is miserable).

The first one is now settled — the fork landed at exercise 08 rather than in
the middle of the call tree, which is roughly as late as it could have been
without hurting. The cost of having answered it late was one hand-rolled C3 in
`src/mro.rs` and the `covering_node` round trip around it; the cost of answering
it during the call tree would have been a half-written `member.rs`. Keep the row as a reminder of what a
late decision looks like when you get away with it.

---

→ Next: [`03-transport-and-parity.md`](03-transport-and-parity.md)
