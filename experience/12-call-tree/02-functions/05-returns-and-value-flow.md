# 01.05 — Returns and value flow

The single most valuable addition over a line-by-line port, and the one
most tempting to skip because `call_resolver.py` has no explicit code for
it. **It's not optional.** This file is the one to read twice.

**Fixture:** `python/returns.py`. **Reading:**
[`plan/03-call-tree/10-return-values-and-state.md`](../../../plan/03-call-tree/10-return-values-and-state.md)
in full.

---

## Why skipping this breaks real code, not edge cases

```python
def build(cfg):
    return JsonWriter() if cfg.json else CsvWriter()

def run(cfg):
    w = build(cfg)        # w is Unknown WITHOUT return flow
    w.write(1)             # no children. tree stops here.
```

This is not exotic — it's factories, builders, `get_handler()`, dependency
injection, service locators: precisely the code where a context-sensitive
call tree earns its keep. A tool that stops at every `x = make_thing()` has
given up on its own reason for existing.

## Walking a call returns its value

This is the idea that makes everything else in this file safe, so get it
straight before anything else:

> **Evaluating a call is resolving it.** `eval` on `make_writer()` walks
> `make_writer`'s body — adding its node and its children — and hands the
> union of every `return` it reached straight back to whoever evaluated the
> call. `w = make_writer()` gets its value from that one evaluation. There
> is no second walk and no call list, so nothing is double-counted.

Python's own evaluation order falls out of it, with nothing to check:

```
eval(emit(make()))
  eval(emit)          → Function(emit)                  1. the function
  eval(make())        → walks make, returns its value   2. the arguments, left to right
  call emit with it   → walks emit                      3. then the call
```

```rust
fn eval_call(&mut self, call: &ast::ExprCall, env: &Env<'db>, mode: &mut Mode<'_, 'db>) -> Values<'db> {
    let callees = self.eval(&call.func, env, mode);                  // 1.
    let args    = self.eval_arguments(&call.arguments, env, mode);   // 2. walks calls inside args
    let mut out = Values::new();
    for callee in callees {                                          // 3.
        out.extend(match callee.as_callable(self.db) {
            // (a) constructor — the ONE place a class is constructed (plan/03-call-tree/10)
            Some(Callable::Class(c)) => {
                let child = self.enter(call, c, mode);
                smallvec![self.construct_instance(c, &args, call, env, child)]
            }
            // (b) ★ project function — walk it; its returns ARE the value
            Some(Callable::Function(f)) if self.is_project_code(f) => {
                let callee_env = self.bind_parameters(f, &args, callee.receiver(), callee.enclosing(), env);
                match self.enter(call, f, mode) {
                    Enter::Walk(child) => self.walk_function(f, &callee_env, child).1,
                    Enter::Cut => self.returns.get(&ReturnKey::of(f, &callee_env))   // recursion, limits
                                      .cloned().unwrap_or(smallvec![AbstractValue::Unknown]),
                }
            }
            // (c) anything else → ty's declared return type
            _ => self.lift(call.inferred_type(mode.model).unwrap_or(Type::unknown())),
        });
    }
    self.cap_fanout(out)
}
```

Three cases, tried in order: **(a)** calling a class constructs — never
asks what the constructor "returns," it just mints an instance. **(b)** ★
calling a project function walks it, with the *same* `bind_parameters`
machinery from `01-calling-a-plain-function.md`, and the value is what its
`return`s produced. **(c)** anything else — stdlib, a function whose body
isn't interpreted — falls back to `ty`'s declared return type via `lift`.

Two rules that keep it right:

- **Every expression is evaluated once per walk.** A second evaluation
  would add a second node for the same call, and the merge on save would
  report `call_count` one too high. `x = y = make()` evaluates `make()` once.
