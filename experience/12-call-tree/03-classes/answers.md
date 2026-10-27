# 03 — Classes: answers

Don't read this until you've written your own answers to [`exam.md`](exam.md).

1. `__init__`: `effects: [Param(0).handler = [Ref(Param(1))], strong]`. The
   caller applies it to the new object. `run`'s key:
   `{ self: Instance{Pipeline, [handler: Instance{JsonHandler, []}]}, data: .. }`.
   `run`'s walk starts with `self.handler` in its env, written by materialize
   from the key.

2. Both `[self ← Greeter object, name ← Payload object]`. The bound call passes
   the receiver from the `BoundMethod`; the unbound call passes none, and the
   first positional lands on `self` by position. The rule is about position,
   never the name `self`.

3. (1) the env's attributes for this object, (2) the class side through
   `vnoc::member`, binding our receiver for methods, (3) for an input object, an
   attribute the key didn't hold becomes an object named by its path. Step 1 is
   path-specific: it is what this path wrote or what this call's key carried.

4. That two constructions get two origins. With one origin per class, both
   `handler` writes land in one slot and the second wins for both objects. A
   one-instance fixture has nothing to collide with. Two `__init__` contexts
   here (different handler types); if both handlers had the same type, one
   context would be right — the summary talks about `Param(0)`, and each
   caller's own origin keeps the objects apart.

5. `configure`'s summary has the effect `Param(0).handler = Obj(0)`. The caller
   resolves `Param(0)` to `p`, writes `handler`, and `p.run()` keys `run` with
   that handler in `self`'s fingerprint.

6. Because `svc.app.config` is an input object (`Entry(Param(0).app.config)`),
   reading a missing attribute on it gives an object named by its path,
   `Entry(Param(0).app.config.sink)`, not `OPAQUE`. So the write is tracked:
   `effects: [Param(0).app.config.sink.h = [Obj(0)], strong]`, and the caller
   applies it to its real `Sink`. Past `max_path`, the object is `Beyond`, the
   write becomes a weak `Unknown` at the deepest path that fits, and the summary
   is marked `DEEP_WRITE`.

7. `static(1)`: `[x ← Ty(int)]`, no receiver. `create(Handler())`:
   `[cls ← Class(Service), handler ← Handler]`, and `cls(handler)` is an ordinary
   construction of `Service`, with a new origin at that site.

8. `super().__init__()` calls `Base.__init__` with the same receiver.
   `Base.__init__`'s effect on its `Param(0)` lands on `Child.__init__`'s
   `self`, becomes part of `Child.__init__`'s summary, and so reaches the new
   `Child` object. No inheritance logic in the store.

9. `vendor/ruff/crates/ty_python_semantic/src/types/vnoc.rs`, `member` (tested
   in `src/attrs.rs`). It answers what `.name` is on a given type, through ty's
   own MRO and descriptor rules. It does not decide which object a receiver is
   on this path; the env, keys and origins do.

10. An `Unknown` receiver has no identity: a write to "some object" can't be
    reported to anyone without invalidating everything. A `Beyond` object is a
    known input object, just deeper than the path cap, so the caller can be told
    "something under this path changed": a weak `Unknown`, marked. Incomplete,
    but never silently wrong.
