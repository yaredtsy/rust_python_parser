# 00 — Values: what flows through one walk

**Goal:** before touching a call site, you know what a value *is* inside one
walk, how it stays small and `Copy` while still naming one specific object, how
the environment resolves a name, and where ty's answers come in.

Everything in this folder lives inside **one walk**: one function body, under
one context. Nothing here survives the walk. How walks are tied together is the
next folder, [`01-summaries/`](../01-summaries/README.md).

---

## How this folder is organised

| | file | what it gives you |
|---|---|---|
| 1 | [`01-abstractvalue.md`](01-abstractvalue.md) | the enum, why `ty::Type` can't do this job, and the precision policy: conditions are never evaluated |
| 2 | [`02-origins.md`](02-origins.md) | `OriginId` and `ValueId`: which object, specifically, without losing `Copy` |
| 3 | [`03-env-and-scope.md`](03-env-and-scope.md) | `Env`, the env-first-then-ty rule, `lift` |
| 4 | [`04-annotation-fallback.md`](04-annotation-fallback.md) | when nothing is known about a parameter, use its annotation |

Then [`exam.md`](exam.md).

---

## Read first

- `plan/03-call-tree/03-design.md`: the design on one page
- `plan/03-call-tree/04-values.md` and `05-the-walk.md`

---

## Done when

- [ ] you can list the `AbstractValue` variants from memory and say when each
      applies, including `Pending`
- [ ] you can name the three kinds of origin and say where each comes from
- [ ] you can explain why every instance ty hands you shares one origin, and
      why that means it can't hold state
- [ ] you can state the env-first-then-ty rule, and where the annotation
      fallback sits
- [ ] you can say why `Env` has no link to the caller

---

→ Start: [`01-abstractvalue.md`](01-abstractvalue.md)
