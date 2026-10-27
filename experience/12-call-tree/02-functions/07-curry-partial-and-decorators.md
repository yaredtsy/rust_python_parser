# 02.07 — Curry, `functools.partial`, and decorators

Three ways a function hands you back another function, and the same question
each time: can the analysis still see inside it?

**Fixture:** `python/curry.py`. **Reading:**
[`plan/03-call-tree/11-closures-and-callbacks.md`](../../../plan/03-call-tree/11-closures-and-callbacks.md)
§ "Decorators" and § "Out of scope, on purpose".

---

## Curry: resolves fully

```python
def make_adder(n):
    def add(x):
        return x + n
    return add

def use_curry():
    add_one = make_adder(1)
    add_one(5)
```

This is file 5 (a function returns a value) composed with file 4 (the value is a
closure). `make_adder {n: Data(int)}` returns
`Closure { func: add, free: [(n, [Ref(Param(0))])] }`. `use_curry` resolves it:
`add_one` is a closure over `n → Ty(int)`. `add_one(5)` keys
`add { x: Data(int), free: [(n, Data(int))] }`.

It doesn't matter that `add_one` is called from `use_curry` and not from
`make_adder`: the captured values travel with the closure. Curry is the idiom
where this matters most, because its whole point is calling the closure later,
somewhere else.

## `functools.partial`: opaque, on purpose

```python
bound = functools.partial(takes_two, Handler())
bound(5)
```

`functools.partial` is stdlib. The call is `External`, its value is ty's type
for a `partial` object, and `bound(5)` has no project target. `takes_two` is
never reached through it, and the `Handler()` bound into it is never recovered.

That is the Jedi driver's behaviour, kept. If a real project shows it losing
edges that matter, measure how often, then consider a narrow special case for
`functools.partial` alone (treat it like a constructor that records its bound
arguments). Not a general mechanism for opaque callables.

## Decorators: transparent, by rule

```python
def retry(fn):
    def wrapper(*a, **kw): return fn(*a, **kw)
    return wrapper

@retry
def fetch():
    return Handler()
```

Taken literally, `fetch` is `retry(fetch)`, and ty types it as whatever `retry`
returns, which usually loses the function. So:

> **A name bound by a `def` resolves to that `def`'s own function. Decorators
> are transparent.**

`fetch()` keys `fetch`, and the tree shows `fetch`'s body, not `wrapper`'s.
`@app.route`, `@pytest.fixture` and `@lru_cache` are everywhere, and treating
them as opaque would blank out large parts of real projects. It is a deliberate
imprecision, pinned by a fixture.

The binding side has a safety rule: a decorator that might change the signature
(not on a small allow-list such as `lru_cache` and `functools.wraps`-style
wrappers) binds every parameter to `Unknown`, rather than risk a shifted
binding ([`plan/03-call-tree/06`](../../../plan/03-call-tree/06-binding-arguments.md)).

---

## Exercise

1. `use_curry()`: write `make_adder`'s summary and `add`'s key at `add_one(5)`.
2. `use_partial()`: is `Handler.go` reachable? What would have to change for it
   to be?
3. `use_decorated()`: does the tree under `fetch` show `wrapper`'s call to
   `fn`, or `fetch`'s construction of `Handler`?

<details><summary>Answers</summary>

1. `make_adder`: `returns: [Closure { add, free: [(n, [Ref(Param(0))])] }]`.
   `add`'s key: `{ x: Data(int), free: [(n, Data(int))] }`.
2. No: the call is to stdlib, so nothing past it is walked. A special case that
   treats `functools.partial(f, *args)` as a value holding `f` and its bound
   arguments would make `bound(5)` call `takes_two` with them.
3. `fetch`'s own body: a `Handler` construction. Decorators are transparent, so
   `wrapper` is never part of the tree for this call.

</details>

---

→ Next: [`08-recursion.md`](08-recursion.md)
