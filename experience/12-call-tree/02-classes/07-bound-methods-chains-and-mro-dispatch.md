# 02.07 — Bound methods, attribute chains, and MRO dispatch

Closing this folder by tying it back to `01-functions/06` (callbacks) and
to the one piece of real, already-merged code in this repo that this whole
exercise has been building toward: `vnoc::member`.

**Fixture:** `python/receiver_rules.py` (`attribute_chain`),
`python/inheritance.py`. **Reading:**
[`plan/03-call-tree/06-attributes-and-self.md`](../../../plan/03-call-tree/06-attributes-and-self.md)
§"★ Step 2 is now one wrapper call", and `src/attrs.rs` in this repo (not
`plan/`, not an exercise fixture — real, merged, tested code).

---

## Attribute chains: `member()`, applied three times

```python
session.service.handler.handle(4)
```

```
eval(session.service.handler.handle)      ← the CALLEE of this call
  = member(member(member(eval(session), "service"), "handler"), "handle")

eval(session)                   → env hit → Instance{Session, O1}
member(Instance{Session,O1}, "service")
  → attrs[(O1, "service")]       → Instance{Service, O2}     (written in Session.__init__)
member(Instance{Service,O2}, "handler")
  → attrs[(O2, "handler")]       → Instance{Handler, O3}     (written in Service.__init__)
member(Instance{Handler,O3}, "handle")
  → attrs has no "handle" entry  → MRO walk on Handler finds `handle` directly
  → BoundMethod{ func: Handler.handle, receiver: intern(Instance{Handler,O3}) }
```

`self.a.b.c()` is exactly this, applied to `self` instead of a plain name —
no new mechanism. The one thing worth watching: because `member` returns
`Values` (plural), a chain over an **ambiguous** link fans out
multiplicatively — a chain where each link has 3 possibilities is 27 paths
by the third hop. `00-values/06`'s `MAX_CHAIN_FANOUT` exists specifically
for this; cap it there, not here.

## Depth is recursion, not a store feature

`Env` maps names to `Values`, and an `Instance` carries its own `OriginId`, so
hop *n+1* is the same lookup as hop *n*. The only depth-related code is that
`eval` of `Expr::Attribute` recurses on `attr.value`:

```rust
Expr::Attribute(attr) => {
    let recvs = self.eval(&attr.value, env, model);     // `a.b.c` -> eval `a.b` -> eval `a`
    let mut out = Values::new();
    for recv in recvs.into_iter().take(MAX_CHAIN_FANOUT) {
        out.extend(recv.member(cx, &attr.attr.id));
        if out.len() >= MAX_CHAIN_FANOUT { break; }
    }
    dedup(&mut out);
    if out.is_empty() { smallvec![Unknown] } else { out }
}
```

- **Mid-chain `Unknown`:** `Unknown.member(..)` is `[Unknown]`, so the chain
  quietly dies and the final call has no edge (`unknown_mid_chain` in
  `python/deep_chain.py`).
- **Cap per hop, after dedup:** two paths to the same instance cost nothing.
- **Mixed hops:** each hop looks only at its own receiver's variant, so
  `config.SERVICE.client.send()` (module -> instance -> instance) needs
  nothing extra. A module hop also resolves **submodules** that the program
  imported (`pkg.sub.mod.f()`), see the plan's "Module chains".
- **Writes at depth** are `05` §(c).

## Bound methods as callbacks — closing the loop with `01-functions/06`

```python
run(fetch, self.handler.on_done)
```

`self.handler.on_done` is evaluated once, at the call site, producing a
`BoundMethod{func: on_done, receiver: <interned handler instance>}` via the
exact chain above. **The receiver must survive the trip through `run`'s
parameter** — by the time `cb()` actually executes inside `run`'s body,
the expression `self.handler.on_done` is long gone from the AST being
walked; only the *value* `cb` was bound to remains. This is precisely why
`00-values/02` interns the receiver as a `ValueId` rather than re-deriving
it lazily: there would be nothing left to re-derive it *from*.

## `super()` and inheritance — same origin, nothing special needed

```python
class Base:
    def __init__(self): self.log = Logger()
class Child(Base):
    def __init__(self):
        super().__init__()
        self.x = 1
    def go(self):
        self.log.write("hi")     # ← from Base.__init__, not Child's own
```

```rust
// eval: Call { func: Name("super"), args: [] } in a method body
//   → AbstractValue::Instance { class: <next in MRO>, origin: SAME origin as self }
```

Make `super().__init__()` an ordinary call whose receiver is **the same
instance** (same `OriginId`) as `self` in the calling `__init__`. Nothing
else is needed — `Base.__init__`'s `self.log = Logger()` write lands under
key `(that same origin, "log")`, and `Child.go`'s read of `self.log` finds
it via tier 1 of `member()`, exactly as if `Child` had written it directly.
Inheritance falls out of origin-sharing, not out of any MRO-aware code in
the attribute store itself.

