# 03.02 — `self` and method binding

The single easiest binding bug to write, and the one that makes every
argument in every method call look shifted by one when you get it wrong.

**Fixture:** `python/self_binding.py`. **Reading:**
[`plan/03-call-tree/06-binding-arguments.md`](../../../plan/03-call-tree/06-binding-arguments.md)
§"Five things that are easy to get wrong", item 1.

---

## The rule

> **Bind the receiver to the *first* parameter, then match remaining
> positionals against the *rest*.**

```rust
let mut params_iter = params.iter();
if let Some(recv) = receiver {
    if let Some(first) = params_iter.next() {
        slots.push(smallvec![recv]);                   // slot 0: the receiver
    }
}
for param in params_iter.by_ref() {
    // ordinary positional/keyword/default binding, same as any function
}
```

The subtlety is entirely in the *order* of operations: consume `receiver`
first, advance the parameter iterator past `self`, **then** start consuming
positionals from the call site against whatever parameters are left. Get
the order backwards — try to match positionals against the full parameter
list including `self` — and every real argument silently lands one
parameter too late.

## Bound call vs. unbound call — the same rule, two syntaxes

```python
Greeter().greet(Payload())              # bound:   self ← Greeter(), name ← Payload()
Greeter.greet(Greeter(), Payload())      # unbound: self ← Greeter() EXPLICITLY, name ← Payload()
```

These produce **identical bindings**. The difference is only in *how* the
receiver arrives at the call:

- **Bound** (`obj.method(...)`): `eval` of the `Attribute` expression
  (`Greeter().greet`) goes through `member()`
  ([`07-bound-methods-chains-and-mro-dispatch.md`](07-bound-methods-chains-and-mro-dispatch.md)),
  which produces `BoundMethod { func, receiver }` with the receiver already
  packaged in. `bind_parameters` is called with `receiver = Some(that value)`.
- **Unbound** (`Cls.method(obj, ...)`): `Greeter.greet` resolves to a plain
  `Function` (no receiver attached — you looked it up straight off the
  class), so `receiver = None`. But then `Greeter()` is just the *first
  positional argument* at the call site, and because the parameter list's
  first entry (`self`) was never pre-filled by a receiver, it's filled by
  the ordinary positional-matching loop instead — landing `Greeter()` on
  `self` anyway, by coincidence of position, not by any receiver-binding
  code path at all.

Both reach the same locals map by genuinely different routes. Getting the
*unbound* case right is actually a good check that your receiver-binding
code isn't accidentally doing something special for `self` by *name* —
it should only ever be about *position*, which the `Cls.method(obj, ...)`
form exercises without any receiver value ever being passed in explicitly.

## Exercise

Using `python/self_binding.py`:

1. Write `bind_parameters`'s resulting locals map for `call_bound()`.
2. Write it for `call_unbound()`. Confirm it's identical to (1) even though
   `receiver` was `None` on this call.
3. Suppose `bind_parameters` had a bug where it bound positionals against
   the **full** parameter list (including `self`) before considering the
   receiver at all. Trace `call_bound()` through that buggy version — what
   does `name` end up bound to, and what's left over for `self`?

<details><summary>Answers</summary>

1. `{ self → Instance{Greeter, O1}, name → Instance{Payload, O2} }`.
2. Same: `{ self → Instance{Greeter, O1}, name → Instance{Payload, O2} }`
   — `Greeter()` (the first positional at the call site) fills the first
   remaining parameter slot, which is `self`, purely because nothing had
   already claimed that slot via the receiver path.
3. `name` would be bound to `Instance{Greeter, O1}` (the first positional,
   matched against the first parameter in the full list, which is `self` —
   but the buggy version is matching *by position in the full list*, so
   whatever parameter is textually first gets whatever argument is
   textually first) and `self` would then try to also claim a slot from
   whatever comes next — with only one positional (`Payload()`) left and
   two parameter slots (`self`, `name`) still open if the receiver is
   *also* separately supplied, you'd get either a silent `Unknown` for one
   of them or, worse, `Payload()` incorrectly landing on `self`. The
   precise wrong answer depends on implementation details, but the general
   shape — **everything shifts by one slot** — is the signature of this
   exact bug class, and it's why the rule is stated as an explicit
   ordering rather than left as "do the obvious thing."

</details>

---

→ Next: [`03-attribute-state.md`](03-attribute-state.md)
