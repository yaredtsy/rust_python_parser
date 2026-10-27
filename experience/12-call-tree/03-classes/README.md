# 03 — Classes: construction, `self`, state, and the MRO

**Goal:** given any method call — plain, static, class, property, inherited,
chained, or two instances alive at once — you can say what the contexts,
summaries and edges are, and prove to yourself that two objects of one class
never mix their state.

Method calls are function calls with a receiver. The new pieces in this folder
are about **state**: where an object's attributes live, how a write reaches the
caller, and how deep it can go.

---

## How this folder is organised

| | file | the case | the one new idea |
|---|---|---|---|
| 1 | [`01-constructing-an-instance.md`](01-constructing-an-instance.md) | `Foo()` | a new origin, then `__init__` as an ordinary call whose writes are effects on it |
| 2 | [`02-self-and-method-binding.md`](02-self-and-method-binding.md) | `obj.m(x)` vs `Cls.m(obj, x)` | the receiver goes to the first parameter, by position |
| 3 | [`03-attribute-state.md`](03-attribute-state.md) | `self.x = v`, read in another method | state lives in the env; the key carries it into the next call |
| 4 | [`04-two-instances-and-origin-isolation.md`](04-two-instances-and-origin-isolation.md) | two instances, no cross-talk | ★ the fixture to write first |
| 5 | [`05-writes-anywhere-and-deep-writes.md`](05-writes-anywhere-and-deep-writes.md) | writes in any method, from outside, at depth | ★ effects by path, and the `max_path` fallback |
| 6 | [`06-classmethod-staticmethod-property.md`](06-classmethod-staticmethod-property.md) | `@staticmethod`, `@classmethod`, `@property` | three receiver rules, and a call without parentheses |
| 7 | [`07-bound-methods-chains-and-mro-dispatch.md`](07-bound-methods-chains-and-mro-dispatch.md) | `a.b.c.f()`, `super()` | chain depth is recursion in `eval`; the MRO is one wrapper |

Then [`exam.md`](exam.md), and the whole-exercise [`../exam.md`](../exam.md).

---

## Read first

- Everything in [`00-values/`](../00-values/README.md),
  [`01-summaries/`](../01-summaries/README.md) and
  [`02-functions/`](../02-functions/README.md)
- [`plan/03-call-tree/07-attributes-and-self.md`](../../../plan/03-call-tree/07-attributes-and-self.md)
  in full, and [`10-one-call.md`](../../../plan/03-call-tree/10-one-call.md) § "Constructors"
- `experience/08-classes-and-mro/README.md`: the `vnoc::member` wrapper file 7
  relies on is already built and tested in this repo

---

## Done when

- [ ] you can trace `Pipeline(JsonHandler())` to the new object's attributes,
      naming the summary of `__init__` and the effect that writes `handler`
- [ ] you can state the receiver rule, and the bug from getting it backwards
- [ ] you can write `member()`'s order from memory and say which step is the
      precise answer
- [ ] you can explain what the two-instance fixture catches that a
      one-instance fixture can't
- [ ] you can trace `rewire(svc)` writing three levels into `main`'s objects, and
      say what changes past `max_path`
- [ ] you can say why writes to an unknown receiver are dropped
- [ ] you can explain why `a.b.c.f()` works at any depth with no depth-specific
      code
- [ ] you can name the real file that answers "what is `.name` on this type",
      and what it does not answer

---

→ Start: [`01-constructing-an-instance.md`](01-constructing-an-instance.md)
