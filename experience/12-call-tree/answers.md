# 12 — The call tree analysis: whole-exercise answers

Don't read this until you've written your own answers to [`exam.md`](exam.md).

1. Contexts: `main`, `read_json`, `read_csv`, `emit{writer: JsonWriter}`,
   `emit{writer: CsvWriter}`, `JsonWriter.write`, `CsvWriter.write`. Graph:
   `main → read_json, read_csv`; `read_json → emit{Json}`; `read_csv → emit{Csv}`;
   each `emit` → its writer's `write`. Tree:
   `main > read_json > emit > JsonWriter.write` and
   `main > read_csv > emit > CsvWriter.write`. v-noc saves the same: the two
   `emit` nodes have different parents.
   If `main` called `emit` twice directly: the same two `emit` contexts, but
   both nodes under `main`. pylspt sends two `emit` nodes; v-noc merges siblings
   by qname and saves one `emit` with `call_count = 1` and both writes under it —
   which looks like a path-free tool's answer. That's why fixtures use the first
   shape.

2. Arguments are evaluated eagerly, in the caller's env, and become the callee's
   key. `middle`'s key has `x: Instance{JsonWriter}`, so `middle`'s walk starts
   with it; `inner(x)` evaluates `x` there and keys `inner` with
   `y: Instance{JsonWriter}`. Each level's key carries the value. Nothing looks
   back.

3. `build`: `returns: [Obj(0), Obj(1)]`, `objects: [Pipeline{handler: Obj(2)},
   Pipeline{handler: Obj(3)}, JsonHandler, CsvHandler]`. Conditions are never
   evaluated, so `p` is both pipelines after `use("json")` too. Two `run`
   contexts (one per handler type), edges to `JsonHandler.process` and
   `CsvHandler.process`.

4. `Service.__init__`'s effect `Param(0).cb = Ref(Param(1))` puts `on_done` on
   the new object. `finish`'s key carries `self: Instance{Service, [cb: Function(on_done)]}`,
   so `finish`'s walk starts with it. `self.cb` hits `member` step 1 (the env),
   and `self.cb(result)` calls `on_done`.

5. `Instance{Handler, Entry(Param(0))}`, from the annotation, because the entry's
   key holds `Unknown` for `h`. The fallback only fires when the key says
   `Unknown`, so a traced value from a caller always wins.

6. The old handler's `process`. Phase 1 resolves `Ref(Param(0).h)` against the
   snapshot before the call; phase 2 then writes the new `h`. Reversed, the
   return would read the new handler, and the old one's edge would be lost.

7. One walk each. `f` and `g` form a group; neither has a base value, so the
   fixpoint gives nothing: `x` is empty (`f` never returns, as in Python). `h`:
   `... > h > h [Recursive]`, its body walked once.

8. `combine` gets `Pending` for its call into the cycle, parks, and joins the
   group: walked once, its deferred operations repeated with the others'.
   Inside a group, every call from one member to another answers `Pending`
   whichever member came first, so every walk is the same; and the fixpoint is
   the least solution of those equations, which doesn't depend on the order.
   Entered at `combine`, the group simply merges into `combine`'s and resolves
   when its walk ends.

9. First: neither parameter reaches a project call, receiver, return or write
   (`logger.info` and, say, a stdlib `validate`), so the key is all `Unknown`
   and every call shares one summary. Second: `a` is relevant, because it
   reaches a receiver through `x`; `b` is relevant, as an argument to a call that
   might be project code. A scan that only looked for parameter names as
   receivers would miss `x = a`, call `a` irrelevant, and give every caller the
   first caller's `write`.

10. Truncates: `max_stack`, `max_contexts`, `contexts_per_func`, the deadline.
    Widens: `fp_depth`, `split_cap`, `max_path`, the fan-out caps,
    `max_rounds`. A widening rule depends only on the key and the code, so every
    caller with that key would get the same answer: safe to reuse. A truncation
    depends on how the context was reached, so reusing it would hand one caller's
    truncation to another.
