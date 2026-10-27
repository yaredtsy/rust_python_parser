# 02 — Functions: answers

Don't read this until you've written your own answers to [`exam.md`](exam.md).

1. Two contexts, two walks, two rendered nodes (`entry > read_json > emit >
   JsonWriter.write`, `entry > read_csv > emit > CsvWriter.write`). One key:
   `emit { writer: Instance{JsonWriter, []}, payload: Data(int) }` — `payload` is
   passed on to a call, so it counts.

2. From the scope that defined `process`: `DEFAULT` is a module global, so ty
   answers, and `h` is an `OPAQUE` `Handler`. Binding never evaluates a call:
   the default ran when the `def` ran, in that walk, and its calls were recorded
   there. Evaluating again would record them in every caller's walk.

3. All of them: a splat at the call site makes every parameter `Unknown`, since
   any alignment would be a guess. `a.go()` is still reached and recorded, but
   with an `Unknown` receiver it has no target, so no node is rendered for it.

4. `returns: [Closure { handle, free: [(writer, [Ref(Param(0))])] }]`. When
   `read_json` resolves it, `Param(0)` is its own `JsonWriter`, so the closure
   carries `writer → that JsonWriter`. Calling it keys `handle` with that in
   `free`. Where it is called doesn't matter.

5. From `make`'s summary: `returns: [Obj(0)]`, resolved in `run` to a new origin
   at that call site. `make` has one context (no parameters), so every caller
   reuses one summary.

6. One context: `task` is `Data(str)` both times and `cb` is the same function.
   Two rendered nodes, one under `a` and one under `b`.

7. A name bound by a `def` resolves to that `def`'s own function: decorators are
   transparent. The risk is a decorator that changes the signature or behaviour;
   binding guards the first by making every parameter `Unknown` for decorators
   not on the allow-list.

8. `main > f > f [Recursive]`, and `main > g > h > g [Recursive]`. Each body is
   walked once. The Jedi driver drops the recursive call; pylspt shows it as a
   marked leaf.

9. `combine` calls `parse_expr`, which is in the group, so `combine` is part of
   the cycle. It gets `Pending` for that call, parks, and joins: its body is
   walked once, and only its deferred operations are repeated in later rounds,
   like every other member's.

10. `Handle` starts in `a` and must reach `b` through `c`. In a single pass `b`
    could read `c` before `c` had learned about `Handle`. Rounds repeat until
    nothing changes. What repeats is only the deferred operations ("read a
    stored summary, translate it, add it in"), never a statement.
