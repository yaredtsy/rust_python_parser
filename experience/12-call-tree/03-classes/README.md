# 02 — Classes: construction, `self`, state, and the MRO

**Goal:** given any method call — plain, static, class, property,
inherited, chained, or two instances alive at once — you can say what the
call tree looks like, and specifically, you can prove to yourself that two
instances of the same class never cross-contaminate each other's state.

Like `01-functions/`, everything here is built out of
[`00-values/`](../00-values/README.md)'s six concepts, with exactly one new
mechanism added: **the attribute store, keyed on `OriginId`**
([`03`](03-attribute-state-after-init.md)). Your project owner named this
chapter explicitly as the hard case — *"it works for callback, attribute
calls, stuff"* — read it slower than the others.

---

## How this folder is organised

Seven files, in the order you should read them:

| | file | the case | the one new idea |
|---|---|---|---|
| 1 | [`01-constructing-an-instance.md`](01-constructing-an-instance.md) | `Foo()` | constructing doesn't run class code — it mints a value, then calls `__init__` like any function |
| 2 | [`02-self-and-method-binding.md`](02-self-and-method-binding.md) | `obj.m(x)` vs `Cls.m(obj, x)` | the receiver-binding order, and why both syntaxes bind identically |
| 3 | [`03-attribute-state-after-init.md`](03-attribute-state-after-init.md) | ★ `self.x = v` then read elsewhere | the attribute store, keyed on `OriginId` |
| 4 | [`04-two-instances-and-origin-isolation.md`](04-two-instances-and-origin-isolation.md) | ★★ two instances, no cross-talk | **the fixture to write first** — proves 3 is actually correct |
| 5 | [`05-assignment-outside-init-and-external-mutation.md`](05-assignment-outside-init-and-external-mutation.md) | writes outside `__init__`, writes from outside the class | one generalisation, one deliberate refusal to generalise further |
| 6 | [`06-classmethod-staticmethod-property.md`](06-classmethod-staticmethod-property.md) | `@staticmethod`/`@classmethod`/`@property` | three more receiver-binding rules, one reachability rule |
| 7 | [`07-bound-methods-chains-and-mro-dispatch.md`](07-bound-methods-chains-and-mro-dispatch.md) | `a.b.c.f()` at any depth, `super()`, bound methods as callbacks | chain depth is `eval` recursion; the MRO walk is **one already-merged wrapper**, not code you write |

Then [`exam.md`](exam.md) for the whole folder — and
[`../exam.md`](../exam.md) for the whole exercise, combining both folders.

---

## Read first

- Everything in [`00-values/`](../00-values/README.md) and
  [`01-functions/`](../01-functions/README.md) — method calls are function
  calls with one extra binding rule and one extra store
- `plan/03-call-tree/06-attributes-and-self.md` in full
- `experience/08-classes-and-mro/README.md` — the MRO wrapper
  (`vnoc::class_mro`, `vnoc::member`) this folder's file 7 relies on is
  **already built and tested** in this repo; that exercise is where it was
  introduced

---

## Done when

- [ ] you can trace `Pipeline(JsonHandler())` from `resolve_call` down
      through `__init__`'s body, naming every step
- [ ] you can state the receiver-binding order rule and predict the bug
      that results from getting it backwards
- [ ] you can write `member()`'s three-tier priority order from memory, and
      say which tier is "the precise answer"
- [ ] you can explain, with the two-instance fixture specifically, what
      `OriginId` collision would look like and why this fixture (not the
      single-instance one) is the test that catches it
- [ ] you can state why writes to an unknown receiver are dropped rather
      than approximated
- [ ] you can produce the locals map for a `@staticmethod` call and for a
      `@classmethod` call, and say why `@property` is a reachability
      question, not a binding one
- [ ] you can explain why `a.b.c.f()` and `a.b.c = v` work at any depth
      without any depth-specific code, and what happens at an `Unknown` hop
- [ ] you can name the real file (`vendor/ruff/crates/ty_python_semantic/src/types/vnoc.rs`)
      that answers "what is `.name` on this type" and explain what it does
      *not* answer

---

→ Start: [`01-constructing-an-instance.md`](01-constructing-an-instance.md)
