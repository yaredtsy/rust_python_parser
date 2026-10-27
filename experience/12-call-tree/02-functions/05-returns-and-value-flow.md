# 02.05 — Returns and value flow

What a line-by-line port of `call_resolver.py` would lose, because the driver
has no explicit code for it: Jedi does it inside the context the driver builds.
**Not optional.**

**Fixture:** `python/returns.py`. **Reading:**
[`plan/03-call-tree/09-summaries.md`](../../../plan/03-call-tree/09-summaries.md),
[`10-one-call.md`](../../../plan/03-call-tree/10-one-call.md).

---

## Why it matters

```python
def build(cfg):
    return JsonWriter() if cfg.json else CsvWriter()

def run(cfg):
    w = build(cfg)        # without return flow: w is Unknown
    w.write(1)            # no edge. The tree stops here.
```

Factories, builders, `get_handler()`, dependency injection, service locators:
exactly the code where a call tree is worth having. A tool that stops at every
`x = make_thing()` has given up on its reason to exist.

## Returns are part of the summary

There is no separate "what does this return?" pass. The callee's walk collects
every `return` it reaches, and the summary carries them, relative to the inputs:

| function | `returns` |
|---|---|
| `def make(): return JsonWriter()` | `[Obj(0)]`, with `objects[0]` a `JsonWriter` |
| `def build(c): return JsonWriter() if c else CsvWriter()` | `[Obj(0), Obj(1)]`: both, conditions are never evaluated |
| `def ident(x): return x` | `[Ref(Param(0))]` |
| `def outer(k): return build(k)` | `[Obj(0), Obj(1)]`, the objects `build` returned, now `outer`'s to export |
| `def nothing(): pass` | `[Plain(Ty(None))]`: falling off the end returns `None` |

In the caller, `eval` on the call gets the summary, resolves the returns
against its snapshot ([`01-summaries/04`](../01-summaries/04-one-call.md)), and
hands the values to whoever evaluated the call: the assignment `w = make()`, the
argument in `emit(make())`, the receiver in `make().run()`.

## Python's order falls out for free

```
eval(emit(make()))
  eval(emit)          → Function(emit)                       1. the function
  eval(make())        → make's summary, returns resolved     2. the arguments, left to right
  call emit with it   → emit's summary                       3. then the call
```

Each argument, calls included, is fully evaluated before the outer call. There's
nothing to check.

**Every expression is evaluated exactly once per walk.** `x = y = make()`
evaluates `make()` once: one call site, one edge.

## Special cases

- **Generators.** A function containing `yield` returns a generator, not the
  yielded values. Its summary returns `[Plain(Unknown)]`; the body is still
  walked. Unioning the yields would make `for w in gen(): w.write(1)` look right
  by accident, while `gen()` itself would wrongly be a `JsonWriter`.
- **`await`.** Transparent: `await afetch()` is the value of `afetch()`.
- **A function that always raises.** Its returns are empty. A call that passes
  that empty value on is still shown, but can't run
  ([`01-summaries/04`](../01-summaries/04-one-call.md#a-call-with-no-value)).
- **Recursion.** `def rec(n): return rec(n - 1)` — the value of the inner call
  isn't known while `rec` is being walked. That's [`08-recursion.md`](08-recursion.md).

---

## Exercise

Using `python/returns.py`:

1. `simple_factory()`: what is `w`, and where does its origin come from?
2. `two_returns_union()`: `build(True).write(1)` — how many values does the
   receiver have? Does `True` change it?
3. `chained_returns()`: write `outer`'s summary.
4. `uses_gen()`: why is `w.write(1)` without an edge, and why is that right?
5. `emit(make())` from one function: how many contexts of `make` are walked if
   ten other functions also call `make()`?

<details><summary>Answers</summary>

1. `Instance{JsonWriter, New{site of make(), t: 0, combo: 0, made: Obj(0)}}`:
   `make`'s summary returns `Obj(0)`, and the caller gives it an origin tied to
   its own call site.
2. Two: a `JsonWriter` and a `CsvWriter`. `True` changes nothing: conditions are
   never evaluated. Both writes get edges.
3. `returns: [Obj(0), Obj(1)]`, `objects: [JsonWriter, CsvWriter]`: `build`'s two
   objects, re-exported by `outer`.
4. `gen()` returns `Unknown` (a generator), and iterating it isn't modelled, so
   `w` is `Unknown`. Right, because `gen()` really returns a generator; the
   writer only appears through iteration.
5. One. `make` has no parameters, so every call has the same key, and the
   summary is reused.

</details>

---

→ Next: [`06-callbacks-and-higher-order.md`](06-callbacks-and-higher-order.md)
