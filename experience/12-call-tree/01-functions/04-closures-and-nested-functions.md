# 01.04 — Closures and nested functions

What `Env.parent` is actually for, and the one rule that decides which
scope a nested call belongs to.

**Fixture:** `python/closures.py`. **Reading:**
[`plan/03-call-tree/05-binding-arguments.md`](../../../plan/03-call-tree/05-binding-arguments.md)
§"the environment", and
[`00-values/03-env-and-scope.md`](../00-values/03-env-and-scope.md)'s
`parent` field.

---

## The rule a nested `def` follows

```python
def outer():
    h = Handler()
    def inner():
        h.go()
    inner()
```

Two separate questions, easy to tangle together:

1. **Whose frame does `h.go()` belong to?** `inner`'s — not `outer`'s. A
   `def` statement, when *evaluated* (not called), binds a function value
   and does **not** execute its body — this is the "one rule, three
   spellings" idea from `experience/02-parse-and-ast/05-object-visitor.md`:
   `enter_node`/`Skip` at parse time, "not calling the walk" at def-collection
   time, and here, at interpretation time, simply: `inner`'s body is only
   walked when `inner` is **called**, which happens on the line `inner()`,
   producing a frame that is a child of `outer`'s frame (because that's
   where the call site is) — never a frame that's a child of the `def inner`
   *statement*.
2. **How does `inner` see `h`, which is `outer`'s local, not its own
   parameter?** This is what `Env.parent` is for.

## `Env.parent` — the closure chain

```rust
pub struct Env<'db> {
    locals: rpds::HashTrieMap<Name, Values<'db>>,
    attrs:  rpds::HashTrieMap<(OriginId, Name), Values<'db>>,
    scope:  ScopeId<'db>,
    parent: Option<Arc<Env<'db>>>,     // ★ this file
}
```

When `inner` is called, its `callee_env` is built the same way any call's
is (`01-calling-a-plain-function.md`'s trace) — except `bind_parameters`
sets `parent` to the `Env` that was active **where `inner` was defined**,
not where it was called from. Looking up `h` inside `inner`'s body:

```
env.lookup("h")
  → not in inner's own `locals` (inner has no parameter named h)
  → walk `parent` — the env active at `def inner`'s own definition site
  → found: h → Instance{Handler, O1}
```

**Critically, this is a search through `parent` specifically because `h`
is a genuine free variable of a nested function** — not a generic "if you
don't find it here, keep looking outward" fallback for every miss.
`00-values/03`'s scope guard exists precisely to stop an unrelated `x` in
some other function from being found this way. Compare: if `inner` were
simply called with `h` passed as a parameter (`inner(h)`, ordinary
pass-through, the case from the whole `outer→middle→inner` example running
through this entire design), you'd never touch `parent` at all — the value
would already be sitting in `inner`'s own `locals`, bound eagerly at the
call site. `Env.parent` exists only for the narrower case: a name read
directly, with no intervening parameter.

## Lambdas — pin the current behaviour before extending it

```python
run = lambda: Handler().go()
run()
```

parso's `Lambda` is a `Function` subclass, and the def-collecting scanner's
early-return rule (`experience/02-parse-and-ast/05-object-visitor.md`'s
`Skip`-at-nested-def behaviour) means lambda **bodies never show up as
project defs** in the node tree. Whether the **call resolver** — a
different traversal — also refuses to descend into a lambda's body is a
separate, explicit decision:

> **Match current behaviour first.** Check what `_analyze_function`'s
> `collect_call_node` actually does with a `lambdef` node against the real
> Python driver, write the answer down as a fixture, then decide whether
> `AbstractValue::Lambda(ExprLambda)` (treating the body as a one-expression
> function) is worth the ~30 lines it costs. Don't guess; verify against
> the driver and pin it.

## Exercise

1. In `outer()`, is `inner`'s frame a child of `outer`'s frame, or a
   sibling at the module's top level? Justify from the "a `def` doesn't
   execute its body" rule.
2. Suppose `inner` took a parameter with the same name as `outer`'s local —
   `def inner(h): h.go()`, called as `inner(SomeOtherHandler())`. Does
   `env.lookup("h")` inside `inner` ever reach `outer`'s `h` via `parent`?
   Why or why not?
3. Write down, in one sentence each, what you'd need to predict before
   running the Python driver on `with_lambda()`, and what you'd check
   after running it to confirm your prediction.

<details><summary>Answers</summary>

1. A child of `outer`'s frame — `inner()` is a call site that appears
   textually inside `outer`'s body, so when `walk_body(outer, ...)` finds
   it and recurses, the resulting `resolve_call` adds `inner` as a child of
   *whichever frame `outer` is currently executing in* (`outer`'s own
   frame, in the simplest case). The `def inner:` statement itself produces
   no tree node at all — only the call does.
2. No — `inner`'s own parameter `h` shadows the free variable entirely.
   `env.lookup("h")` finds a hit in `inner`'s own `locals` (bound to
   `SomeOtherHandler()` from the call site) before ever considering
   `parent`. This is ordinary lexical shadowing, and it falls out for free
   because `locals` is always checked before `parent`.
3. Predict: does `Handler().go()` inside the lambda appear anywhere in the
   call tree produced for `with_lambda`, or does the tree stop at
   `with_lambda → run` with no further children? Check after running:
   whether `run()`'s frame has a `go` child, which tells you whether the
   real driver's `collect_call_node` actually descends into `lambdef`
   nodes or skips them like it skips nested `def`/`class`.

</details>

---

→ Next: [`05-returns-and-value-flow.md`](05-returns-and-value-flow.md)
