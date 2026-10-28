# 03 — Classes, inline mode

**Goal:** objects. Constructing them, calling their methods, and following
what they hold: `self.handler = handler` in `__init__`, read back as
`self.handler.process()` in another method, on the right object, on this path.

Everything here is built in **inline mode** first, where it is simplest: every
call is walked right there, one walk holds every object of the request, and a
write in a callee is simply visible to the caller afterwards. Summary mode
doesn't handle objects yet; these fixtures run in inline mode only until part
04 brings summary mode to the same trees.

---

## The steps

| | step | you build | the decision |
|---|---|---|---|
| 1 | [`01-constructing.md`](01-constructing.md) | `Foo()` → a node, `__init__` walked, a new object | an object is named by where it was built: its origin |
| 2 | [`02-methods-and-self.md`](02-methods-and-self.md) | `obj.method()` | ty finds the method; you bind your own receiver |
| 3 | [`03-attribute-state.md`](03-attribute-state.md) | `self.h = h` … `self.h.process()` | state lives in the env, keyed by (origin, name), and flows back out of a call |
| 4 | [`04-two-instances-and-opaque.md`](04-two-instances-and-opaque.md) | two objects alive at once; objects from ty | origins keep objects apart; an object nobody built here holds no state |
| 5 | [`05-writes-from-anywhere.md`](05-writes-from-anywhere.md) | writes outside `__init__`, outside the class, on one path | one write rule for every receiver. Several candidates: a weak write |
| 6 | [`06-static-class-property.md`](06-static-class-property.md) | `@staticmethod`, `@classmethod`, `@property` | the decorator decides the receiver. A property is a call |
| 7 | [`07-chains-super-and-annotations.md`](07-chains-super-and-annotations.md) | `a.b.c.f()`, `super()`, `def f(h: Handler)` | one `member` per hop. `super()` is the same receiver. An annotation is a claim, and is counted |

**Steps 1, 2 and 3 are the core.** 1 introduces identity, 2 the receiver
rule, 3 state. The rest are those three applied to more shapes.

---

## What you add

```
AbstractValue += Class(def) | Instance { class, origin } | BoundMethod { func, receiver }
Walk          += origins: OriginKey → OriginId,  values: ValueArena (receivers)
Env           += attrs: (OriginId, Name) → Values
Frame         += exits: [Env]            (state flows back out of a call)
src/interp/member.rs:  member on instances and classes, bind_attribute
```

`00-values/07-origins-and-ids.md` and `08-the-value.md` show where these end
up. In inline mode an origin is simpler than there: it only has to be unique
within the one walk of the request.

---

## Read first

- `plan/03-call-tree/07-attributes-and-self.md`: the plan's version of steps
  2–7, written for summary mode. Read it for the rules, which are the same.
- `experience/08-classes-and-mro/README.md`: the `vnoc::member` wrapper and its
  test, already in this repository.

---

## Done when

- [ ] every test from steps 1–7 passes in inline mode
- [ ] every part 01 and 02 test still passes in both modes
- [ ] you can say, for each step, which fixture line breaks without its
      decision

---

→ Start: [`01-constructing.md`](01-constructing.md)
