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
   `Env.parent` pointing back to the environment active where `inner` was
   *defined* (inside `outer`'s own call frame), not passed as an argument
   anywhere. If `inner` instead took `h` as a parameter and were called
   `inner(h)`, the value would arrive the ordinary way — eagerly evaluated
   in the caller's env and placed directly into `inner`'s own `locals` by
   `bind_parameters` — and `Env.parent` would never be consulted for `h` at
   all, because `locals` is always checked first and would already have a
   hit.

5. `resolve_call` (the ordinary tree walk, which adds `make` as a child of
   `run`'s frame regardless of its return value) and `eval_returns` (a
   value query that interprets `make`'s `return JsonWriter()` to determine
   what `w` is bound to). Merging them would double-count `make` in the
   tree — once from the walk finding the call site, and again from
   whatever bookkeeping a merged pass might do while evaluating the return
   — and would also make the in-flight memo (safe only because it's a pure
   value query with no tree side effects) unsafe to cache, since it would
   now be entangled with frame mutation.

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

9. **Wrong if reused for the value query, missing the cycle:**
   `def f(): return g(); def g(): return f(); def main(): x = f(); x.run()`
   — `is_ancestor` only tracks tree frames, and `x = f()` is a value query
   that pushes no tree frame, so the check never fires and `f↔g` recurse
   forever trying to compute a return value.
   **Wrong if reused, firing when it shouldn't:**
   `def walk(node): r = walk(node.first()); r.close(); return Handle()` —
   `walk` genuinely is a tree ancestor while its own return value is being
   computed, so an `is_ancestor`-based guard fires and answers `Unknown`
   for `walk`'s return — even though `walk`'s `return Handle()` is plainly
   determinable and `r.close()` should resolve against `Handle`.

10. `functools.partial` stays opaque; manual curry resolves at full
    precision. The reason: curry's "remembered" state is just an ordinary
    closure your interpreter already interprets completely (a nested
    function, a free variable via `Env.parent`, a value returned via
    `eval_returns`) — nothing about it is foreign machinery. `partial` is a
    stdlib *function*, so `is_project_code` is false for it, and the
    interpreter never looks inside stdlib bodies at all — it has no return
    statements of `functools.partial`'s own to interpret, only `ty`'s
    (usually opaque) declared type for whatever it returns.
