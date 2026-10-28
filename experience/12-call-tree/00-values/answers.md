# 00 — The structures: answers

Don't read this until you've written your own answers to [`exam.md`](exam.md).

1. See the README picture. **Whole request:** `Solver`, its `fps`, `contexts`,
   `slots`, `stack`, `heads`, and each slot's `state` and `result` (the
   summary). **One walk only:** `Walk` and everything in it: `Env`, `origins`,
   `values`, `activations`, `deferred`, and every `AbstractValue` and walk-local
   id. (A walk in a recursion group lives in its slot until the group resolves.)

2. `AbstractValue`, in a walk: yes, by an `OriginId` that means something only in
   that walk. `Fp`, in a key: no, class and attributes only. `SVal`, in a
   summary: yes, relative to the inputs, as `Ref(path)` or `Obj(j)`.

3. Small request-local indexes into the solver's interners. Interning turns
   "have I seen this call shape before?" into a lookup: an equal context gets
   the same id, and its slot already holds the summary. A context refers to
   its fingerprints by `FpId`, so nested fingerprints are shared too.

4. (a) `Instance { Pipeline, [writer: Instance { JsonWriter, [] }] }`.
   (b) `Data(int)`: literals widen to their class. (c) `Opaque(JsonWriter)`: an
   instance ty handed over has no tracked attributes. (d) `Unknown`: irrelevant
   parameters are left out of the key.

5. `New → Running → Ready`. With recursion: `Running → Parked → Resolving →
   Ready`. With a cost limit read during the walk: `Running → Cut`, and a `Cut`
   slot goes back to `Running` the next time it is reached. Without recursion
   or limits: `New`, `Running`, `Ready`.

6. `returns: [Plain(Ty(None))]`, `effects: [Effect { target: Param(0), attr:
   handler, value: [Ref(Param(1))], strong: true }]`, `objects: []`, `calls: []`,
   Finished. The effect came from the exit env's `written` set: `(Entry(Param(0)),
   handler) → Must`.

7. From `materialize` (when `get` finds a `New` or `Cut` slot) to `finish`. Then
   it is dropped, and the slot keeps only the summary. The exception: a walk in a
   recursion group is kept, `Parked` then `Resolving`, until the group's fixpoint
   ends, because its deferred operations are replayed.

8. `locals` (what does `x` hold), `attrs` (what does this object's `.a` hold),
   `written` (which attributes this walk wrote, `Must` or `May`), `scope` (may
   this name be seen here), `enclosing` (for a closure, the run where it was
   defined). No caller link because values from the caller arrive only through
   the key; looking a name up through the caller would leak one function's
   locals into another.

9. `Entry(path)`, `New { site, t, combo, made }`, `Beyond { at, attr }`. In
   `run`: `Entry(Param(0).writer)`. In `read_json`: `New { site: JsonWriter(),
   t: 0, combo: 0, made: Instance }`.

10. (1) fingerprint: part of `Instance { Pipeline, [writer: Instance { JsonWriter, [] }] }`,
    no identity. (2) materialize: `Instance { JsonWriter, Entry(Param(0).writer) }`
    in `run`. (3) finish: `Ref(Param(0).writer)` in `run`'s returns. (4) resolve:
    `Instance { JsonWriter, o0 }` in `read_json`: the same object it started as.
