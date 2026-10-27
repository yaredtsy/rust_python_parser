# 01 — Summaries: answers

Don't read this until you've written your own answers to [`exam.md`](exam.md).

1. The tree is exponential in depth (3 calls per function, depth 15: 14 million
   nodes). The Jedi driver only survives by being slow and stopping early. With
   summaries, work is the number of distinct contexts times body size; the tree
   is rendered from the summaries afterwards.

2. (1) A summary is a function of its key: otherwise one caller's extra state
   leaks to the next caller with the same key. (2) Nothing walk-local leaves a
   summary: otherwise a summary names "origin #17", which means something else
   in every other walk. (3) Precision depends only on the key and the code; cost
   limits only truncate: otherwise a summary truncated for one caller is reused
   by another, and the answer depends on which came first.

3. One. Both have
   `self = Instance { Pipeline, [handler: Instance { JsonHandler, [] }] }`.
   It's safe because the summary never names the object: it talks about
   `Param(0)` and paths from it. Each caller resolves those against its own
   object. Identity is kept by paths and aliases, not by the key.

4. The same for both: `{msg: Unknown, obj: Unknown}`. Neither parameter reaches
   a project call, a receiver, a return or a write, so neither is in the key.
   One summary for every call.

5. `install`: `effects: [Param(0).handler = Ref(Param(1)), strong]`. `maybe`:
   the same effect, not strong. For `install` the caller replaces `p.handler`;
   for `maybe` it joins the new value with the old one.

6. Phase 1 resolves the summary's values against the snapshot taken before the
   call: `Ref(Param(0).h)` reads `b.h` as it was, the `JsonHandler`. Phase 2
   writes the effect, `b.h = CsvHandler`. Reversed, `Ref(Param(0).h)` reads the
   just-written `CsvHandler`, so `old.process()` finds `CsvHandler.process` and
   `JsonHandler.process` is lost.

7. The two contexts each return a `Job`. If both got one origin, the two `Job`s
   would be one object in the caller, their attributes joined, and `go()` would
   see pairs that never existed: a `JsonHandler` with a `CsvKey`.

8. The truncation depended on how the context was reached (the stack depth),
   not on its key. Storing it would hand a truncated answer to a later caller
   with plenty of room. So its slot is `Cut`, and it is walked again next time.

9. It recomputes every summary instead of reusing it, and the output must be
   identical. That catches a walk that depends on something its key doesn't
   hold. It doesn't check that the graph is right: both modes can miss the same
   edge.

10. Two nodes sent, each with `call_count = 0`. One node saved, with
    `call_count = 1` and both writes under it. Fixtures avoid it because the
    saved tree looks like what a path-free tool prints; calls from different
    functions stay apart after the merge, and only those prove path sensitivity.
