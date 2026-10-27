# 02.01 — Constructing an instance

The class-side base case, mirroring
[`01-functions/01-calling-a-plain-function.md`](../01-functions/01-calling-a-plain-function.md)
exactly — the only new idea is that calling a class doesn't run the class's
code, it mints a value and then calls `__init__` *as an ordinary function
call* against that value.

**Fixture:** `python/construct.py`. **Reading:**
[`plan/03-call-tree/03-the-abstract-interpreter.md`](../../../plan/03-call-tree/03-the-abstract-interpreter.md)
§"The main loop", the `Callable::Class` arm.

---

## The shape

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler
    def run(self, data):
        self.handler.process(data)

Pipeline(JsonHandler()).run(1)
```

Two calls here, chained: `Pipeline(JsonHandler())` constructs, `.run(1)`
calls a method on the result. This file is about the first half only —
[`02-self-and-method-binding.md`](02-self-and-method-binding.md) picks up
the second.

## The trace

```
resolve_call(Pipeline(JsonHandler()), env, frame)
  1. eval_callee(Pipeline) → env miss → ty: Type::ClassLiteral(Pipeline)
     → lift → AbstractValue::Class(Pipeline)
  2. eval_arguments((JsonHandler(),)) → [Instance{JsonHandler, O1}]
  3. target = Pipeline → Callable::Class(Pipeline)
     qname, docstring id, is_ancestor checks — same as any function target
     child = frame.add_child("Pipeline", "ClassSchema/<id>")
  4. lookup_init(Pipeline) → finds `__init__`
  5. instance = construct_instance(Pipeline, &args, call, env)
                 — interns OriginKey{file, range, Pipeline, args}; a new key
                   gets a FRESH OriginId and runs __init__'s self.X scan once
  6. init_env = bind_parameters(
         __init__, args, receiver = Some(instance)   // ★ `self` ← the NEW instance
     )
  7. walk_body(__init__, init_env, child)
       finds `self.handler = handler`
       → this is an ASSIGNMENT to an Attribute target, not a call —
         02-classes/03 picks this up; nothing to resolve here
```

Compare this to calling a plain function: steps 1–3 are **identical** to
[`01-functions/01`](../01-functions/01-calling-a-plain-function.md)'s trace
— the class-vs-function branch only appears at step 4 onward. Constructing
is not a special kind of call; it's the same `resolve_call` entry point,
dispatching on what kind of `Callable` the target turned out to be.

## Why `__init__`'s body runs through the exact same `walk_body`

There is no separate "constructor interpreter." `__init__` is a function
like any other — `bind_parameters` binds its `self` parameter to the fresh
instance (receiver binding, covered properly in
[`02-self-and-method-binding.md`](02-self-and-method-binding.md)), and
`walk_body` walks it exactly like it would walk `emit`'s body in
`01-functions/01`. The only genuinely new piece of machinery anywhere in
this folder is the **attribute store** — recording what `self.handler =
...` wrote, so a *different* method can read it back later. That's next.

## Exercise

Using `python/construct.py`:

1. At what exact point in the trace is the fresh `OriginId` minted? Could
   it be minted any earlier (e.g. before `__init__`'s args are bound) or
   later (e.g. after `__init__`'s body finishes)? What would break in each
   case?
2. Does `Pipeline`'s own class body (not `__init__`, just the class
   statement) ever get "called" or "walked" as part of constructing an
   instance? Compare to how `def` statements for plain functions are
   handled in [`01-functions/04`](../01-functions/04-closures-and-nested-functions.md).
3. `main()` resolves `Pipeline(JsonHandler())` first, then `.run(1)` on the
   result. From this file's trace alone, can you tell what `self.handler`
   is equal to inside `run`? Why not — what's still missing?

<details><summary>Answers</summary>

1. At step 5, right before `__init__`'s env is built — the instance has to
   exist before it can be bound to `self`. Minting it earlier (before
   arguments are evaluated) would be harmless but pointless; minting it
   *later* (after `__init__`'s body runs) would be a real bug — any
   `self.handler = handler` write inside `__init__` needs a concrete
   `OriginId` to key the attribute store entry on, and there isn't one
   until the instance exists.
2. No — the class **statement** itself (the `class Pipeline:` block) binds
   a class value once, when the module is loaded, exactly the way a `def`
   statement binds a function value without running its body
   (`01-functions/04`'s "one rule, three spellings"). Only `__init__`'s
   *body* gets walked, and only when a construction call actually happens.
3. No — this file's trace stops at `__init__`'s body being walked; it
   doesn't show *what* gets recorded when `self.handler = handler` runs.
   That's exactly [`03-attribute-state-after-init.md`](03-attribute-state-after-init.md)'s
   subject — this file only gets you to the point where the write is
   *about* to happen.

</details>

---

→ Next: [`02-self-and-method-binding.md`](02-self-and-method-binding.md)
