# 12 — The call tree resolver

**Goal:** you can look at any Python call site — a plain call, a method
call, a callback, a factory, a chained attribute access — and say exactly
what node(s) it produces in the call tree, why, and which piece of the
design is responsible. By the end, `plan/03-call-tree/` reads like a
description of a machine you already understand, not a spec for one you
haven't met yet.

This is the ~70% of the port `plan/README.md` budgets the most effort for,
and the one `experience/README.md` originally said this folder would *not*
teach — because it's design work, and "you cannot get a mental model of a
machine you have not built yet." This exercise is what changed: it teaches
the design the same hands-on way exercises 00–11 teach everything else,
*before* you write `src/interp/`, using fixtures, worked traces, and the
real Jedi driver as ground truth — not by handing you finished Rust to
copy.

---

## How this exercise is organised

Three folders. The first is infrastructure both of the other two depend
on; read it first, in full, before either.

```
12-call-tree/
├── 00-values/ ....... the value domain: AbstractValue, OriginId, Env,
│                      the annotation fallback, Frame/PathKey, the budget
│                      covers: plan/03-call-tree/03, 04, 05 (environment),
│                               08, 09
│
├── 01-functions/ .... every case a plain function call can produce:
│                      defaults, *args, closures, returns, callbacks,
│                      curry/partial, decorators, recursion
│                      covers: plan/03-call-tree/05, 07, 08, 10
│
└── 02-classes/ ...... every case a method call can produce: construction,
                       self-binding, attribute state, origin isolation,
                       inheritance, static/class/property, MRO dispatch
                       covers: plan/03-call-tree/06, and the already-merged
                               vnoc::member wrapper from exercise 08
```

| | folder | what it gives you |
|---|---|---|
| 1 | [`00-values/`](00-values/README.md) | the vocabulary everything else is built from — **read first, no exceptions** |
| 2 | [`01-functions/`](01-functions/README.md) | the function-call cases, one file per case |
| 3 | [`02-classes/`](02-classes/README.md) | the method-call cases, one file per case |

Then [`exam.md`](exam.md) — the whole-exercise exam, cross-cutting both
folders with the canonical fixtures from `plan/03-call-tree/01` and `/02`.

---

## Read first

- `plan/03-call-tree/01-what-jedi-actually-does.md` and
  `02-why-ty-alone-cannot.md` — the problem this whole exercise exists to
  solve, read once before any of the mechanism
- `experience/08-classes-and-mro/README.md` — the `vnoc::member` /
  `vnoc::class_mro` wrappers this exercise's `02-classes/07` leans on are
  **already built and tested** in this repo, not hypothetical
- `plan/01-crates/04-public-vs-private-api.md` — why the interpreter lives
  in `pylspt` and the fork stays wrapper-only

---

## ⚠ There is no Rust binary to run yet — and what "predict, then run" means here

Every exercise before this one ends with you running your own Rust and
diffing its output against a golden file. `src/` in this repo has **no**
`interp/` module yet — no `value.rs`, `env.rs`, `bind.rs`, `member.rs`,
`frame.rs` (the module layout `plan/03-call-tree/03` §"Module layout"
specifies). That's what you'll eventually write, and this exercise is the
walkthrough that comes before you write it — the same relationship
exercise 09 has to `ty_ide::outgoing_calls`, generalised to the whole
interpreter.

So here, "predict, then run" means three things instead of one:

1. **Predict** the tree by hand, using the rules taught in these files.
2. **Check against the real Python driver.** It isn't pseudocode —
   `/Users/yared/Documents/Programing/ide/v-noc/src/lsp/py/vnoc_lsp_python/call_resolver.py`
   is real, runnable Jedi code. If you have `jedi` installed, run it
   directly on any fixture in this folder and compare.
3. **Compare both against `plan/03-call-tree/`'s own worked traces** —
   several fixtures across these three folders are the plan's own
   "Fixtures" tables, turned into runnable Python with worked answers
   alongside them.

When you do eventually write `src/interp/`, treat every `exam.md` in this
exercise as your first integration test suite — the fixtures here are
already the right shape for `#[test]` functions once you have something to
run them against.

---

## Done when

- [ ] you've finished all three folders' own "done when" checklists
- [ ] you can draw, from memory, the canonical cross-scope fixture
      (`read_json`/`read_csv`/`emit`) and explain why it — not the
      same-frame version — is the one that proves context sensitivity
- [ ] you can draw the two-instance isolation fixture and say exactly what
      `OriginId` bug it's built to catch
- [ ] you can name every scaling brake from `00-values/06` and the one
      optimisation that matters most on a real codebase
- [ ] you can point at the real, already-merged `vnoc::member` wrapper and
      say precisely what it answers and what it deliberately leaves to you
- [ ] you can explain the annotation-fallback tier, where it sits in the
      resolution order, and why the ordering is the entire safety argument

---

→ Start: [`00-values/README.md`](00-values/README.md)
