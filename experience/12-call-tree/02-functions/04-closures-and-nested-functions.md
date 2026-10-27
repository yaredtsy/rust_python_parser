# 01.04 — Closures and nested functions

What `Env.enclosing` is for, the one rule that decides which scope a nested
call belongs to — and a gap in an earlier draft's `closure_parent` that a
same-frame example hides and a cross-scope one exposes.

**Fixture:** `python/closures.py`. **Reading:**
[`plan/03-call-tree/01-what-jedi-actually-does.md`](../../../plan/03-call-tree/01-what-jedi-actually-does.md)
§"Present via Jedi, not via the resolver" (closures are now a named row
there),
[`plan/03-call-tree/05-binding-arguments.md`](../../../plan/03-call-tree/05-binding-arguments.md)
§"the environment", and
[`00-values/03-env-and-scope.md`](../00-values/03-env-and-scope.md)'s
`enclosing` field.

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
   parameter?** This is what `Env.enclosing` is for.

## `Env.enclosing` — the closure chain

```rust
pub struct Env<'db> {
    locals: rpds::HashTrieMap<Name, Values<'db>>,
    attrs:  rpds::HashTrieMap<(OriginId, Name), Values<'db>>,
    scope:  ScopeId<'db>,
    enclosing: Option<ActivationId>,   // ★ this file — the DEFINING run, never the caller
}
```

