# 00 — Values: the infrastructure function and class calls share

**Goal:** before touching a single call site, you know what a value *is* in
this interpreter, how it stays small and `Copy` while still being
path-specific, how the environment resolves a name, and the three brakes
that keep the whole thing from running forever on real code.

This folder comes **first** on purpose. Both `01-functions/` and
`02-classes/` are built entirely out of the six things taught here —
`AbstractValue`, `OriginId`/`ValueId`, `Env`, the annotation fallback,
`Frame`/`PathKey`, and the budget. Skipping ahead means re-deriving this
vocabulary mid-lesson, in the middle of a harder problem.

---

## How this folder is organised

Six files, each one concern, read in order:

| | file | what it gives you |
|---|---|---|
| 1 | [`01-abstractvalue.md`](01-abstractvalue.md) | the enum itself, and why `ty::Type` alone can't do this job |
| 2 | [`02-origin-and-identity.md`](02-origin-and-identity.md) | `OriginId`, `ValueId` — staying `Copy` without losing "which one, specifically" |
| 3 | [`03-env-and-scope.md`](03-env-and-scope.md) | `Env`, the env-first-then-ty rule, `lift()` |
| 4 | [`04-annotation-fallback.md`](04-annotation-fallback.md) | ★ a third resolution tier: fall back to the declared annotation before giving up to `Unknown` |
| 5 | [`05-frame-pathkey-and-budget.md`](05-frame-pathkey-and-budget.md) | `Frame`, the `(parent, qname)` identity rule, `PathKey`, the ancestor guard |
| 6 | [`06-scaling-budget-and-memoization.md`](06-scaling-budget-and-memoization.md) | the four brakes, and the one optimisation that matters most at scale |

Then [`exam.md`](exam.md) for the whole folder.

---

## Read first

- `plan/03-call-tree/03-the-abstract-interpreter.md` — the architecture this
  folder's vocabulary implements
- `plan/03-call-tree/04-value-domain.md`, `05-binding-arguments.md` §"The
  environment", `08-termination-and-cycles.md`, `09-path-identity.md`

---

## ⚠ There is no Rust binary to run yet

Every earlier exercise in this folder (`00-setup` through `11-capstone`)
ends with you running your own Rust and comparing its output to a golden
file. **This exercise is different.** `src/` in this repo has no
`interp/value.rs`, `env.rs`, `bind.rs` or `frame.rs` yet — that module layout
(`plan/03-call-tree/03` §"Module layout") is what you'll eventually write,
and this exercise is the design walkthrough that comes *before* you write
it, the way exercise 09 showed you the gap before you tried to close it.

So "predict, then run" here means:

1. **Predict** the tree by hand, using the rules in these files.
2. **Check** against the real Python driver —
   `/Users/yared/Documents/Programing/ide/v-noc/src/lsp/py/vnoc_lsp_python/call_resolver.py`
   is real, runnable Jedi code, not pseudocode. If you have a Python
   environment with `jedi` installed, you can run it on these fixtures
   directly.
3. **Compare** both against the worked traces in `plan/03-call-tree/` —
   several fixtures in these files are lifted straight from that plan's own
   "Fixtures" tables, so the expected shape is already written down there
   too.

When you later build `src/interp/`, this exercise *is* your spec — the
`exam.md` fixtures across all three folders are what your first integration
tests should assert.

---

## Done when

- [ ] you can list all seven `AbstractValue` variants from memory and say
      when each one applies
- [ ] you can explain, without looking it up, why `OriginId` must be
      different for two instances of the same class
- [ ] you can state the env-first-then-ty rule, and where the annotation
      fallback sits relative to it
- [ ] you can state the `(parent frame, target_qname)` identity rule and
      predict `call_count` for a 3-call same-frame fixture
- [ ] you can name all four scaling brakes and which one is a global,
      request-wide counter
- [ ] you can explain, in one sentence, why `is_context_independent` gets
      stricter once local assignment tracking exists

---

→ Start: [`01-abstractvalue.md`](01-abstractvalue.md)