- **The store (`ReturnKey`) is only for reuse and the cut.** A normal call
  gets its value directly. The store is read only when the call is *not*
  walked — a memoised subtree, or a recursion/limit cut — and its key is
  the whole entry state: bound parameters (including `self`), the enclosing
  run, and the attribute state.

## Recursion — walked once, then `Unknown`

There is no separate cycle guard for values. `is_ancestor` (`00-values/05`)
cuts the inner recursive call, exactly as it does for the tree. The inner
call's value read then finds nothing stored — the outer run hasn't
finished — and gets `Unknown`.

```python
def rec(n): return rec(n - 1)
rec(3)          # walked once; the inner rec(n-1) is cut → Unknown; terminates
```

That always terminates, because there's no second traversal that could
loop. The cost is a known gap, accepted for v1:

```python
def walk(node):
    r = walk(node.first())   # inner walk is cut → r = Unknown
    r.close()                 # → no edge, though walk plainly returns Handle
    return Handle()
```

(An earlier version of this design had a second traversal, `eval_returns`,
a value query with its own in-flight memo, specifically to handle this
case. It was dropped: the tree walk already reaches every call a value read
needs, in the right order, and one walker can't disagree with itself. If a
fixture ever shows this gap losing an edge that matters, the fix is the
same walker with node output turned off — never a second walker.)

## Exercise

Using `python/returns.py`, before reading further:

1. `simple_factory()` — what does `w` resolve to, and through which of the
   three `eval_call` cases?
2. `two_returns_union()` — how many values does `build(True).write(1)`'s
   receiver carry, and does the fact that `build(True)` passes a *specific*
   boolean change the answer? (It shouldn't — say why.)
3. `uses_rec()` calls `rec(3)`. Walk it by hand. Where is the recursion
   cut, what does the inner `rec(n - 1)` read from the return store, and
   what does `rec(3)` ultimately evaluate to?
4. `uses_gen()` — why does a function containing `yield` store `Unknown`
   no matter what its `return` statements say? What would go wrong if you unioned the
   yielded values instead of answering `Unknown`?

<details><summary>Answers</summary>

1. Case (b) — `make` is a project function. Evaluating `make()` walks it,
   reaches its single `return JsonWriter()`, and returns
   `Instance{JsonWriter, O}` (origin minted where `JsonWriter()` is
   constructed, inside `make`'s body) straight to the assignment `w = …`.
2. Two values — `Instance{CsvWriter, Oa}` and `Instance{JsonWriter, Ob}` —
   because `build` has **two** `return` statements and there's no branch
   evaluation; both arms are always unioned, regardless of what `True`
   concretely is. (`00-values`'s "no branch analysis" rule: conditions are
   never evaluated, only their consequences are unioned.)
3. The tree walk enters `rec` once (child of `uses_rec`). Inside, the call
   `rec(n - 1)` has `rec` as a tree ancestor, so `is_ancestor` cuts it: no
   child node, no walk. Its value read finds nothing stored under its key
   — the outer `rec` run hasn't finished — so it's `Unknown`. The outer
   run's only `return` is that `Unknown`, so the value of `rec(3)`
   is `Unknown`. Nothing loops: there is only one walk, and it was cut.
4. Because once you know the function contains a `yield`, calling it
   produces a generator object, full stop — the `return` statements (if
   any) are irrelevant to what the *call expression* evaluates to; only
   `next()`/iteration produces the yielded values, which this design
   doesn't model. Unioning the yielded values instead would make `gen()`
   resolve to `Instance{JsonWriter}` directly, and then `for w in gen(): w.write(1)`
   would (by accident) look right for this one fixture while being
   semantically backwards — `w` is never a `JsonWriter`, it's a generator,
   and the *iteration* produces the writer. The fixture passing for the
   wrong reason is exactly the kind of bug that survives until someone
   calls `.send()` or `next()` directly on the generator and gets a
   nonsensical leaf.

</details>

---

→ Next: [`06-callbacks-and-higher-order.md`](06-callbacks-and-higher-order.md)
