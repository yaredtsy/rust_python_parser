# 02.05 — Assignment outside `__init__`, and external mutation

Two generalisations of [`03`](03-attribute-state-after-init.md), both
small deltas once local-assignment tracking exists
([`01-functions/05`](../01-functions/05-returns-and-value-flow.md)) — and
one deliberate refusal to generalise further.

**Fixture:** `python/external_mutation.py`. **Reading:**
[`plan/03-call-tree/06-attributes-and-self.md`](../../../plan/03-call-tree/06-attributes-and-self.md)
§"Attributes written outside `__init__`", and
[`plan/03-call-tree/10-return-values-and-state.md`](../../../plan/03-call-tree/10-return-values-and-state.md)
§"3. Object state beyond `__init__`".

---

## (a) A write inside a *different* method

```python
class Pipeline:
    def __init__(self): pass
    def configure(self):
        self.handler = OtherHandler()     # ← not in __init__
    def run(self, data):
        self.handler.process(data)
```

`03`'s `record_self_assignments` only scanned `__init__`. The generalised
rule: **when walking *any* method body with a bound `self`, record `self.X
= ...` into the attribute store going forward** — not as a special second
pass, but by reusing the exact same assignment-recording step that
`01-functions/05`'s `walk_body` already threads through every statement:

```rust
fn walk_body(&mut self, func, env: &Env<'db>, frame: &mut Frame<'db>) {
    let mut env = env.clone();
    for stmt in statements_in_scope(func) {
        for call in calls_in(stmt) { self.resolve_call(call, &env, frame); }
        env = self.record_bindings(stmt, env);   // ★ handles BOTH local AND attribute targets
    }
}
```

`record_bindings` already has to handle `Expr::Name` targets (plain local
assignment) for `01-functions/05` to work at all; handling `Expr::Attribute`
targets too is not new code, it's the **same match arm already present**,
not previously described as relevant to `self`. There is no `__init__`
special case to remove — there was never a need to add one in the first
place, once `record_bindings` exists.

## (b) A write from *outside* the class entirely

```python
p = Pipeline()
p.handler = JsonHandler()      # mutation from code that isn't even a method
p.run()
```

Same generalisation, one more step: **any receiver**, not just `self`.

```rust
fn bind_attribute(&mut self, attr: &ast::ExprAttribute, v: Values<'db>, env: Env<'db>) -> Env<'db> {
    for recv in self.eval(&attr.value, &env, model) {
        if let AbstractValue::Instance { origin, .. } = recv {
            return env.bind_attr(origin, &attr.attr.id, v);
        }
    }
    env    // unknown receiver → drop the write
}
```

`self.x = v` is now just the special case where `attr.value` happens to
evaluate to the receiver bound as `self` inside a method. `p.handler =
JsonHandler()` at module scope goes through the **identical** code path —
`self.eval(&attr.value, ...)` evaluates `p` (an ordinary name lookup,
whatever `env` has for it), and if that's a traced `Instance`, the write
lands in `attrs` keyed on *that instance's* origin, exactly like any
`__init__` write would.

## The one thing this does **not** attempt

> **Dropping writes to unknown receivers is deliberate.** If `self.eval(&attr.value, ...)`
> doesn't resolve to a traced `Instance` — say, the receiver is itself
> `Unknown` — the write is silently discarded, not applied speculatively to
> "something."

There is no sound way to model "some object of unidentified identity was
mutated" without invalidating potentially everything your attribute store
knows, and an unsound over-approximation here would corrupt unrelated
paths that happen to share a class. Losing one attribute write is a strictly
smaller cost than that.

## Exercise

Using `python/external_mutation.py`:

1. Trace `write_outside_init()`. At what point does `(origin, "handler")`
   first get an entry in `attrs` — is it before or after `p.configure()`
   returns? What does `p.run(1)` read?
2. Trace `write_from_outside_the_class()`. Is `bind_attribute`'s code path
   here any different from (1)'s, given that this write happens at module
   scope, outside any class body at all?
3. Modify the fixture mentally: `write_outside_init()`'s `p = Pipeline()`
   is replaced by `p = get_pipeline()` where `get_pipeline()` is some
   project function whose return value your interpreter, for whatever
   reason, can't determine (so `p` is `Unknown`). What happens to
   `p.configure()`'s write to `self.handler` in that case? (Hint: trace
   through what `self` is bound to inside `configure` when the receiver
   itself is `Unknown`, and re-read "dropping writes to unknown receivers.")

<details><summary>Answers</summary>

1. The entry appears when `p.configure()`'s body is walked — specifically,
   when `self.handler = OtherHandler()` is reached inside `configure` and
   `record_bindings` processes it. This happens entirely within the call to
   `configure()`, before `write_outside_init()` moves on to the next
   statement. `p.run(1)` then reads `(O_p, "handler") → Instance{OtherHandler, Oh}`,
   which was written moments earlier on the same path.
2. No difference at all — `bind_attribute` doesn't check *where* the
   assignment statement lives, only what `attr.value` (the receiver
   expression) evaluates to. `p` resolves to `Instance{Pipeline, O_p}`
   whether the assigning statement is inside a method or at module scope;
   the write lands in the same `attrs` map either way.
3. If `p` is `Unknown`, then calling `p.configure()` can't even resolve
   `configure` as a method to call in the first place (there's no instance
   to look `configure` up on via `member()`) — so this scenario doesn't
   reach the write at all; the call tree simply has no `configure` node
   under this path. If instead `p` resolved fine but `self` *inside*
   `configure` somehow ended up `Unknown` (a more contrived scenario),
   `bind_attribute`'s `for recv in self.eval(&attr.value, ...)` loop would
   find no `Instance` variant among the receiver's possibilities, and the
   write would be silently dropped — exactly the "unknown receiver" rule
   this file names explicitly.

</details>

---

→ Next: [`06-classmethod-staticmethod-property.md`](06-classmethod-staticmethod-property.md)
