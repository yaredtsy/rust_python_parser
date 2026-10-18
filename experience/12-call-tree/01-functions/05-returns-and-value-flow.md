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

## Two traversals of the same function — keep them separate

This is the idea that makes everything else in this file safe, so get it
straight before anything else:

> **`resolve_call` (the tree walk) and `eval_returns` (a value query) are
> two independent passes over the same function body.** The call
> `make_writer()` becomes a child node of the current frame via the
> ordinary `resolve_call` path — that's unrelated to, and happens
> regardless of, whatever `eval_returns` concludes `make_writer()`'s *return
> value* is. Conflating them double-counts every factory in your tree.

```rust
fn eval_call_result(&mut self, call: &ast::ExprCall, env: &Env<'db>, model) -> Values<'db> {
    let callees = self.eval_callee(&call.func, env, model);
    let args = self.eval_arguments(&call.arguments, env, model);
    let mut out = Values::new();
    for callee in callees {
        match callee.as_callable(self.db) {
            Some(Callable::Class(c)) => {                     // (a) constructor
                out.push(AbstractValue::Instance { class: c, origin: self.fresh_origin(call, env.path_key()) });
            }
            Some(Callable::Function(f)) if self.is_project_code(f) => {  // (b) ★ interpret its returns
                let callee_env = self.bind_parameters(f, &args, callee.receiver(), env);
                out.extend(self.eval_returns(f, &callee_env));
            }
            _ => out.extend(lift(self.db, self.env(), call.inferred_type(model).unwrap_or(Type::unknown()))),  // (c)
        }
    }
    self.cap_fanout(out)
}
```

Three cases, tried in order: **(a)** calling a class constructs — never
asks what the constructor "returns," it just mints an instance. **(b)** ★
calling a project function interprets its `return` statements, using the
*same* `bind_parameters` machinery from `01-calling-a-plain-function.md`.
**(c)** anything else — stdlib, a function whose body you've decided not to
interpret — falls back to `ty`'s declared return type via `lift`.

## The in-flight memo — a different cycle guard, on purpose

```rust
enum Memo<'db> { InFlight, Done(Values<'db>) }

fn eval_returns(&mut self, func: FunctionDef<'db>, env: &Env<'db>) -> Values<'db> {
    let key = (func, fingerprint(env.locals()));
    match self.return_memo.get(&key) {
        Some(Memo::InFlight) => return smallvec![AbstractValue::Unknown],   // ★ cycle, give up HERE
        Some(Memo::Done(v)) => return v.clone(),
        None => {}
    }
    self.return_memo.insert(key, Memo::InFlight);
    let mut out = Values::new();
    for ret in return_statements_in_scope(func) {
        out.extend(match &ret.value {
            Some(e) => self.eval(e, env, model),
            None    => smallvec![AbstractValue::Unknown],
        });
    }
    let out = self.cap_fanout(out);
    self.return_memo.insert(key, Memo::Done(out.clone()));
    out
}
```

Why not just reuse `is_ancestor` (`00-values/05`) here? Because it answers
a different question, and reusing it breaks in **both** directions:

| | `is_ancestor` (tree walk) | in-flight memo (value query) |
|---|---|---|
| Purpose | output semantics — defines the tree's shape | termination — an implementation detail |
| Observable in the JSON? | yes | no |
| Free to change? | no, it's the contract | yes, any correct mechanism works |

```python
def f(): return g()
def g(): return f()
def main():
    x = f()     # a VALUE query — pushes no tree frame at all
    x.run()     # is_ancestor never fires: infinite recursion if reused here
```

```python
def walk(node):
    r = walk(node.first())   # a value query on walk, while...
    r.close()                 # ...`walk` IS a tree ancestor right now
    return Handle()
# is_ancestor-as-reused-here WOULD fire, return Unknown, and r.close()
# dead-ends — even though walk's return is plainly Handle.
```

The in-flight memo gets both right because it tracks **exactly** the
recursion that matters for value queries — "am I already computing this
exact `(func, env)` pair?" — independent of whatever the tree's own frame
stack happens to be doing at the same moment.

## Exercise

Using `python/returns.py`, before reading further:

1. `simple_factory()` — what does `w` resolve to, and through which of the
   three `eval_call_result` cases?
2. `two_returns_union()` — how many values does `build(True).write(1)`'s
   receiver carry, and does the fact that `build(True)` passes a *specific*
   boolean change the answer? (It shouldn't — say why.)
3. `uses_rec()` calls `rec(3)`. Walk `eval_returns(rec, ...)` by hand. At
   what point does the in-flight memo return `Unknown`, and what does
   `rec(3)` ultimately evaluate to?
4. `uses_gen()` — why must detecting `yield` come **before** evaluating any
   `return` statements in the body? What would go wrong if you unioned the
   yielded values instead of answering `Unknown`?

<details><summary>Answers</summary>

1. Case (b) — `make` is a project function, so its single `return
   JsonWriter()` is interpreted: `w → Instance{JsonWriter, O}`, with a fresh
   origin minted where `JsonWriter()` is actually constructed, inside
   `make`'s body.
2. Two values — `Instance{CsvWriter, Oa}` and `Instance{JsonWriter, Ob}` —
   because `build` has **two** `return` statements and there's no branch
   evaluation; both arms are always unioned, regardless of what `True`
   concretely is. (`00-values`'s "no branch analysis" rule: conditions are
   never evaluated, only their consequences are unioned.)
3. `eval_returns(rec, env₀)` (n=3) isn't yet in the memo → marked
   `InFlight` → evaluates `return rec(n-1)` → calls `eval_returns(rec,
   env₁)` (n=2, a *different* key, different fingerprint) → ... down to
   some n where the fingerprint repeats (or `MAX_RETURN_DEPTH` fires first)
   → that call finds `InFlight` already recorded for a matching key →
   returns `Unknown` immediately. That `Unknown` propagates back up through
   every enclosing `eval_returns` call, so `rec(3)` ultimately evaluates to
   `Unknown` — and nothing loops forever.
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
