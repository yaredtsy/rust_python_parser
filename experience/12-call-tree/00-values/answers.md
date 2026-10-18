# 00 — Values: answers

Don't read this until you've written your own answers to [`exam.md`](exam.md).

1. `Function(f)` — `f` named, not called. `Class(C)` — `C` named, not
   called. `Instance{..}` — `C()`. `BoundMethod{..}` — `obj.method` named,
   not called. `Module(m)` — `import m` then naming `m`. `Ty(t)` — a
   literal, e.g. `1` or `"x"`. `Unknown` — a value your interpreter never
   asked anyone about, e.g. a parameter bound to something that itself
   bottomed out.

2. `Type::Union` is ty's representation of "the set of types this could be,
   considered together" — operations on a `Union` (`member`, display, etc.)
   are defined over the whole set at once. `Values` is your domain's
   decision to keep the same alternatives **separate and enumerable**, so
   each one can independently produce its own subtree in the output tree.
   Treating them the same — i.e. collapsing `Values` back into one
   `Type::Union` — silently rebuilds `ty_ide::outgoing_calls`'s behaviour:
   every path through a union-typed callee gets the *same* answer,
   regardless of which branch actually produced it on this path.

3. `Box<AbstractValue>` is a heap allocation, which makes `AbstractValue`
   lose `Copy`. Since `AbstractValue` gets copied into environments and
   passed by value constantly, that one `Box` would make every clone that
   touches a `BoundMethod` allocate. Interning the receiver into a
   per-request arena and storing its `u32` index (`ValueId`) keeps
   `AbstractValue` `Copy` and, as a bonus, dedupes identical receivers that
   repeat across a method-heavy codebase.

4. **Different** `OriginId`s. `OriginId` identifies a *construction site on
   a path* — "which `Foo()` expression, reached by which sequence of
   calls" — not just a line number. Two different callers reaching the same
   line produce two different paths, hence two different origins, even
   though the source text of the construction is identical. This is exactly
   what keeps `a = Pipeline(JsonHandler())` in one caller and
   `b = Pipeline(CsvHandler())` in another from cross-contaminating each
   other's `self.handler`.

5.
   ```rust
   self.resolve_from_env(expr, env)
       .or_else(|| self.resolve_from_ty_if_precise(expr))
       .unwrap_or_else(|| self.resolve_from_annotation(binding))
   ```
   Tier 1 (`env`) may override tier 2 and 3; tier 2 may override tier 3;
   tier 3 never overrides anything — it only fires when both earlier tiers
   produced nothing. The ordering is the whole safety argument: a real,
   path-specific value must never lose to a declared-but-unverified
   annotation.

6. The chain reaches tier 3: `env` has nothing (the value was `Unknown` at
   the call site), `ty`'s own inference for an unresolvable argument gives
   nothing precise either, so `resolve_from_annotation` lifts `Handler` from
   the parameter's declared type. This should be recorded as a
   lower-confidence hit — increment an `annotation_fallbacks` counter in
   your stats — because it's a declared claim, not a traced value; a caller
   asking "why does this leaf say `Handler`" deserves the honest answer
   "the annotation, not the trace."

7. **Frame identity is `(parent frame, target_qname)` — two calls to the
   same function from the same frame are one node; the same function from
   different frames is different nodes.** Three calls to `f()` from one
   frame: **one** child, `call_count == 2` (starts at 0, bumped on each
   call past the first).

8. ```python
   def f(): return g()
   def g(): return f()
   def main():
       x = f()     # a VALUE query — no tree frame pushed for this
       x.run()
   ```
   `is_ancestor` only tracks the **tree's** call frames, and a value query
   like `f()`'s return-value evaluation never pushes one. So an
   `is_ancestor`-based guard here never fires at all, and `f → g → f → g →
   …` recurses forever trying to compute a return value. The in-flight memo
   is a *separate* mechanism precisely because it guards the value-query
   recursion that `is_ancestor` (a tree-walk-only mechanism) can't see.

9. **Depth cap, node budget, union fan-out cap, chain fan-out cap** (plus
   the ancestor guard from the previous file and the wall-clock check —
   if you listed those too, that's fine, the file names four as the
   "cheap layers" table's rows). The **node budget** is the one checked
   once globally per request — every other limit (depth, fan-out) is
   evaluated locally, at the specific expression or call site it bounds.
   The distinction matters because only a single global counter gives you
   a *hard* latency guarantee independent of the input's shape — a
   per-branch limit alone can't prevent many cheap branches from adding up
   to an expensive request.

10. Before local-assignment tracking, the context-independence check is a
    syntactic name scan: "does any parameter name appear directly in a
    callee or receiver position?" Once local bindings are tracked, a
    parameter can reach a callee position **indirectly**, through a local
    variable: `def run(w): x = w; x.write(1)` — `w` never appears as a
    callee itself, but `x` does, and `x` was bound from `w`. A scan that
    only looks at parameter *names* at call sites would wrongly call `run`
    context-independent and memoise one subtree for every writer it's ever
    called with, silently producing the same (wrong) leaf regardless of
    which writer a given path actually passed in.
