# 12 — Call tree resolver: whole-exercise answers

Don't read this until you've written your own answers to [`exam.md`](exam.md).

1. ```
   main
   ├── read_json
   │   └── emit
   │       └── JsonWriter.write
   └── read_csv
       └── emit
           └── CsvWriter.write
   ```
   versus, if `main` called `emit` directly twice:
   ```
   main
   └── emit  (call_count=1)
       ├── JsonWriter.write
       └── CsvWriter.write
   ```
   The responsible rule: **frame identity is `(parent frame, target_qname)`**
   (`00-values/05`). Two calls to `emit` from the *same* frame merge into
   one node with both leaves as siblings; two calls from *different*
   frames (`read_json`, `read_csv`) produce two separate nodes, each with
   exactly one leaf.

2. Because arguments are evaluated **eagerly, at each call site, in that
   call's own environment** — not lazily, re-walked through a context
   chain the way Jedi does it. At `middle(w)`, `w` is looked up in
   `outer`'s env (a hit: `Instance{JsonWriter}`) and that *value* — not a
   reference to `outer`'s `w` — is placed directly into `middle`'s own
   `locals` under the name `x`. At `inner(x)`, the same thing happens again:
   `x` is looked up in `middle`'s env (a hit, because it was just placed
   there) and copied into `inner`'s `locals` under `y`. The chain is
   **flattened at each bind**, so `y.write(1)` only ever needs one `env`
   lookup, not a three-hop walk back to `outer` — eager binding converts
   Jedi's transitive context-chain walk into a flat lookup at every level
   (`00-values/03`, `01-functions/01`'s trace, generalised).

3. The `01-functions/05` mechanism is **return-value flow** (`build`'s
   `return` value stored by the tree walk of `build`, then read for `p`); the `02-classes/01`/`03`
   mechanism is **construction with attribute state** (each branch of
   `build` constructs a `Pipeline` and writes `self.handler` in its own
   `__init__` call). `p` resolves to a **union of two values** —
   `Instance{Pipeline, Oa}` (handler = JsonHandler) and
   `Instance{Pipeline, Ob}` (handler = CsvHandler) — because `build` has
   two `return` statements and the "no branch analysis" rule unions both
   arms unconditionally; `kind == "json"` is never evaluated, so the fact
   that `use("json")` was called with a literal `"json"` makes **no
   difference** to what `p` resolves to at this point in the design.
   (A sharper design could special-case literal-valued conditions to avoid
   this union, but that's explicitly out of scope — see `00-values/01`'s
   "what you deliberately do not model" list.)

4. [`01-functions/06-callbacks-and-higher-order.md`](01-functions/06-callbacks-and-higher-order.md)
   names "a callback stored on self, called later" as fixture 5, the one
   that "ties this chapter to `06-attributes-and-self`." Tracing it:
   `Service.__init__` writes `attrs[(O, "cb")] = Function(on_done)` (an
   ordinary attribute write, same mechanism as any other — a function
   value is just another kind of `AbstractValue`); `finish`'s body reads
   `self.cb` via **tier 1** of `member()` (a direct `attrs` hit, not a
   class-body/MRO lookup), getting back `Function(on_done)`; `self.cb(result)`
   then resolves `on_done` as the callee via the ordinary env-hit path and
   descends into it.

5. `h` resolves to whatever `04-annotation-fallback.md`'s **tier 3** (the
   declared parameter annotation) produces — `lift()` applied to
   `Handler`'s type — because tier 1 (`env`) has nothing (the argument
   itself, `opaque`, is `Unknown`) and tier 2 (`ty`'s inference) has
   nothing precise either for an unresolvable value. The one required
   ordering property: tier 3 must **never** override a real hit from tier
   1 or 2 — it only fires when both have already produced nothing, which
   is exactly why a real, path-specific value is never discarded in favour
   of a merely-declared one.

6. This is a test of **`OriginId` isolation**
   (`00-values/02-origin-and-identity.md`, exercised directly in
   `02-classes/04`). The specific bug: if `OriginId` allocation were keyed
   on the *class* being constructed (or on anything less than "which
   `Foo()` expression, building which class, with which argument values")
   rather than per construction,
   `Pipeline(JsonHandler())` and `Pipeline(CsvHandler())` would collide
   onto the same origin, `attrs` would have one entry instead of two under
   that shared key, and whichever `__init__` ran second would silently
   overwrite the first's handler for **both** `a.run(1)` and `b.run(2)`.

7. **One mechanism: `is_ancestor`** (`00-values/05`), for both. Return
   values come out of the same tree walk (`01-functions/05`), so there's no
   second traversal that could loop. `h`: the inner `h()` has `h` as an
   ancestor → cut. `x = f()`: the tree walks `f` → `g` → the inner `f()` is
   cut (ancestor). Its value read finds nothing stored — the outer `f` run
   hasn't finished — so it's `Unknown`; `g` returns that, `f` returns that,
   and **`x` is `Unknown`**. (An earlier design had a separate in-flight
   memo for return values, because it computed them in a second traversal
   that pushed no tree frames. That design is retired.)

8. Yes, `helper(a, b)` as originally written is context-independent —
   neither `a` nor `b` ever appears in a callee or receiver position (they're
   only passed as *arguments* to `logger.info`/`validate`, which doesn't
   taint the callees `logger.info`/`validate` themselves). Changed to
   `def helper(a, b): a.write(b)`, it becomes context-dependent — `a` is
   now directly in **receiver** position, so whether `write` resolves to
   `X.write` or `Z.write` depends on which path called `helper`. This
   specific change is why the independence check needs *more* than a
   parameter-name-at-call-site scan once local bindings exist at all: a
   parameter can reach a callee or receiver position *indirectly* through
   an intermediate local (`x = a; x.write(b)`), and only tracking local
   assignments (taint propagation through `record_bindings`) catches that
   — a bare syntactic scan for parameter names appearing directly as
   callees would miss it.

9. **`@staticmethod`**: no receiver bound at all; positionals start at
   parameter 0. **`@classmethod`**: first parameter bound to the *class*
   value, not an instance. **`@property`**: receiver binding is ordinary
   (first param ← instance), but the access itself (no `()` at the call
   site) must still trigger immediate evaluation of the getter's body —
   a reachability rule, not a binding rule. **A plain decorator
   (`@retry`)**: callee resolution treats the name as the *undecorated*
   function — a deliberate, documented imprecision, not a binding rule at
   all.

10. `vendor/ruff/crates/ty_python_semantic/src/types/vnoc.rs`'s `member`
    function. The test:
    `src/attrs.rs::member_answer_depends_on_which_instance_you_pass` —
    it calls the same wrapper with `Handler`'s instance and then with
    `LoudHandler`'s instance, and asserts the two calls produce *different*
    bound methods, proving the wrapper answers per-receiver rather than
    unioning over every possible construction site the way the public,
    syntax-driven `static_member_type_for_attribute` would.
