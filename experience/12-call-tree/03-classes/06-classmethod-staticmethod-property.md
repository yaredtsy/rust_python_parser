# 03.06 — `@staticmethod`, `@classmethod`, and `@property`

Four receiver-binding rules total — plain method is the one you've already
built. This file is the other three, side by side, so the differences
stay sharp instead of blurring together.

**Fixture:** `python/receiver_rules.py`. **Reading:**
[`plan/03-call-tree/06-binding-arguments.md`](../../../plan/03-call-tree/06-binding-arguments.md)
§"5. Decorated functions" table.

---

## The table, restated as code

| decorator | receiver binding |
|---|---|
| none (plain method) | first param ← the instance |
| `@staticmethod` | **no receiver**; positionals start at param 0 |
| `@classmethod` | first param ← **the class**, not an instance |
| `@property` | accessed without `()` — see below |

```rust
fn receiver_for(decorators: &[Decorator], call_site: CallSite<'db>) -> Option<AbstractValue<'db>> {
    match decorator_kind(decorators) {
        None               => call_site.receiver,                    // ordinary method
        Some(Static)        => None,                                  // ★ staticmethod: drop it
        Some(Classmethod)   => Some(AbstractValue::Class(call_site.receiver_class)),  // ★ the CLASS
        Some(Property)       => call_site.receiver,                    // same as plain, invoked differently
    }
}
```

**`@staticmethod` is the one that actually changes `bind_parameters`'s
first step.** Everywhere else in this folder, step 1 of `bind_parameters`
("bind the receiver to the first parameter, *if* one was supplied")
assumes a receiver is coming; here, there explicitly isn't one, and
`static(x)`'s **first** parameter (`x`) is matched against the **first**
positional at the call site directly — not shifted by one the way
`02-self-and-method-binding.md`'s rule would otherwise apply. Detecting
`@staticmethod` and suppressing receiver-binding for that one call is a
precondition for `Service.static(1)` resolving at all, let alone
correctly.

**`@classmethod` substitutes a different *kind* of value as the receiver** —
`Class(Service)`, not `Instance{Service, ...}`. Inside `create`'s body,
`cls(handler)` then resolves through the ordinary `Callable::Class` path
from [`01-constructing-an-instance.md`](01-constructing-an-instance.md),
because `cls` is just a local bound to a `Class` value, and the call doesn't
care whether that value arrived via a literal class name or via a
classmethod's first parameter. (In `create`'s key, `cls` is `Fp::Class(Service)`,
so a subclass calling `create` gets its own context.)

## `@property` — a reachability question, not a binding question

```python
@property
def active_handler(self):
    return self.handler

s.active_handler.handle(3)
```

The receiver-binding rule for `active_handler` itself is identical to a
plain method's — `self` binds to the instance. What's different is *when*
it gets invoked: `s.active_handler` — no parentheses — is still a call,
disguised as an attribute access. When `member()` finds a `@property` on the
class side, it calls the getter right there: a project call with `self` bound
to the receiver, keyed, answered by the getter's summary, and recorded as an
edge at the attribute expression's site. The value is the getter's returns
(`plan/03-call-tree/07` § Properties), rather than a `BoundMethod` waiting for
a `(...)` the source never contains.

Miss this, and `s.active_handler.handle(3)` would try to call `.handle` on
a `BoundMethod` value (the getter itself, uninvoked) instead of on
whatever the getter *returns* — a `TypeError` in real Python, and a dead
end in your tree.

## Attribute chains, briefly — the next file's subject

`receiver_rules.py` also has `attribute_chain()`:
`session.service.handler.handle(4)` — three `member()` calls in sequence,
each one's receiver being the previous one's result. That's
[`07-bound-methods-chains-and-mro-dispatch.md`](07-bound-methods-chains-and-mro-dispatch.md)'s
subject; this file only needed `@property` to make sense of one link in
such a chain.

## Exercise

Using `python/receiver_rules.py`:

1. Write `bind_parameters`'s parameter values for `static_call()`'s call to
   `Service.static(1)`.
2. Write it for `classmethod_call()`'s call to `Service.create(Handler())`.
   What's `cls` bound to inside `create`'s body, and what does `cls(handler)`
   then construct?
3. `property_access()` calls `s.active_handler.handle(3)`. At what point
   does the getter's body (`return self.handler`) actually get walked —
   is it when `s.active_handler` is evaluated, or only once `.handle(3)` is
   applied to the result?

<details><summary>Answers</summary>

1. `{ x → Ty(int) }` — no `self`/receiver entry at all. `x` is matched
   directly against the first (and only) parameter, because
   `@staticmethod` suppresses receiver-binding entirely.
2. `[cls ← Class(Service), handler ← Instance{Handler}]`. Inside
   `create`'s body, `cls` holds `AbstractValue::Class(Service)`, so
   `cls(handler)` resolves through the ordinary constructor path —
   `Callable::Class(Service)` — minting a `New` origin at that site, exactly
   as if the source had written `Service(handler)` directly.
3. When `s.active_handler` is evaluated: `member()` finds the property and
   calls the getter there (walked the first time this key is seen, reused
   after that). Its summary returns `Ref(Param(0).handler)`, which
   resolves to whatever `s.handler` holds — the `Handler` — as the value of
   `s.active_handler`, *before* `.handle(3)` is applied to it. `.handle(3)` then runs as an
   ordinary method call on that instance, same as any other.

</details>

---

→ Next: [`07-bound-methods-chains-and-mro-dispatch.md`](07-bound-methods-chains-and-mro-dispatch.md)