## ★ Where "walk the MRO" stopped being your code

Tier 2 of `member()` — "walk the MRO for a class member" — is not
something this project implements from scratch. It's **one already-merged
wrapper**, real code in this repository:

```rust
// vendor/ruff/crates/ty_python_semantic/src/types/vnoc.rs
pub fn member<'db>(db: &'db dyn Db, env: &ProgramEnvironment<'db>, ty: Type<'db>, name: &str)
    -> Option<Type<'db>>
{
    ty.member(db, env, name).ignore_possibly_undefined()
}
```

`ty`'s own `Type::member` already handles the MRO walk, descriptors,
`__getattr__`, metaclasses and properties — correctly, because it's the
*same* lookup `ty` uses for its own diagnostics. `src/attrs.rs` in this
repo already has a passing test proving the key property this whole
chapter has been building toward:

```rust
// src/attrs.rs — member_answer_depends_on_which_instance_you_pass
let handler_handle = vnoc::member(db, &env, handler_instance, "handle").unwrap();
let loud_handle    = vnoc::member(db, &env, loud_instance,    "handle").unwrap();
assert_ne!(handler_handle.display(db, &env).to_string(),
           loud_handle.display(db, &env).to_string());
```

**the wrapper answers "what is `.handle` on *this* receiver's type" — it
never decides *which* receiver to pass.** That choice — "`self.handler` is,
on *this* path, a `LoudHandler`" — is entirely your `attrs` store and your
`OriginId`s, the parts of this whole folder that are actually yours to
write. Go read `experience/08-classes-and-mro/README.md`'s step 5 if you
haven't yet; it's the same wrapper, exercised from the other direction.

## Exercise

Using `python/inheritance.py`:

1. Trace `Child().go()`. At what point does `self.log` get its value, and
   whose `__init__` wrote it?
2. If `Child` had its own `log` attribute too (say, `Child.__init__` also
   did `self.log = SpecialLogger()` *after* calling `super().__init__()`),
   what would `go()` read — `Base`'s `Logger()` or `Child`'s
   `SpecialLogger()`? Why (tie this back to `03`'s "last write wins" rule)?
3. For `receiver_rules.py`'s `attribute_chain()`, how many separate
   `member()` calls does resolving the callee of
   `session.service.handler.handle(4)` require? Which one(s) hit `attrs`,
   and which hit the MRO walk?
4. In `deep_chain.py`'s `ambiguous_link`, `holder` has two possible values.
   What does `holder.service.handler.handle(3)` produce, and what bounds it?
5. In `unknown_mid_chain`, which hop yields `Unknown`, and what happens to the
   hops after it?

<details><summary>Answers</summary>

1. `self.log`'s value comes from `attrs[(O, "log")]`, where `O` is `Child()`'s
   own origin — written not by `Child.__init__` but by `Base.__init__`,
   reached via `super().__init__()`, which evaluates to
   `Instance{Base, O}` — **the same origin** as `self` in `Child.__init__`.
   `Base.__init__`'s `self.log = Logger()` therefore writes into exactly
   the slot `Child.go` later reads.
2. `Child`'s `SpecialLogger()` — because `record_self_assignments`/`record_bindings`
   is a single linear pass, last write wins, and the `self.log =
   SpecialLogger()` statement runs *after* `super().__init__()`'s write, in
   program order, inside the *same* `__init__` call (both writes share the
   same instance's origin, same as question 1). This is `03`'s "last write
   wins" rule, now shown to apply across a `super()` call boundary exactly
   as it would across two statements in one method.
3. Three `member()` calls: `(session, "service")`, `(result, "handler")`,
   `(result, "handle")`. The first two hit `attrs` directly (both
   `Session.__init__` and `Service.__init__` wrote exactly these keys on
   their respective instances). The third — `"handle"` — has no `attrs`
   entry for `Handler`'s instance (nothing ever assigned `self.handle =
   ...`), so it falls through to the MRO walk, which finds `handle` defined
   directly on the `Handler` class body.

4. Two `Session()` call sites mint two origins, `O_a` and `O_b`, so `holder`
   is a 2-value `Values`. Hop 1 gives two `Service` instances, hop 2 gives
   two `LoudHandler` instances (`Session.__init__`'s depth-2 write replaced
   `Service`'s `Handler()` on each), and hop 3 finds the same
   `LoudHandler.handle` for both. Two paths, **one distinct callee** after
   dedup, so the fan-out costs nothing against `MAX_CHAIN_FANOUT` (8). The
   cap only bites when distinct values pile up.
5. `thing.service`: `thing` is a bare parameter with no annotation, so it is
   `Unknown`. `Unknown.member("service")` is `Unknown`, and so on to the end.
   The call has no edge, and nothing panics or guesses.

</details>

---

With both `01-functions/` and `02-classes/` done, you've covered every case
this folder set out to teach. Do the whole-exercise exam next — it's
cross-cutting, drawing fixtures from both folders at once.

→ [`exam.md`](exam.md)
