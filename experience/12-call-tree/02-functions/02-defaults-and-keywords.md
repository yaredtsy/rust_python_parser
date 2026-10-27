# 01.02 — Defaults and keyword arguments

Same machinery as the previous file — `bind_parameters` — with the two
details that are easy to get backwards.

**Fixture:** `python/defaults.py`. **Reading:**
[`plan/03-call-tree/05-binding-arguments.md`](../../../plan/03-call-tree/05-binding-arguments.md)
§"Five things that are easy to get wrong", items 1–2 (item 1 is self-binding,
covered in `02-classes/02`; this file is items 2 and the keyword case).

---

## Keyword binding — matches by name, never by position

```python
def keyword_order(a, b=None): ...
keyword_order(b=Handler(), a=Handler())    # reversed order
```

`bind_parameters` iterates the **parameter list**, not the call's argument
list, and for each parameter checks the keyword map first:

```rust
if let Some(v) = args.keywords.get(param.name()) {
    locals.insert_mut(param.name().clone(), v.clone());
} else if let Some(v) = positional.next() {
    locals.insert_mut(param.name().clone(), v.clone());
} else if let Some(default) = param.default() {
    ...
}
```

Written-order of the keywords at the call site is irrelevant — `b=` and
`a=` land correctly regardless of which one appears first in the source.
This is the "easy" half; the trap is the other one.

## ★ The trap: defaults evaluate in the DEFINING scope

```python
DEFAULT = Handler()

def process(h=DEFAULT):
    h.go()
```

`DEFAULT` is not looked up in the **caller's** environment — it never could
be; a default expression is only ever evaluated once, when nobody has
called the function yet. It must be evaluated against **the module scope of
the function's own definition**:

```rust
let v = self.eval(default, &self.env_of_defining_scope(func));
```

Get this wrong — evaluate the default in the *caller's* env instead — and
the failure mode is the one you'll never notice while testing: `env.lookup("DEFAULT")`
quietly misses (because the caller's locals don't have that name), falls
through to `ty`, and `ty` usually *also* answers correctly for a
module-level constant like this — so the bug hides until a caller happens
to shadow the name `DEFAULT` with something unrelated of its own. Then
`process()`'s default silently resolves to the caller's `DEFAULT` instead of
the real one.

## Exercise

Using `python/defaults.py`:

1. `call_with_default()` calls `process()` with no arguments. Which
   environment is `DEFAULT` evaluated in, and what's bound to `h` inside
   `process`?
2. `call_with_override()` calls `process(Handler())`. Does the default
   expression get evaluated at all on this call? Why would evaluating it
   anyway (even though its result is discarded) be a correctness bug, not
   just wasted work?
3. For `call_keywords_out_of_order()`, write out `bind_parameters`'s locals
   map after it finishes — which name gets which value.

<details><summary>Answers</summary>

1. The defining scope — `defaults.py`'s module scope, the same place
   `DEFAULT = Handler()` itself lives — regardless of which module
   `call_with_default` is defined in. `h` is bound to that one
   `Handler()` instance (`AbstractValue::Instance{Handler, O_default}`,
   where `O_default` is the origin minted once for `DEFAULT = Handler()`
   at module load, not a fresh one per call).
2. No — `bind_parameters`'s logic checks keywords, then positionals, then
   falls to the default only when both are absent; `process(Handler())`
   supplies a positional, so the default branch never runs. Evaluating it
   anyway would be a correctness bug (not just waste) if the default
   expression had a side effect your interpreter modelled — e.g. a call
   that writes to `attrs` — since that write would then happen on every
   call, including ones that explicitly overrode the default and should
   never have triggered it.
3. `{ a → Instance{Handler, O1}, b → Instance{Handler, O2} }` — two fresh
   origins, one per `Handler()` construction at the call site, bound by
   keyword name, with `b`'s value landing correctly on `b` despite being
   written first in the source.

</details>

---

→ Next: [`03-star-args-and-splats.md`](03-star-args-and-splats.md)
