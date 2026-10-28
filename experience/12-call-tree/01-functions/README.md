# 01 — Functions, inline mode

**Goal:** a working call tree for plain functions. At every call to a project
function, the callee's body is walked right there, with the caller's values,
and its calls become the node's children. That is the Jedi driver's algorithm,
and the simplest one that gives a correct per-path tree.

By the end you have the walker that every later part reuses: values, the env,
imports, argument binding, branches, returns and closures. Only one thing in
this part is temporary: the line that walks the callee inline. Part 02
replaces it.

---

## The steps

| | step | you build | the decision |
|---|---|---|---|
| 1 | [`01-the-harness.md`](01-the-harness.md) | fixtures with expected trees, a test runner | expected trees live in the fixture, compared as text |
| 2 | [`02-one-call.md`](02-one-call.md) | walk `main`, resolve `helper()` | calls are found by evaluating, not by collecting them first |
| 3 | [`03-locals-and-the-env.md`](03-locals-and-the-env.md) | `f = helper; f()` | the env answers first, ty second |
| 4 | [`04-imports-and-modules.md`](04-imports-and-modules.md) | `import util; util.helper()` | ty resolves imports. Module values keep ty's module type |
| 5 | [`05-following-a-call.md`](05-following-a-call.md) | nested trees | the callee gets a fresh frame, never the caller's locals |
| 6 | [`06-recursion-guard.md`](06-recursion-guard.md) | `f → f` terminates | stop on the current path, and show it |
| 7 | [`07-arguments.md`](07-arguments.md) | `run(on_ok)` → `on_ok` | arguments are evaluated once, eagerly, and bound as values |
| 8 | [`08-star-args-and-splats.md`](08-star-args-and-splats.md) | `*args`, `f(*xs)` | `Unknown`, never a guessed alignment |
| 9 | [`09-branches-and-loops.md`](09-branches-and-loops.md) | `if` / `for` / `try` | fork and join. Conditions are never evaluated |
| 10 | [`10-returns.md`](10-returns.md) | `make()()` | a walk ends in its returns, and a block may never fall through |
| 11 | [`11-closures.md`](11-closures.md) | `make(f)()` | free variables come from where the `def` ran, never the caller |
| 12 | [`12-callbacks-and-decorators.md`](12-callbacks-and-decorators.md) | `run(fetch, on_ok)`, `@retry` | dispatch on the value, never the syntax. Decorators are transparent |
| 13 | [`13-the-wall.md`](13-the-wall.md) | a walk counter | the same function, same inputs, walked over and over |

**Steps 2, 5, 7 and 11 are the core.** 2 is the shape of the walker. 5 is
inline following, the part 02 replaces. 7 is how a value crosses a call. 11 is
the scoping rule that a quick port gets wrong.

---

## What you have at the end

```
src/
├── sem.rs              the only file that talks to ty
└── interp/
    ├── mod.rs          call_tree(), Options, Mode::Inline
    ├── tree.rs         Node, Mark, to_text
    ├── fixture.rs      the test harness (tests only)
    ├── value.rs        AbstractValue, Values, lift
    ├── env.rs          Env, join
    ├── walk.rs         Walk, Frame, walk_stmts, eval
    ├── call.rs         eval_call, call_function (inline)
    ├── bind.rs         EvaluatedArgs, bind_parameters
    └── member.rs       member on modules (instances in part 03)
```

Fixtures are in [`python/`](python/). The project root for every test is that
folder, so `util.py` and `pkg/` next to the fixtures are project code, and the
standard library is not.

---

## Read first

- `plan/03-call-tree/01-what-jedi-actually-does.md`: the algorithm you are
  rebuilding in this part
- `plan/03-call-tree/05-the-walk.md` and `06-binding-arguments.md`: the plan's
  version of steps 2–10

---

## Done when

- [ ] every test from steps 1–13 passes
- [ ] you can say, for each of steps 2–12, which fixture line fails without
      that step's decision
- [ ] you can explain, with step 13's counter, why inline mode can't be what
      ships

---

→ Start: [`01-the-harness.md`](01-the-harness.md)
