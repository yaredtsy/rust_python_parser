# 01.07 — Curry, `functools.partial`, and decorators

Three different-looking patterns, and the same underlying question each
time: *this expression evaluated to another function — can I still see
inside it?* The honest answer is "sometimes," and this file is about
knowing which case you're in.

**Fixture:** `python/curry.py`. **Reading:**
[`plan/03-call-tree/07-callbacks-and-higher-order.md`](../../../plan/03-call-tree/07-callbacks-and-higher-order.md)
§"3. Decorators" and §"5. `functools.partial`...".

---

## Curry — resolves fully, for free

```python
def make_adder(n):
    def add(x):
        return x + n
    return add

add_one = make_adder(1)
add_one(5)
```

This needs **no new machinery at all** — it's
[`05-returns-and-value-flow.md`](05-returns-and-value-flow.md) (a function
returning a value) composed with
[`04-closures-and-nested-functions.md`](04-closures-and-nested-functions.md)
(the returned value is itself a nested function, closing over `n`):

```
eval_call_result(make_adder(1))
  → case (b): project function → eval_returns(make_adder, env{n: Ty(int)})
  → the body's one `return add` → eval(Name "add") → env hit (add is a
    local of make_adder, bound by the `def add:` statement itself) →
    AbstractValue::Function(add)
add_one = Function(add)
add_one(5)
  → eval_callee(add_one) → env hit → Function(add)
  → resolve_call descends into add, exactly like any other function call
```

`n` inside `add`'s body resolves through `Env.parent`, same mechanism as
`04`'s `h` — curry is not a new case, it's two cases you already built,
composed.

## `functools.partial` — resolves to nothing, on purpose

```python
bound = functools.partial(takes_two, Handler())
bound(5)
```

`functools.partial` is stdlib — `is_project_code` returns `false` for it —
so the tree stops at `functools.partial(...)` and **the bound argument is
never recovered**. `bound` itself becomes `Ty(<whatever ty infers for a
partial object>)`, almost certainly too opaque to resolve further, and
`bound(5)`'s call simply produces no useful children.

This is **current behaviour, deliberately preserved**, not a gap to fill
reflexively. If a fixture against the real driver ever shows this losing
something that matters on a real project, the right response is to measure
how often, then consider a narrow, explicit special case for
`functools.partial` specifically (treat it like a constructor: capture the
bound positional/keyword args, produce something like
`AbstractValue::Partial { func, bound_args }`) — not a general mechanism for
arbitrary opaque callables.

## Decorators — resolve to the undecorated function, by rule

```python
def retry(fn):
    def wrapper(*a, **kw): return fn(*a, **kw)
    return wrapper

@retry
def fetch():
    return Handler()
```

Structurally, `fetch` now denotes `retry(fetch)` — if you resolved it
*literally*, `fetch()` would mean "call whatever `retry` returned," and
`ty`'s own answer for a decorated function's type is usually the
decorator's declared return type (often a generic `Callable`, sometimes
`Unknown`) — which loses the underlying function entirely.

> **Rule: if a name resolves to a function *definition*, use the
> UNDECORATED function for call-tree purposes.** Decorators are treated as
> transparent.

This is a **deliberate, documented imprecision** — write it down as a
choice, with a fixture pinning it, not as an oversight. Decorated functions
are extremely common in real code (`@app.route`, `@pytest.fixture`,
`@lru_cache`, the `@retry` here); treating every one as opaque would blank
out large parts of a real project's tree. The cost: a decorator that
*actually* changes arity or behaviour (`functools.partial`-like decorators)
makes the undecorated-function assumption produce an incorrect binding —
[`05-binding-arguments.md` in the plan](../../../plan/03-call-tree/05-binding-arguments.md)'s
point #5 ("detect a non-trivial decorator list and fall back to binding
everything `Unknown`") is the safety valve for that case specifically.

## Exercise

Using `python/curry.py`:

1. `use_curry()` — does resolving `add_one(5)` ever touch `ty`'s inference
   at all, or does every step stay inside your own `env`/`eval_returns`
   machinery? Why does that matter for precision?
2. `use_partial()` — what does the tree look like for `bound(5)`? Is
   `Handler.go` reachable from this call at all, under the base design?
3. `use_decorated()` calls `fetch()`. Per the undecorated-function rule,
   does the tree show `wrapper`'s body (the `return fn(*a, **kw)` line) or
   `fetch`'s own body (`return Handler()`)? Justify from the rule, not from
   what feels more "correct" semantically.

<details><summary>Answers</summary>

1. Never touches `ty` — `make_adder`'s return is interpreted directly
   (`eval_returns`), `add_one` lands in `env` as a traced `Function` value,
   and `add_one(5)`'s callee lookup is an `env` hit. This matters because
   it means curry patterns resolve at **full precision** — no union, no
   opaque fallback — purely because every step happens to be something your
   own machinery already tracks precisely.
2. No — the tree stops at `functools.partial(takes_two, Handler())`,
   because `functools.partial` itself is not project code. `bound(5)`
   produces no children; `Handler.go` is not reachable through this call in
   the base design, even though a human reading the source can see that it
   obviously will be invoked at runtime. This is the documented, deliberate
   gap.
3. `fetch`'s own body (`return Handler()`) — the rule says resolve to the
   **undecorated** function for call-tree purposes, so `fetch()` is treated
   as if `@retry` weren't there at all. `wrapper`'s body is never shown as
   part of this call, even though at runtime `wrapper` is what actually
   executes first. This is the imprecision the rule accepts in exchange for
   not blanking out every `@app.route`-decorated handler in a real project.

</details>

---

→ Next: [`08-recursion-and-termination.md`](08-recursion-and-termination.md)
