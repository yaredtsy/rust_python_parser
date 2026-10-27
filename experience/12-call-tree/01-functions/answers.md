# 01 — Functions: answers

Don't read this until you've written your own answers to [`exam.md`](exam.md).

1. Two `emit` nodes: `entry → read_json → emit → JsonWriter.write` and
   `entry → read_csv → emit → CsvWriter.write`. Exactly two because frame
   identity is `(parent frame, target_qname)` — `emit` is reached from two
   *different* parent frames (`read_json` and `read_csv`), so it gets two
   separate nodes, each carrying only the one leaf that path's writer
   produced. (One frame calling `emit` twice would instead merge into a
   single `emit` node with both leaves as siblings.)

2. For `a()`: `DEFAULT` is resolved in the **defining scope** of `process`
   — the module where `process` itself lives — never the caller's
   environment, regardless of what module `a` is defined in. For `b()`:
   no — `process(Handler())` supplies a positional argument, so
   `bind_parameters` never reaches the default branch for `h` at all; the
   default expression is not evaluated.

3. **All** parameters bind to `Unknown`, including `a` — not just the
   starred ones. The call-site-splat rule is "bind everything to Unknown
   rather than guess an alignment," because the length of `items` is
   unknown and any specific alignment would be a guess dressed up as a
   fact. `a.go()` is still a node in the tree — the walk proceeds
   regardless of what `a` resolved to — but its leaf resolves to nothing
   useful, since `member()` has an `Unknown` receiver to work with.

4. Eagerly-bound — `h` is a free variable of `inner`, read directly via
   `Env.enclosing` pointing back to the environment active where `inner` was
   *defined* (inside `outer`'s own call frame), not passed as an argument
   anywhere. If `inner` instead took `h` as a parameter and were called
   `inner(h)`, the value would arrive the ordinary way — eagerly evaluated
   in the caller's env and placed directly into `inner`'s own `locals` by
   `bind_parameters` — and `Env.enclosing` would never be consulted for `h` at
   all, because `locals` is always checked first and would already have a
   hit.

   ⚠ Caveat worth holding onto: this fixture calls `inner()` from *inside*
   `outer`, so `caller_env` at that point already equals the defining env —
   the lookup succeeds without the mechanism actually having to be
   closure-aware. Call the closure from a *different* frame than the one
   that defined it (`04`'s `make_handler`/`read_json`/`read_csv` fixture)
   and the same reasoning breaks: `caller_env` is then wherever the call
   happens to be textually written, not where `h` (or `n`, or `writer`) was
   bound. See `04-closures-and-nested-functions.md`'s "escaping-closure
   gap" section.

5. From the **return store**. When the tree walk resolves `make()` (adding
   `make` as a child of `run`'s frame), it walks `make`'s body and stores
   the union of its returns — `Instance{JsonWriter, O}` — under `make`'s
   `ReturnKey`. It's there in time because inside one statement calls are
   resolved *before* bindings are recorded, so `make()` is walked before
   `record_bindings` asks what `w` is. Reading it is a lookup: it walks
   nothing and emits nothing, so `make` appears once.

6. `a()` and `b()` — **two different frames**, each calling `run` once —
   is the shape that demonstrates context sensitivity: two `run` nodes, one
   with `on_success` as its only descendant structure reachable from `a`'s
   path, the other reachable only from `b`'s. A single frame calling `run`
   twice with the same callback would merge into one `run` node via the
   `(parent, qname)` rule and tell you nothing about path sensitivity at
   all — any path-free tool would print something nearly identical.

7. **A name that resolves to a function definition is treated as the
   undecorated function for call-tree purposes — decorators are
   transparent.** The risk: a decorator that genuinely changes the
   function's arity or receiver convention (something `functools.partial`-like)
   makes this assumption bind arguments incorrectly; the mitigation is to
   detect a non-trivial decorator list and fall back to binding everything
   `Unknown` for that specific call, rather than confidently mis-binding.

8. `f`: **one** node (`main → f`); the guard fires inside `f`'s own frame,
   on the attempt to add a *second* `f` as its child (ancestors at that
   point: `[root, main, f]`, and `f` is already on the chain). `g`/`h`:
   **two** nodes (`main → g → h`); the guard fires inside `h`'s frame, on
   the attempt to call `g` again (ancestors: `[root, main, g, h]`, and `g`
   is already on the chain).

9. `def walk(node): r = walk(node.first()); r.close(); return Handle()`.
   The tree walks `walk` once; the inner `walk(node.first())` has `walk`
   as an ancestor, so it's cut. Its value read finds nothing stored — the
   outer run hasn't reached `return Handle()` yet — so `r` is `Unknown`,
   and **`r.close()` gets no edge**, even though `walk` plainly returns a
   `Handle`. It's a known gap, accepted for v1; `plan/03-call-tree/10`
   pins it as fixture 21.

10. Neither resolves precisely under the base design when the returned
    closure is called from a different frame than where it was created —
    but for different reasons, one permanent and one fixable.
    `functools.partial` stays opaque **permanently, by design**: it's a
    stdlib function, `is_project_code` is false for it, and the
    interpreter never looks inside stdlib bodies at all — there are no
    `return` statements of `functools.partial`'s own to interpret, only
    `ty`'s usually-opaque declared type for whatever it returns. Manual
    curry's imprecision is a **fixable gap**, not a design limit: `inner`
    is an ordinary project function, and the only reason `n` fails to
    resolved under the earlier draft is that `closure_parent(func, caller_env)` was handed the
    *caller's* env instead of the env captured at `inner`'s definition
    site (`04-closures-and-nested-functions.md`'s escaping-closure
    section). With `Closure { func, activation }`, now in the plan, curry
    resolves at full precision and `partial` still doesn't — which is
    exactly the distinction worth holding onto: one imprecision is a
    property of what `functools.partial` *is* (foreign code), the other was
    a property of what the interpreter hadn't built yet.
