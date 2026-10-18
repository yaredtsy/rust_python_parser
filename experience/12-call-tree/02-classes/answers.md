# 02 — Classes: answers

Don't read this until you've written your own answers to [`exam.md`](exam.md).

1. Right before `__init__`'s own environment is built — the instance must
   exist before it can be bound to `self`. If it were minted *after*
   `__init__`'s body finished, there would be no `OriginId` for
   `self.handler = handler` to key its `attrs` entry on while that
   statement runs, so the write would have nowhere valid to go.

2. Both: `{ self → Instance{Greeter, O1}, name → Instance{Payload, O2} }`.
   Identical because the *bound* call supplies the receiver explicitly
   (via `BoundMethod`'s packaged receiver, consumed by step 1 of
   `bind_parameters`), while the *unbound* call supplies no receiver at
   all — but `Greeter()` is then just the first positional argument, which
   lands on the first still-open parameter slot (`self`) anyway, purely by
   position. Same result, different route.

3. **(1) path-specific instance state (`attrs`), (2) class body via the
   MRO, (3) `ty`'s full generality (descriptors, `__getattr__`,
   metaclasses).** Tier 1 is "path-specific" because it's keyed on
   `OriginId` — it answers "what did *this specific instance, on this
   path* get assigned" rather than "what does this *class* generally
   have," which is exactly the distinction a call-graph tool can't make.

4. A single-instance fixture can't distinguish "the write landed under the
   right key" from "there's only one key, so of course it looks right." The
   two-instance fixture specifically tests whether `Pipeline(JsonHandler())`
   and `Pipeline(CsvHandler())` get **different** `OriginId`s — if origin
   allocation collided (e.g. keyed on class instead of construction site),
   both instances' `attrs` entries would land under the same key, and
   whichever `__init__` ran *second* would silently overwrite the first's
   handler for **both** instances. A single-instance fixture has no second
   instance to collide with, so it passes either way.

5. No separate code path — `record_bindings`/`record_self_assignments`'s
   rule is "when walking *any* method body with a bound `self` (or more
   generally, any receiver), record `self.X = ...` assignments as they're
   encountered." This isn't `__init__`-specific machinery with `configure`
   added as a special case; it's the same statement-level recording step
   already required for ordinary local-assignment tracking, applied
   uniformly to every method body that gets walked.

6. No difference in kind — `bind_attribute` evaluates the receiver
   expression (`p`, here an ordinary name lookup in the module's `env`)
   regardless of whether the assignment statement sits inside a method or
   at module scope. The write succeeds exactly when that evaluation
   produces a traced `AbstractValue::Instance` — if `p` resolves to
   anything else (notably `Unknown`), the write is silently dropped per
   the "drop writes to unknown receivers" rule, with no exception made for
   module-scope writes.

7. `static(1)`: `{ x → Ty(int) }` — no receiver slot at all, because
   `@staticmethod` suppresses receiver-binding, so the single positional
   `1` lands directly on `x`. `create(Handler())`: `{ cls → Class(Service),
   handler → Instance{Handler, O1} }` — `cls` is bound to the **class**
   value `Service` (not an instance), and `cls(handler)` then resolves
   through the ordinary constructor path, producing a fresh
   `Instance{Service, O2}`.

8. `super().__init__()` must evaluate to an `Instance` carrying the **same
   `OriginId`** as `self` in `Child.__init__` — not a fresh one. With that
   shared origin, `Base.__init__`'s `self.log = Logger()` writes into
   `attrs[(that shared origin, "log")]`, and `Child.go`'s read of `self.log`
   (tier 1 of `member()`, keyed on the very same origin since `self` inside
   `go` is the same instance) finds it directly. The field that must be
   shared is the `OriginId` itself — nothing about the attribute store or
   `member()` needs to know anything about inheritance explicitly.

9. `vendor/ruff/crates/ty_python_semantic/src/types/vnoc.rs`'s `member`
   function (with a passing test for exactly this property already in
   `src/attrs.rs`). It answers "what is `.name` on *this* receiver's type,"
   walking `ty`'s own MRO, descriptors, `__getattr__` and metaclass rules
   correctly because it's the same lookup `ty` uses internally. It does
   **not** answer *which* receiver to pass for a given expression on a
   given path — that choice (e.g. "`self.handler` is a `LoudHandler` on
   this specific path") is entirely the attribute store's and the
   environment's job, not the wrapper's.

10. The receiver must be **interned** (`ValueId`, from
    `00-values/02-origin-and-identity.md`) rather than re-derived lazily
    from the original expression. Without it, by the time the callback is
    actually invoked inside whatever function it was passed into, the
    expression `self.handler.on_done` that originally produced it is long
    gone from the AST being walked at that point — there would be nothing
    left to re-evaluate it from, and the callback's receiver would be lost
    entirely rather than merely imprecise.
