# 02 — Functions: every shape a plain call takes

**Goal:** given any function call — plain, defaulted, starred, nested,
returning a value, passed as a callback, curried, decorated, or recursive — you
can say which contexts it creates, what each summary holds, and what the
rendered tree looks like.

Everything here is built from [`00-values/`](../00-values/README.md) (one walk)
and [`01-summaries/`](../01-summaries/README.md) (how walks are tied together).
No new machinery, except for recursion in file 8.

---

## How this folder is organised

| | file | the case | the one new idea |
|---|---|---|---|
| 1 | [`01-calling-a-plain-function.md`](01-calling-a-plain-function.md) | the base case | the full trace, step by step |
| 2 | [`02-defaults-and-keywords.md`](02-defaults-and-keywords.md) | `f(a, b=X)` | defaults belong to the defining scope |
| 3 | [`03-star-args-and-splats.md`](03-star-args-and-splats.md) | `*args`, `**kwargs`, `f(*items)` | `Unknown` rather than a guessed alignment |
| 4 | [`04-closures-and-nested-functions.md`](04-closures-and-nested-functions.md) | nested `def`, free variables | captured values come from where the closure was made |
| 5 | [`05-returns-and-value-flow.md`](05-returns-and-value-flow.md) | `w = make(); w.write()` | returns are part of the summary |
| 6 | [`06-callbacks-and-higher-order.md`](06-callbacks-and-higher-order.md) | functions as values | different callbacks, different contexts |
| 7 | [`07-curry-partial-and-decorators.md`](07-curry-partial-and-decorators.md) | curry, `functools.partial`, decorators | which ones resolve, and why |
| 8 | [`08-recursion.md`](08-recursion.md) | direct, mutual, parsers, visitors | ★ pending values, groups, the fixpoint |

Then [`exam.md`](exam.md).

**Files 1, 5 and 8 are the core.** 1 is the trace you'll replay for every call
you ever debug. 5 is what a line-by-line port of `call_resolver.py` would lose.
8 is the hardest part of the design, and the one real code stresses most.

---

## Read first

- [`plan/03-call-tree/06-binding-arguments.md`](../../../plan/03-call-tree/06-binding-arguments.md),
  [`10-one-call.md`](../../../plan/03-call-tree/10-one-call.md),
  [`11-closures-and-callbacks.md`](../../../plan/03-call-tree/11-closures-and-callbacks.md),
  [`12-recursion.md`](../../../plan/03-call-tree/12-recursion.md)

---

## Done when

- [ ] you can trace `emit(JsonWriter(), 1)` from `eval_call` to the leaf,
      naming every step, without looking at file 1
- [ ] you can say where a default's value comes from, and why binding never
      evaluates a call
- [ ] you can say why a splat at the call site makes every parameter `Unknown`
- [ ] you can trace an escaping closure (`make_handler`) and say why it doesn't
      matter where it's called from
- [ ] you can write the summary of a factory and say how the caller gets the
      object
- [ ] you can say how many contexts of `run` two different callbacks create
- [ ] you can trace a parser's cycle through its fixpoint rounds, and say why
      each body is walked once
- [ ] you can say what a `Recursive` leaf is, and how it differs from Jedi's
      output

---

→ Start: [`01-calling-a-plain-function.md`](01-calling-a-plain-function.md)
