# 02.03 — Attribute state after `__init__`

The one genuinely new piece of machinery in this whole project — everything
else so far has been binding and walking. This is the chapter your project
owner named explicitly as the hard case: *"it works for callback, attribute
calls, stuff."*

**Fixture:** `python/pipeline.py`. **Reading:**
[`plan/03-call-tree/06-attributes-and-self.md`](../../../plan/03-call-tree/06-attributes-and-self.md)
§"The design: an attribute store keyed on origin" and §"Writing".

---

## The shape, restated

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler        # ← WRITE here
    def run(self, data):
        self.handler.process(data)    # ← READ here, in a DIFFERENT method
```

Three mechanisms cooperate, and all three already exist in pieces you've
built:

1. **`self` is bound** to a specific instance — `02-self-and-method-binding.md`.
2. **`__init__` writes** `self.handler` — needs to be *recorded* somewhere
   that outlives `__init__`'s own call frame. ★ new, this file.
3. **`run` reads** `self.handler` — needs to see the write from *this
   path's* `__init__`, specifically. ★ new, this file.

## Where the write lives: `Env.attrs`, keyed on `OriginId`

Recall from [`00-values/03-env-and-scope.md`](../00-values/03-env-and-scope.md):

```rust
attrs: rpds::HashTrieMap<(OriginId, Name), Values<'db>>
```

Keyed on `OriginId`, **not** on scope — this is the field that's shared
*across* a whole path, unlike `locals`, which is private to one activation.
`Env::clone()` when entering `run`'s frame carries `attrs` forward
unchanged; only `locals` starts fresh.

## Writing: one linear pass over `__init__`'s body

```rust
fn record_self_assignments(
    &self, init: FunctionDef<'db>, init_env: &Env<'db>, instance_origin: OriginId,
) -> rpds::HashTrieMap<(OriginId, Name), Values<'db>> {
    let mut attrs = init_env.attrs.clone();
    let self_param = init.parameters(self.db).first()?.name();

    for stmt in body_statements(init) {
        if let ast::Stmt::Assign(assign) = stmt {
            for target in &assign.targets {
                if let ast::Expr::Attribute(attr) = target {
                    if is_name(&attr.value, self_param) {
                        let v = self.eval(&assign.value, init_env, model);
                        attrs.insert_mut((instance_origin, attr.attr.id.clone()), v);
                    }
                }
            }
        }
    }
    attrs
}
```

**Deliberately not a fixed-point analysis.** One linear pass, last write
wins, matching the "no branch analysis" scope limit from `00-values/01`'s
domain design — this is O(size of `__init__`), not O(size of the program).

## Reading: `member()`'s priority order

```rust
fn member(self, cx: &Cx<'db>, name: &str) -> Values<'db> {
    match self {
        AbstractValue::Instance { class, origin } => {
            if let Some(v) = cx.env.attrs.get(&(origin, name.into())) {
                return v.clone();                          // 1. path-specific — the precise answer
            }
            for base in class.iter_mro(cx.db, cx.env()) {    // 2. class body, walking the MRO
                if let Some(sym) = base.own_member(cx.db, name) {
                    return /* ... BoundMethod, Class, or lift() ... */;
                }
            }
            cx.ty_member(Type::instance(class), name)        // 3. ty: descriptors, __getattr__, etc.
        }
        // ...
    }
}
```

**The order is the design.** Instance state beats class members beats
`ty`'s full generality — each fallback trades precision for coverage.
Tracing `self.handler.process(data)` inside `run`: `self` is already
`Instance{Pipeline, O1}` in `run`'s `locals` (bound the same way `self` is
bound anywhere, `02-classes/02`); `.handler` hits tier 1 of `member`,
because `__init__` wrote exactly that key when `Pipeline(...)` was
constructed; `.process` is then a **second** `member()` call, this time on
whatever `.handler` resolved to, hitting tier 2 (walking `JsonHandler`'s
MRO for a method named `process`).

## Exercise

Using `python/pipeline.py`'s `main()`:

1. At what point, in what order, does `attrs` get its one entry — before or
   after `__init__`'s body finishes walking? Does `name = "pipe"` (the
   second assignment in `__init__`) also produce an `attrs` entry? Should
   it?
2. Trace `self.handler.process(data)` inside `run` using `member()`'s three
   tiers above. Which tier answers for `.handler`? Which for `.process`?
3. Suppose `JsonHandler` defined `process` as a `@property` instead of a
   method. Which tier of `member()` would still find it, and what would
   change about how the result gets used afterward (hint: re-read
   `receiver_rules.py`'s `@property` note once you reach
   [`06-classmethod-staticmethod-property.md`](06-classmethod-staticmethod-property.md))?

<details><summary>Answers</summary>

1. During — each assignment statement in the linear pass updates `attrs`
   as it's encountered, so by the time the pass finishes, both `self.handler`
   and `self.name` have entries (`record_self_assignments` doesn't special-case
   which attribute name it is — every `self.X = ...` assignment is recorded).
   Yes, `name = "pipe"` produces an entry too: `(O1, "name") → Ty(str)` —
   there's no reason to special-case it; the mechanism records every
   attribute write uniformly.
2. `.handler` is answered by **tier 1** — `(O1, "handler")` is a direct hit
   in `attrs`, written by `__init__` on this exact instance. `.process` is
   answered by **tier 2** — `JsonHandler` (whatever `.handler` resolved to)
   has no per-instance `attrs` entry for `process` (nothing ever assigned
   `self.process = ...`), so `member()` falls through to walking
   `JsonHandler`'s MRO and finds `process` defined directly on the class.
3. Still tier 2 — `own_member` finds `process` on the class body regardless
   of whether it's a plain method or a property; `member()` doesn't
   special-case properties itself (that's `ty`'s job, via tier 3's
   `cx.ty_member` for anything tier 2 doesn't resolve plainly, or via how
   the *caller* of `member()` decides to invoke what tier 2 returns). What
   changes is downstream: a property's "value" is the result of *calling*
   its getter, not the function object itself — so wherever `eval`
   processes an `Expr::Attribute`, it needs to know this access should
   trigger an implicit call rather than just producing a `BoundMethod` to
   be called later. That distinction is exactly what
   [`06-classmethod-staticmethod-property.md`](06-classmethod-staticmethod-property.md)
   covers.

</details>

---

→ Next: [`04-two-instances-and-origin-isolation.md`](04-two-instances-and-origin-isolation.md)
