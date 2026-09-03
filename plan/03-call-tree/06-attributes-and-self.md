# 03.06 — Attributes, `self`, and object state

You named this explicitly: *"it works for callback, attribute calls, stuff."*
This is the attribute-call chapter. It is the second-hardest thing after
argument binding, and the place where Option B
([`01-crates/04`](../01-crates/04-public-vs-private-api.md)) hurts most.

---

## The shape of the problem

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler          # ← state written here
        self.name = "pipe"

    def run(self, data):
        self.handler.process(data)      # ← read here, in a DIFFERENT method

def main():
    Pipeline(JsonHandler()).run(x)      # → JsonHandler.process
    Pipeline(CsvHandler()).run(y)       # → CsvHandler.process
```

Three separate mechanisms must cooperate:

1. **`self` is bound** to a specific instance (`bind_parameters`, done in [`05`](05-binding-arguments.md)).
2. **`__init__` writes** `self.handler` — that write must be *recorded*.
3. **`run` reads** `self.handler` — must see the write from *this* path's `__init__`.

Jedi gets this via `TreeInstance` holding the constructing `arguments`, so
attribute access re-infers `self.handler` in a context where `handler` is bound.

---

## The design: an attribute store keyed on origin

From [`04-value-domain.md`](04-value-domain.md), `Env` carries:

```rust
attrs: rpds::HashTrieMap<(OriginId, Name), Values<'db>>
```

`OriginId` identifies *which* instance. Two `Pipeline(...)` calls get different
origins, so their `handler` attributes never collide.

### Writing: scan `__init__` for `self.X = ...`

When you enter a class via its constructor, before walking the body normally:

```rust
fn record_self_assignments(
    &self,
    init: FunctionDef<'db>,
    init_env: &Env<'db>,
    instance_origin: OriginId,
) -> rpds::HashTrieMap<(OriginId, Name), Values<'db>> {
    let mut attrs = init_env.attrs.clone();
    let self_param = init.parameters(self.db).first()?.name();

    // Single linear pass over __init__'s body. No flow sensitivity:
    // later assignments simply overwrite earlier ones, matching the
    // "no branch analysis" limit from 03-call-tree/01.
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
        // also: AnnAssign (`self.x: T = v`), and walk into `if`/`try` bodies
        // one level — constructors commonly guard assignments.
    }
    attrs
}
```

**Deliberately not a fixed-point analysis.** One linear pass, last write wins,
descend into `if`/`try` bodies but don't model conditions. That matches the
"no branch analysis" scope limit from
[`01`](01-what-jedi-actually-does.md#what-it-does-not-do-scope-limits--respect-them)
and keeps this O(size of `__init__`).

### Reading: `member()` on a value

```rust
fn member(self, cx: &Cx<'db>, name: &str) -> Values<'db> {
    match self {
        AbstractValue::Instance { class, origin } => {
            // 1. ★ path-specific instance state — the precise answer
            if let Some(v) = cx.env.attrs.get(&(origin, name.into())) {
                return v.clone();
            }

            // 2. class body: methods, class vars — walk the MRO
            for base in class.iter_mro(cx.db, cx.env()) {
                if let Some(sym) = base.own_member(cx.db, name) {
                    return match sym {
                        Member::Function(f) => smallvec![AbstractValue::BoundMethod {
                            func: f,
                            receiver: cx.intern(self),   // ★ bind the receiver
                        }],
                        Member::Class(c) => smallvec![AbstractValue::Class(c)],
                        Member::Other(t)  => lift(cx.db, cx.env(), t),
                    };
                }
            }

            // 3. give up gracefully — ty knows about descriptors, __getattr__,
            //    dataclass fields, properties, slots, metaclass attrs
            cx.ty_member(Type::instance(class), name)
        }

        AbstractValue::Class(c)  => /* unbound: methods stay unbound */,
        AbstractValue::Module(m) => /* module-level symbol lookup */,
        AbstractValue::Ty(t)     => cx.ty_member(t, name),
        _                        => smallvec![AbstractValue::Unknown],
    }
}
```

**The order is the design.** Instance state (path-specific) beats class members
(path-free) beats ty (fully general). Each fallback loses precision and gains
coverage.

Step 2 producing a `BoundMethod` with an interned receiver is what makes
`self.handler.process(data)` work end to end: `self` → `Instance`,
`.handler` → step 1 hit → the `JsonHandler` instance, `.process` → step 2 →
`BoundMethod{process, receiver: that JsonHandler}`, and `bind_parameters` then
binds `self` correctly inside `process`.

---

## Attribute chains

`self.a.b.c()` is just `member` three times, each on the result of the last.
Because `member` returns `Values` (plural), a chain over an ambiguous link
fans out. **Cap the fan-out** — a chain where each link has 3 possibilities is
27 paths. `MAX_CHAIN_FANOUT` alongside `MAX_UNION_FANOUT` from
[`04`](04-value-domain.md).

---

## Attributes written outside `__init__`

```python
def configure(self):
    self.handler = OtherHandler()      # not in __init__
```

Options:
- **(a)** Only scan `__init__`. Simple, matches most code.
- **(b)** ★ When walking *any* method body with a bound `self`, record
  `self.X = ...` into the env going forward. The assignment scan is already
  written — just call it during `walk_body` too and let it extend the env for
  subsequent statements in that body.
- **(c)** Whole-class fixed point. No.

**Build (b).** It is a small delta on (a), and
[`10`](10-return-values-and-state.md#3-object-state-beyond-__init__)
generalises it further — to *any* receiver, not just `self`, so
`p.handler = X()` from outside the class works through the same code path.
Note it makes the env
mutable *during* a body walk, which conflicts with the "envs are immutable"
invariant from [`03`](03-the-abstract-interpreter.md). Resolve it by threading
the env through the statement loop by value:

```rust
let mut env = env.clone();      // cheap: rpds
for stmt in body {
    if let Some(updated) = self.record_assignments(stmt, &env) { env = updated; }
    self.visit_calls_in(stmt, &env, frame);
}
```

That keeps each env immutable once shared with a child frame, while allowing
sequential refinement within one body. Statement order becomes meaningful —
which is correct.

---

## Inherited attributes

```python
class Base:
    def __init__(self): self.log = Logger()
class Child(Base):
    def __init__(self): super().__init__(); self.x = 1
    def go(self): self.log.write("hi")     # ← from Base.__init__
```

Handle by making `super().__init__()` a normal call whose receiver is **the same
instance** (same `OriginId`). Then `Base.__init__`'s `self.log = Logger()` writes
into the same `(origin, "log")` slot and `Child.go` sees it. Nothing special
needed — it falls out of the design, *provided* `super()` resolves.

```rust
// eval: Call { func: Name("super"), args: [] } in a method body
//   → AbstractValue::Instance { class: <next in MRO>, origin: <same origin> }
```

**[check]** — Jedi handles `super()` via bound-method machinery; verify what your
current driver actually produces for the fixture above before deciding how much
to invest. If it produces nothing today, produce nothing.

---

## What this costs under Option B (no `pub(crate)` access)

Step 2 needs "members of this class, walking the MRO". The public surface gives
you `static_member_type_for_attribute(model, &ast::ExprAttribute)` — which takes
an **AST node** and infers the receiver itself. You cannot supply your own.

Workarounds, all bad:
- Walk the class body AST directly and match `def` names yourself. Loses
  properties, descriptors, dataclass fields, `__slots__`, metaclass attributes,
  and inherited C-level members.
- Use `definitions_for_attribute` on the real AST node, then filter the returned
  definitions by whether their enclosing class is in your value's MRO. **This
  actually works reasonably well** and is the best Option-B strategy — you let
  ty over-approximate, then narrow using your path knowledge.

The second one is worth knowing even under Option A: it is a good fallback for
step 3.

---

## Fixtures

```python
# 1. constructor-injected handler, two instances
Pipeline(JsonHandler()).run(x)
Pipeline(CsvHandler()).run(y)

# 2. two instances alive at once, distinct state
a = Pipeline(JsonHandler()); b = Pipeline(CsvHandler())
a.run(1); b.run(2)                     # must NOT cross-contaminate

# 3. attribute chain
self.client.session.send(req)

# 4. inherited attribute via super().__init__()
# 5. assignment outside __init__
# 6. @property returning a handler
# 7. classmethod constructor:  Pipeline.create(JsonHandler()).run(x)
# 8. attribute on a module:  config.HANDLER.process(x)
```

Fixture 2 is the one that proves `OriginId` is doing its job. If `a` and `b`
share state, your origin allocation is wrong. Make it the first test you write.

---

→ Next: [`07-callbacks-and-higher-order.md`](07-callbacks-and-higher-order.md)
→ Generalised (any receiver, any method, plus return-value flow):
  [`10-return-values-and-state.md`](10-return-values-and-state.md)
