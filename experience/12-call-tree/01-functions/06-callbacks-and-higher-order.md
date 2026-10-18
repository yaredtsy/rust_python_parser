# 01.06 — Callbacks and higher-order functions

The good news up front: if [`01`](01-calling-a-plain-function.md) and
[`05`](05-returns-and-value-flow.md) are right, this chapter is almost
free. A callback is just a parameter whose bound value happens to be
callable.

**Fixture:** `python/callbacks.py`. **Reading:**
[`plan/03-call-tree/07-callbacks-and-higher-order.md`](../../../plan/03-call-tree/07-callbacks-and-higher-order.md)
in full.

---

## The structural property that makes this free

> **`resolve_call` dispatches on the callee's *value*, not its syntax.** It
> never asks "is this a bare name, or an attribute, or a parameter" — it
> asks "what value is here, and is it callable." Higher-order code is then
> just first-order code where the value happened to arrive via a parameter
> instead of a literal name.

Guard this property specifically — the temptation to write
`if let Expr::Name(n) = call.func { ... }` as a quick special case is
exactly how this generality gets lost, one shortcut at a time.

## The trace, reusing machinery you already have

```python
def on_success(result): audit.record(result)
def run(task, cb): cb(task)
def handle_ok(): run("ok-task", on_success)
```

```
1. eval(on_success)                    — env miss, ty: Type::FunctionLiteral
   → lift → AbstractValue::Function(on_success)
2. bind_parameters binds `cb` ← that value, inside run's callee_env
3. walk_body(run) finds `cb(task)`
4. eval_callee(cb) → env HIT this time → Function(on_success)
5. resolve_call descends into on_success, exactly as in 01-calling-a-plain-function.md
```

No new code anywhere in this trace — every step is a step you already
wrote. If this *doesn't* work on the first try once 01 and 05 are correct,
the bug is in one of those two files, not here.

## Why the two-caller shape, again

`handle_ok()` and `handle_err()` each call `run` with a *different*
callback, from *different* frames. Two `run` nodes, one correct leaf each —
and this is deliberately the same emphasis as
[`01-calling-a-plain-function.md`](01-calling-a-plain-function.md)'s
cross-scope fixture: one frame calling `run` twice with two callbacks would
merge into a single `run` node (`00-values/05`'s merge rule) with both
leaves as siblings inside it — still correct, but it wouldn't demonstrate
that the *callback choice* is what's being tracked per-path.

## The three variants worth a specific look

**Bound methods as callbacks** —
`run(fetch, self.handler.on_done)` — covered properly once you've done
[`02-classes/07-bound-methods-chains-and-mro-dispatch.md`](../02-classes/07-bound-methods-chains-and-mro-dispatch.md),
because `eval` of an `Attribute` goes through `member()`, producing a
`BoundMethod{func, receiver}`. The receiver **must** survive the trip
through `run`'s parameter — which is exactly why `BoundMethod` interns its
receiver as a `ValueId` (`00-values/02`) rather than re-deriving it later:
by the time `cb()` actually runs, the expression `self.handler.on_done` is
long gone from view.

**Decorated functions** — see
[`07-curry-partial-and-decorators.md`](07-curry-partial-and-decorators.md).

**Functions stored in containers** — `HANDLERS[kind](data)` — out of scope.
Containers aren't modelled (`00-values/01`'s domain), so `HANDLERS[kind]`
resolves to `Unknown`, with no children. If a fixture against the real
driver shows Jedi occasionally resolving a constant-key dict literal,
that's a measured, deliberate extension — not a gap you default to closing.

## Exercise

Using `python/callbacks.py`:

1. Trace `main()`'s tree by hand: how many `run` nodes, and what's the one
   leaf under each?
2. `call_through_levels()` passes `on_success` through **two** parameter
   hops (`through_two_levels(cb)` → `middle(cb)` → `cb("value")`) before it's
   invoked. At which point does `cb` actually get looked up in `env`, and
   is that lookup any different in kind from the direct case in `main()`?
3. Why is "a callback is just a parameter whose value happens to be
   callable" exactly the same idea as fixture #2 from
   [`00-values/02-origin-and-identity.md`](../00-values/02-origin-and-identity.md)
   (the aliasing exercise)?

<details><summary>Answers</summary>

1. Two `run` nodes: `main → handle_ok → run → on_success → audit.record`
   and `main → handle_err → run → on_failure → alert.page`.
2. `cb` is looked up inside `middle`'s body, at the point `cb("value")` is
   evaluated — `eval_callee` checks `middle`'s own `env.locals`, finds `cb`
   bound there (because `bind_parameters` eagerly copied the value in when
   `middle(cb)` was called from `through_two_levels`, which itself got it
   the same way from `call_through_levels`). It's the *identical* lookup
   mechanism as the direct case — `env.lookup("cb")` — just resolving a
   name that was threaded through two hops of eager parameter binding
   rather than bound once. No special "look further up the call chain"
   logic exists or is needed.
3. Both are instances of the same underlying fact: a name binding (whether
   a parameter binding or a plain `y = x` alias) doesn't create a *new*
   value, it just gives an *existing* value a new name to be found under.
   `cb` bound to `on_success` is structurally the same operation as `y`
   bound to the same `Instance` as `x` — in both cases, the thing that
   matters (callability in one case, origin identity in the other) rides
   along unchanged through the rebinding.

</details>

---

→ Next: [`07-curry-partial-and-decorators.md`](07-curry-partial-and-decorators.md)
