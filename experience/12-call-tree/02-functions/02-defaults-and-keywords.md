# 02.02 — Defaults and keyword arguments

Same machinery as the previous file — `bind_parameters`, which matches the
call's values to parameters before they become the callee's key — with the two
details that are easy to get backwards.

**Fixture:** `python/defaults.py`. **Reading:**
[`plan/03-call-tree/06-binding-arguments.md`](../../../plan/03-call-tree/06-binding-arguments.md)
§"Five things that are easy to get wrong", items 1–2 (item 1 is self-binding,
covered in `03-classes/02`; this file is item 2 and the keyword case).

---

## Keyword binding — matches by name, never by position

```python
def keyword_order(a, b=None): ...
keyword_order(b=Handler(), a=Handler())    # reversed order
```

`bind_parameters` iterates the **parameter list**, not the call's argument
list, and for each parameter checks the keyword map first:

```rust
slots.push(
    args.keyword(param.name()).cloned()                       // a keyword, by name
        .or_else(|| positional.next().cloned())               // else the next positional
        .or_else(|| default_value(s, w, func, param, defining)) // else the default
        .unwrap_or_else(|| smallvec![AbstractValue::Unknown]),
);
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
be; a default expression runs once, when the `def` statement runs, before
anyone has called the function. It belongs to **the scope that defined the
function**.

The walk that ran the `def` already evaluated the default and recorded any
calls in it. Binding never evaluates a call again. So `default_value` is:

- a module-level function: ty's type of the default expression, lifted
  (`DEFAULT` is a module global, so ty answers it);
- a nested `def`: a name is read from the closure's defining run
  (`def inner(h=h)`); anything else is ty's type, lifted.

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
   `call_with_default` is defined in. `DEFAULT` is a module global, so ty
   answers it: `h` is `Instance{Handler, OPAQUE}`, and `h.go()` finds
   `Handler.go` through the class.
2. No — `bind_parameters`'s logic checks keywords, then positionals, then
   falls to the default only when both are absent; `process(Handler())`
   supplies a positional, so the default branch never runs. Binding never
   evaluates a call in any case: a default's calls belong to the walk that
   ran the `def`. Evaluating one again at every call would record its call
   site in the wrong walk, once per caller.
3. `[a: Instance{Handler, New{site 2}}, b: Instance{Handler, New{site 1}}]`
   in the caller — two origins, one per `Handler()` site, matched by keyword
   name, with `b`'s value landing on `b` despite being written first. The key
   is `{a: Instance{Handler, []}, b: Instance{Handler, []}}`, and `alias` says
   they are two different objects.

</details>

---

→ Next: [`03-star-args-and-splats.md`](03-star-args-and-splats.md)