**What `enclosing` does**: when `inner` is called, its
`callee_env` is built the same way any call's is
(`01-calling-a-plain-function.md`'s trace), except `bind_parameters` sets
`enclosing` to the `Env` that was active **where `inner` was defined**, not
where it was called from. (The name is the point: the caller is a different
link, `Frame.parent` in the call tree, and lookup never follows it —
`00-values/03`.) Looking up `h` inside `inner`'s body:

```
env.lookup("h")
  → not in inner's own `locals` (inner has no parameter named h)
  → walk `enclosing` — the env active at `def inner`'s own definition site
  → found: h → Instance{Handler, O1}
```

**Critically, this is meant to be a search through `enclosing` specifically
because `h` is a genuine free variable of a nested function** — not a
generic "if you don't find it here, keep looking outward" fallback for
every miss. `00-values/03`'s scope guard exists precisely to stop an
unrelated `x` in some other function from being found this way. Compare:
if `inner` were simply called with `h` passed as a parameter (`inner(h)`,
ordinary pass-through, the case from the whole `outer→middle→inner`
example running through this entire design), you'd never touch `enclosing`
at all — the value would already be sitting in `inner`'s own `locals`,
bound eagerly at the call site. `Env.enclosing` exists only for the narrower
case: a name read directly, with no intervening parameter.

That's the intent. **In this `outer`/`inner` example specifically, it also
happens to be what you get** — but, as the next section shows, that's a
coincidence of this one fixture, not a property of the mechanism as
specified.

## ⚠ The escaping-closure gap — confirmed, and now fixed in the plan

This section traces an **earlier** version of `plan/03-call-tree/05`, which
built the field (then called `parent`) as `closure_parent(func, caller_env)`.
That bug is why the plan now uses `Closure { func, activation }` and the name
`enclosing`. The trace is kept because it shows how a same-frame fixture
hides the bug.

Change exactly one thing about the fixture above: call `inner` from
somewhere other than directly inside `outer`.

```python
def make_handler(writer):
    def handle(data):
        writer.write(data)      # ← writer: a free variable of handle
    return handle

def read_json():
    h = make_handler(JsonWriter())
    h(1)                         # ← called HERE, inside read_json's frame

def read_csv():
    h = make_handler(CsvWriter())
    h(2)
```

Run this the way `call_resolver.py` actually runs (infer the callee, build
`as_context(arguments)`, resolve inside it — `jedi.settings.dynamic_params`
off, so you're exercising the closure mechanism itself, not Jedi's
heuristic cross-call-site guesswork) and Jedi gets this right:
`read_json`'s `handle` resolves to `JsonWriter.write`, `read_csv`'s to
`CsvWriter.write`. **[verified]** — this is now its own row in
`plan/03-call-tree/01`'s "Present via Jedi" list, alongside return-value
flow and attribute state — the same `as_context` mechanism, a third place
it shows up.

Trace the earlier design against this fixture and it breaks, for three
compounding reasons:

**1. `closure_parent(func, caller_env)` only ever receives the *caller's*
env.** Its line in the earlier draft's `bind_parameters` was
`parent: self.closure_parent(func, caller_env)` — and `caller_env` is
`bind_parameters`'s own fourth parameter: the env of *whoever is calling
`func` right now*. When `h(1)` runs, that's `read_json`'s env — which
never had `writer` in it at all. `read_json`'s env isn't "close enough and
missing one level" — it is a completely different function's frame.
`closure_parent` is never given anything that could tell it "walk back to
`make_handler`'s activation, not `read_json`'s."

**2. `record_bindings` has no arm for a nested `def`.** Its match arms
(`plan/03-call-tree/10`) are `Assign`, `AnnAssign`, `With`, `For`, and a
catch-all `_ => env`. `def handle(data): ...` falls into that catch-all —
nothing records *where* `handle` was defined, or what its enclosing env
looked like at that moment.

**3. `AbstractValue::Function` has nowhere to put a captured env even if
(2) were fixed.** Look back at the enum in
[`00-values/01-abstractvalue.md`](../00-values/01-abstractvalue.md):
`Function(Definition<'db>)` is a bare salsa handle. `return handle`
still resolves to *some* `Function` value — via the ordinary `ty`-fallback,
since `ty`'s own lexical scoping finds the nested def regardless of our
`env` — but that value carries no information about which `Env` was active
when the `def` statement ran. There's nothing to attach, even in
principle, to a plain `Function(handle)`.

**Why the `outer`/`inner` fixture above didn't catch this:** `inner` is
*defined and called in the same frame* — `caller_env` at the point
`inner()` runs is `outer`'s own, current env, which still has `h` in its
`locals` (nothing left that frame between definition and call). So
`closure_parent(inner, caller_env=outer's env)` happens to land on exactly
the right env, **without the mechanism actually doing anything
closure-aware at all** — it would produce the same result for *any* two
pieces of code sharing one frame, closure or not. The `make_handler`
fixture is the cross-scope version of the same principle used everywhere
else in this exercise (`01-calling-a-plain-function.md`'s `read_json`/
`read_csv`, `06-callbacks-and-higher-order.md`'s two callers): a same-frame
example cannot distinguish "the mechanism is correct" from "the mechanism
is unused and something else happened to agree with it."

### The fix — now in the plan

It falls directly out of the three causes above and fixes all three at
once. `plan/03-call-tree/04` (the `Closure` variant), `05` (`enclosing`,
lookup) and `10` (the `def` arm of `exec_simple`) now specify it:

```rust
AbstractValue::Closure { func: Definition<'db>, activation: ActivationId }
```

Every run of a function body — every walk of it — gets an
`ActivationId` and a slot holding that run's **current** env, updated after
each statement. When the walk passes `def handle(data): ...`, it binds
`handle` to `Closure { func, activation: <this run> }`. When that value is
later called — from any frame — `bind_parameters` sets the callee's
`enclosing` to that run, and looking up `writer` reads the run's slot.

Two things this gets right that a frozen copy of the env (an earlier
`captured: EnvId` version of this fix) didn't:

- **Later assignments.** `h = None; def run(): h.go(); h = JsonHandler();
  return run` — the slot holds `JsonHandler` by the time `run` is called,
  as in Python. A copy taken at the `def` would still say `None`.
- **Recursion.** A nested `def walk(n): walk(n.child)` finds `walk` in the
  slot. A copy taken just before binding `walk` didn't contain it.

Why per *run* and not per tree frame: two calls to `make_handler` from the
same function share one tree frame (frames merge by `(parent, qname)`), and
two calls merged into one frame are still two runs, so each gets its own slot.
Full reasoning: `plan/03-call-tree/05` § "Why the slot is per run".

One targeted optimisation worth naming, not yet verified in this repo: `ty`
has to know, internally, whether a given name in a nested scope is local,
global, or free, to do its own name resolution at all — so a function with
**no** free variables doesn't need to be a `Closure` in the first place.
Mark this **[check]** rather than fact until you've confirmed a concrete
accessor for it (`ty_python_core::scope::Scope::parent()` confirms ty
tracks lexical nesting; a per-scope "these names are free here" list is
plausible but not something this repo has confirmed exists as a public
accessor).

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

Questions 1–3 are about the same-frame `outer`/`inner` fixture — still
worth doing, since the frame-ownership rule they test is correct.
Questions 4–5 are about the escaping case, using `make_handler`/`read_json`/
`read_csv` from `python/closures.py`.

1. In `outer()`, is `inner`'s frame a child of `outer`'s frame, or a
   sibling at the module's top level? Justify from the "a `def` doesn't
   execute its body" rule.
2. Suppose `inner` took a parameter with the same name as `outer`'s local —
   `def inner(h): h.go()`, called as `inner(SomeOtherHandler())`. Does
   `env.lookup("h")` inside `inner` ever reach `outer`'s `h` via `enclosing`?
   Why or why not?
3. Write down, in one sentence each, what you'd need to predict before
   running the Python driver on `with_lambda()`, and what you'd check
   after running it to confirm your prediction.
4. For `read_json()`'s call to `h(1)`, what is `caller_env` at the point
   `bind_parameters(handle, args, None, caller_env)` runs? Is it the same
   env `writer` was bound in, or a different one? Name the function whose
   frame it actually belongs to.
5. With the fix (`Closure { func, activation }`), at what point does
   `activation` get set, which env does a lookup of `writer` read, and does
   it matter which frame `handle` is later *called* from? Contrast this with
   question 4's answer under the earlier design.

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
   `enclosing`. This is ordinary lexical shadowing, and it falls out for free
   because `locals` is always checked before `enclosing`.
3. Predict: does `Handler().go()` inside the lambda appear anywhere in the
   call tree produced for `with_lambda`, or does the tree stop at
   `with_lambda → run` with no further children? Check after running:
   whether `run()`'s frame has a `go` child, which tells you whether the
   real driver's `collect_call_node` actually descends into `lambdef`
   nodes or skips them like it skips nested `def`/`class`.
4. `caller_env` is `read_json`'s own env at the point `h(1)` executes —
   which never had `writer` bound in it at all; `writer` was a parameter
   of `make_handler`, bound in a call that already returned by the time
   `h(1)` runs. It belongs to `read_json`'s frame, not `make_handler`'s —
   a completely different function than the one that actually bound
   `writer`. Under the earlier design (before the fix), `closure_parent`
   has no way to recover `make_handler`'s env from this input.
5. `activation` is set once, when the statement walk passes
   `def handle(data): ...` **inside one run of `make_handler`**. A lookup of
   `writer` reads that run's slot — its env as it is at lookup time, which
   after `make_handler` returns is its final env (`writer → JsonWriter`).
   It does **not** matter which frame `handle` is later called from: `h(1)`
   inside `read_json`, or passed three parameters deep as a callback — all
   of them read the *same* run, because the run was fixed at definition
   time, not re-derived at call time. (`read_csv`'s `handle` comes from a
   *different* run of `make_handler`, so it reads a different slot.)
   Contrast with question 4: under the earlier design, the answer to "which
   env does the closure see" changes depending on *where you call it from*,
   which is exactly backwards — a closure's captured variables are
   determined by where it was **written**, never by where it's **invoked**.

</details>

---

→ Next: [`05-returns-and-value-flow.md`](05-returns-and-value-flow.md)
