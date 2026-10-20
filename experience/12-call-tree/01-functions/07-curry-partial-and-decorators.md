# 01.07 — Curry, `functools.partial`, and decorators

Three different-looking patterns, and the same underlying question each
time: *this expression evaluated to another function — can I still see
inside it?* The honest answer is "sometimes," and this file is about
knowing which case you're in.

**Fixture:** `python/curry.py`. **Reading:**
[`plan/03-call-tree/07-callbacks-and-higher-order.md`](../../../plan/03-call-tree/07-callbacks-and-higher-order.md)
§"3. Decorators" and §"5. `functools.partial`...".

---

## Curry — the escaping-closure case, not a free pass

```python
def make_adder(n):
    def add(x):
        return x + n
    return add

add_one = make_adder(1)
add_one(5)
```

This *looks* like it should need no new machinery — just
[`05-returns-and-value-flow.md`](05-returns-and-value-flow.md) (a function
returning a value) composed with
[`04-closures-and-nested-functions.md`](04-closures-and-nested-functions.md)
(the returned value is itself a nested function, closing over `n`). That
was this file's original claim. **It's wrong, for the exact reason
`04`'s "escaping-closure gap" section exists** — and this fixture is a
sharper example of it than `04`'s own, because curry's whole *point* is
assigning the closure to a name and calling it **later, from somewhere
else**. `add_one(5)` runs inside `use_curry`'s frame — a different frame
from `make_adder`'s, which is the one that actually bound `n`.

```
eval_call_result(make_adder(1))
  → case (b): project function → eval_returns(make_adder, env{n: Ty(int)})
  → the body's one `return add` → eval(Name "add")
    → NOT an env hit — record_bindings has no arm for `def add:`, so
      nothing in env.locals was ever set for the name "add"
    → falls to the ty-fallback: ty's own lexical scoping finds the nested
      def regardless of our env → Type::FunctionLiteral(add) → lift
    → AbstractValue::Function(add)        — a BARE handle, no captured env
add_one = Function(add)
add_one(5)
  → eval_callee(add_one) → env hit → Function(add)
  → bind_parameters(add, args, None, caller_env = use_curry's env)
  → parent: closure_parent(add, caller_env)
    → caller_env is use_curry's env, which never had `n` in it —
      `n` was a parameter of make_adder, a call that already returned
  → inside add's body, env.lookup("n") → MISSES, both locals and parent
```

Under the base design, **`add_one(5)`'s `x + n` cannot resolve `n`
correctly** — `n` falls through to whatever `ty` infers for a bare,
unbound name in that position, which is not going to be `1`. This needs
exactly the fix `04` sketches: bind `add` to
`Closure { func: add, captured: EnvId }` at the point the `def add:`
statement is walked (while `n` is still in `make_adder`'s own `locals`),
and use that captured env — not whichever frame `add_one` is later called
from — as `add`'s `parent` every time it's invoked.

With that fix in place, curry resolves at full precision, and *then* the
original framing holds: it's `05` and `04` composed, no new case. Without
it, curry is the fixture that makes the escaping-closure gap impossible to
ignore, because it's not an edge case here — it's the entire idiom.

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

1. `use_curry()` — under the **unfixed** base design, does `add_one(5)`
   resolve `n` correctly? Walk the trace above and say exactly where it
   goes wrong. Then say what changes once the `Closure`/`EnvId` fix from
   `04` is in place.
2. `use_partial()` — what does the tree look like for `bound(5)`? Is
   `Handler.go` reachable from this call at all, under the base design?
3. `use_decorated()` calls `fetch()`. Per the undecorated-function rule,
   does the tree show `wrapper`'s body (the `return fn(*a, **kw)` line) or
   `fetch`'s own body (`return Handler()`)? Justify from the rule, not from
   what feels more "correct" semantically.

<details><summary>Answers</summary>

1. No — it goes wrong at the point `add`'s body looks up `n`.
   `bind_parameters(add, args, None, caller_env)` is called with
   `caller_env = use_curry`'s env (wherever `add_one(5)` textually is),
   not `make_adder`'s env (where `n` was actually bound and had already
   returned by this point). `closure_parent(add, caller_env)` has no way
   to recover `make_adder`'s env from that input, so `env.lookup("n")`
   inside `add` misses both `locals` and `parent`, and falls through to
   whatever `ty` infers for a bare name in that position — not `1`. Once
   the `Closure { func, captured: EnvId }` fix is in place, `add` is bound
   to that closure (with `n` captured) at the point `def add:` is walked
   inside `make_adder`, and `add_one(5)` uses the *captured* env
   regardless of which frame it's called from — resolving to `1 + 5`
   precisely, with no union and no opaque fallback.
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
