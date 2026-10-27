# 03.07 — Bound methods, attribute chains, and MRO dispatch

Closing the folder: chains at any depth, bound methods carried as values,
`super()`, and the one piece of real, merged code this exercise has been
building toward: `vnoc::member`.

**Fixtures:** `python/receiver_rules.py` (`attribute_chain`),
`python/deep_chain.py`, `python/inheritance.py`. **Reading:**
[`plan/03-call-tree/07-attributes-and-self.md`](../../../plan/03-call-tree/07-attributes-and-self.md)
§ "Attribute chains", § "Inherited attributes and `super()`", § "`vnoc::member`",
and `src/attrs.rs` in this repo.

---

## Attribute chains: `member`, applied per hop

```python
session = Session(Service(Handler()))
session.service.handler.handle(4)
```

```
eval(session)                         → env hit → Instance{Session, S}
member(Instance{Session, S}, service) → attrs[(S, service)]  → Instance{Service, V}  (from Session.__init__'s effect)
member(Instance{Service, V}, handler) → attrs[(V, handler)]  → Instance{Handler, H}  (from Service.__init__'s effect)
member(Instance{Handler, H}, handle)  → no attrs entry → class side
                                        → BoundMethod{Handler.handle, receiver: H}
```

`self.a.b.c()` is the same, starting from `self`. Depth is not a feature of the
store: `eval` of an attribute recurses on its receiver expression, and each hop
is one `member` call.

- **Mid-chain `Unknown`**: `member` on `Unknown` is `Unknown`, so the chain
  dies quietly and the final call has no target (`unknown_mid_chain`).
- **Fan-out**: `member` returns `Values`, so an ambiguous link multiplies.
  `max_chain_fanout` (8) caps it per hop, after dedup, so two paths to the same
  object cost nothing (`ambiguous_link`).
- **Mixed hops**: each hop looks only at its own receiver, so
  `config.SERVICE.client.send()` (module → instance → instance) needs nothing
  extra. A module hop also finds submodules the program imported
  (`pkg.sub.mod.f()`).
- **Writes at depth** are [`05`](05-writes-anywhere-and-deep-writes.md).

## Bound methods as values

```python
run(fetch, self.handler.on_done)
```

`self.handler.on_done` is evaluated once, at the call site, to
`BoundMethod { on_done, receiver }`. The receiver is interned as a `ValueId`
([`00-values/02`](../00-values/02-origins.md)), because by the time `cb()` runs
inside `run`, the expression is long gone. Across the call it becomes
`Fp::Bound { recv }`, and a write the method makes on its receiver comes back as
an effect at `Param(0).Recv` ([`02-functions/06`](../02-functions/06-callbacks-and-higher-order.md)).

## `super()`: an ordinary call on the same receiver

```python
class Base:
    def __init__(self): self.log = Logger()
class Child(Base):
    def __init__(self):
        super().__init__()
        self.x = 1
    def go(self):
        self.log.write("hi")     # written by Base.__init__
```

`super().__init__` looks `__init__` up in the receiver's MRO after `Child`, and
binds it to **the same receiver**. So `super().__init__()` is a call to
`Base.__init__` with `self` = `Child.__init__`'s `self`, which is
`Entry(Param(0))`. `Base.__init__`'s effect `Param(0).log = Obj(0)` lands on
`Child.__init__`'s parameter 0, so `Child.__init__`'s own summary has
`Param(0).log` too. When `Child()` is built, the new object gets `log`, and
`go`'s key carries it.

Nothing about the attribute store knows about inheritance. It falls out of the
receiver being the same object. **[check]** what the Jedi driver produces for
`inheritance.py`; if it produces nothing, match it.

## Where "walk the MRO" stopped being your code

The class side of `member` (step 2) is not reimplemented. It is one wrapper,
merged in this repo:

```rust
// vendor/ruff/crates/ty_python_semantic/src/types/vnoc.rs
pub fn member<'db>(db: &'db dyn Db, env: &ProgramEnvironment<'db>, ty: Type<'db>, name: &str)
    -> Option<Type<'db>>
{
    ty.member(db, env, name).ignore_possibly_undefined()
}
```

ty's own `Type::member` handles the MRO, descriptors, `__getattr__`, metaclasses
and properties, correctly, because it's the lookup ty uses for its own
diagnostics. `src/attrs.rs` has a test proving the property this whole folder
relies on:

```rust
// src/attrs.rs — member_answer_depends_on_which_instance_you_pass
let handler_handle = vnoc::member(db, &env, handler_instance, "handle").unwrap();
let loud_handle    = vnoc::member(db, &env, loud_instance,    "handle").unwrap();
assert_ne!(handler_handle.display(db, &env).to_string(),
           loud_handle.display(db, &env).to_string());
```

**The wrapper answers "what is `.handle` on this receiver's type". It never
decides which receiver to pass.** "`self.handler` is a `LoudHandler` on this
path" is the env, the key and the origins: the parts that are yours to write.
And for a method it finds, `member` binds **your** receiver, not ty's, so the
method's walk sees the object's state.

---

## Exercise

1. `inheritance.py`: trace `Child().go()`. Whose `__init__` wrote `self.log`, and
   how did it reach `go`?
2. If `Child.__init__` also did `self.log = SpecialLogger()` after
   `super().__init__()`, what does `go()` read? Why?
3. `receiver_rules.py`, `attribute_chain()`: how many `member` calls for the
   callee of `session.service.handler.handle(4)`? Which hit the env, which the
   class side?
4. `deep_chain.py`, `ambiguous_link`: `holder` has two values. How many edges
   does `holder.service.handler.handle(3)` produce?
5. `deep_chain.py`, `unknown_mid_chain`: which hop gives `Unknown`?

<details><summary>Answers</summary>

1. `Base.__init__`, through `super().__init__()`, a call with the same receiver.
   Its effect on `Param(0).log` became `Child.__init__`'s effect, which landed
   on the new `Child` object; `go`'s key carried `log`.
2. `SpecialLogger`. In `Child.__init__`'s walk the two writes are sequential on
   one path: `Base.__init__`'s effect is applied first, then the later write
   replaces it. Assignment order is respected.
3. Three. `service` and `handler` hit the env (written by the constructors'
   effects); `handle` goes to the class side.
4. One. Two `Session()` sites give two origins, but both chains end at a
   `LoudHandler`, so after dedup there is one callee, `LoudHandler.handle`.
5. `thing.service`: `thing` is an unannotated parameter of an entry, so it is
   `Unknown`, and everything after it is too. No edge, nothing guessed.

</details>

---

With both case folders done, do the folder exam, then the whole-exercise exam.

→ [`exam.md`](exam.md), then [`../exam.md`](../exam.md)
