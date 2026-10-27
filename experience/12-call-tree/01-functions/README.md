# 01 — Functions: every case a plain function call can produce

**Goal:** given any function call in a Python file — plain, defaulted,
starred, nested, returning a value, passed as a callback, curried,
decorated, or recursive — you can say by hand what the call tree looks
like, and why.

Everything here is built entirely out of [`00-values/`](../00-values/README.md)'s
six concepts. Nothing in this folder introduces a new type — only new
*shapes of call site* for the same `resolve_call` / `bind_parameters` /
`walk_body` machinery to handle.

---

## How this folder is organised

Eight files, one case each, in the order you should read them — later
files lean on earlier ones:

| | file | the case | the one new idea |
|---|---|---|---|
| 1 | [`01-calling-a-plain-function.md`](01-calling-a-plain-function.md) | ★ the base case | the full trace, step by step |
| 2 | [`02-defaults-and-keywords.md`](02-defaults-and-keywords.md) | `f(a, b=X)` | defaults evaluate in the **defining** scope |
| 3 | [`03-star-args-and-splats.md`](03-star-args-and-splats.md) | `*args`, `**kwargs`, `f(*items)` | bind to `Unknown` rather than guess an alignment |
| 4 | [`04-closures-and-nested-functions.md`](04-closures-and-nested-functions.md) | nested `def`, free variables, lambdas | ★ `Env.enclosing` — the closure chain, and the gap in an earlier draft's `closure_parent` that led to it |
| 5 | [`05-returns-and-value-flow.md`](05-returns-and-value-flow.md) | ★ `w = make(); w.write()` | one walker: the tree walk stores each return, the value read looks it up |
| 6 | [`06-callbacks-and-higher-order.md`](06-callbacks-and-higher-order.md) | functions as values | dispatch on the **value**, never on the syntax |
| 7 | [`07-curry-partial-and-decorators.md`](07-curry-partial-and-decorators.md) | curry, `functools.partial`, decorators | curry is free; partial stays opaque; decorators resolve undecorated |
| 8 | [`08-recursion-and-termination.md`](08-recursion-and-termination.md) | direct/mutual recursion | the ancestor guard, applied, with a callback proving it's precise |

Then [`exam.md`](exam.md) for the whole folder.

**Files 1, 5 and 6 are the core.** 1 is the trace you'll replay in your
head for every call site you ever debug; 5 is the single highest-value
addition over a naive port; 6 is the one that shows the design was never
about functions-as-syntax in the first place. **File 4 is the one with a
confirmed gap, now fixed in the plan** — an earlier draft's `closure_parent` is checked
against the real Jedi driver and shown to break the moment a closure is
called from a different frame than the one that defined it (curry, file 7,
inherits the same gap). The plan now uses `Closure { func, activation }`
and `Env.enclosing` instead. Read it for the bug, not just the mechanism.

---

## Read first

- Everything in [`00-values/`](../00-values/README.md) — this folder
  assumes all six files
- `plan/03-call-tree/05-binding-arguments.md`, `07-callbacks-and-higher-order.md`,
  `10-return-values-and-state.md`

---

## Done when

- [ ] you can trace `emit(JsonWriter(), 1)` from `resolve_call` down to the
      leaf, naming every step, without looking at `01`
- [ ] you can state the defaults-evaluate-in-the-defining-scope rule and
      say what silently breaks if you get it backwards
- [ ] you can say, for a call-site splat, why ALL parameters (not just the
      starred one) become `Unknown`
- [ ] you can explain why `inner`'s frame is a child of the call site, not
      of the `def inner:` statement
- [ ] you can explain where `w = make()`'s value comes from (the return
      store the tree walk fills) and why it's there in time (calls before
      bindings, innermost first)
- [ ] you can trace `walk(node)` and say why recursion walked once leaves
      `r.close()` without an edge
- [ ] you can trace a callback through two levels of parameter passing
      using only `env` lookups, no special-casing
- [ ] you can say which of curry / `functools.partial` / decorators
      resolves fully, which stays opaque, and which is deliberately
      imprecise — and why each is the way it is
- [ ] you can draw the tree for `walk(root, visit)` with three levels of
      recursion and get the `on_node_found` node count right

---

→ Start: [`01-calling-a-plain-function.md`](01-calling-a-plain-function.md)
