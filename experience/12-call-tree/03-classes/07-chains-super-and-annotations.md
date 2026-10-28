# 03.07 — Chains, `super()` and annotations

**You build:** three small things that finish objects. Attribute chains of any
depth need nothing new, as long as each hop keeps its values apart. `super()`
calls the parent's method on the same object. And a parameter that has no
value from a caller starts from its annotation.
**Fixture:** `python/chains.py`
**Plan:** `plan/03-call-tree/07-attributes-and-self.md` § "Attribute chains",
§ "Inherited attributes and `super()`", `08-keys.md` § "The annotation fallback"

---

## The fixture

```python
class Service:
    def __init__(self, handler: Handler): self.handler = handler
    def call(self): self.handler.handle()
class Session:
    def __init__(self, service): self.service = service
class Base:
    def __init__(self, h): self.h = h
class Child(Base):
    def __init__(self, h): super().__init__(h)
    def go(self): self.h.handle()

def chain():
    s = Session(Service(LoudHandler()))
    s.service.handler.handle()            # LoudHandler.handle
def write_then_read():
    s = Session(Service(Handler()))
    s.service.handler = LoudHandler()
    s.service.handler.handle()            # LoudHandler.handle
def unknown_mid_chain(thing):
    thing.service.handler.handle()        # nothing
def inherited():
    Child(LoudHandler()).go()             # Child > Base.__init__; Child.go > LoudHandler.handle
def annotated(h: Handler):
    h.handle()                            # Handler.handle

# and the entry `Service.call`: Service.call > Handler.handle
```

## Predict

1. `s.service.handler.handle()`: how many `member` calls, and which ones hit
   the object's state?
2. In `inherited`, `Base.__init__` writes `self.h`. Which object is `self`
   there? What happens to `go` if it is a different one?
3. `Service.call` is the entry: nobody called it, so `self` has no value. What
   should `self` be?

---

## Build

### Decision 1: a chain is one `member` per hop

`s.service.handler.handle` is three attribute expressions, one inside the
other. `eval_attribute` evaluates the receiver first, so depth needs no code of
its own (step 01.04): `member(s, service)` hits state, `member(that, handler)`
hits state, `member(that, handle)` goes to the class side. A write at depth,
`s.service.handler = X`, evaluates `s.service` and writes `handler` on it
(step 03, decision 3).

- **Mid-chain `Unknown`:** `member(Unknown, _)` is `[Unknown]`, so the rest of
  the chain stays `Unknown`, and the final call has no target. Nothing is
  guessed.
- **Fan-out:** a hop on several objects returns several values, and the next
  hop multiplies. Cap the values carried per hop at `max_chain_fanout` (8),
  after deduplication, and count each time it fires. Two paths to one object
  cost nothing.

### Decision 2: `super()` binds the same receiver

```python
class Child(Base):
    def __init__(self, h):
        super().__init__(h)
```

`super().__init__` is `Base.__init__`, bound to **the same object** `Child.__init__`
was called on. Its write to `self.h` lands on the `Child` object, and `go`
finds it.

Without care it goes wrong quietly: `super()` is a call to a builtin, so its
value is ty's type (a `super` object), and `.__init__` on it gives ty's bound
method, with ty's receiver: an `OPAQUE` `Child`. `Base.__init__`'s write is
dropped, and `go` finds nothing.

```rust
// eval_attribute, before evaluating the receiver
if is_zero_arg_super(&attr.value) {
    let t = s.sem.type_of(&self.frame.model, ExprRef::from(attr));   // ty: which function
    if let Shape::BoundMethod { func, .. } = s.sem.shape(t, self.frame.file) {
        let me = self.first_parameter_value(s, env);                   // this method's `self`
        return me.into_iter()
            .map(|r| AbstractValue::BoundMethod { func, receiver: self.values.intern(r) })
            .collect();
    }
}
```

**Why syntax here, when step 01.12 said dispatch on values:** zero-argument
`super()` means "the class this method is written in, and this method's first
argument". Both come from *where the expression is written*, not from a value
it evaluates to. It is the one call whose meaning is lexical.

ty finds which `__init__` (the MRO after `Child`). You bind the receiver, as
always.

**[check]** what the Jedi driver shows for `super().__init__(h)`. If it shows
nothing, record the edge (the write must still land) and skip the node.

### Decision 3: a parameter with no value starts from its annotation

An entry's parameters have no caller. Neither does a parameter whose argument
was missing. Both are `[Unknown]`. If the parameter is annotated, start it
from the annotation instead, lifted:

```rust
if v == [Unknown] {
    if let Some(ann) = &param.annotation {
        let t = s.sem.type_of(&self.frame.model, ExprRef::from(&**ann));   // [check] annotation type, not value
        v = self.reorigin(self.lift(s, t), OriginKey::Entry(i));
        s.stats.annotation_fallbacks += 1;
    }
}
```

And an unannotated `self` or `cls` of an entry method uses the enclosing
class: `Service.call`'s `self` is a `Service`.

**Why `Entry(i)` and not `OPAQUE`:** `lift` gives an annotated parameter an
`OPAQUE` instance, which holds no state. But it is one specific object, the
one passed as parameter `i`, so writes to it inside this walk should be
tracked. `OriginKey::Entry(i)` gives it an origin of its own. Part 04 builds
on exactly this: every object a summary receives from its caller is named by
where it sits among the inputs.

**Why it is counted:** an annotation is a claim, not a proof.
`def process(h: Handler)` may be called with a `LoudHandler`, and the edge
says `Handler.handle`. A leaf that came from an annotation should be
explainable as such, by a counter.

`Service.call` as an entry: `self` is a `Service` from the class; `self.handler`
has no state (nothing wrote it in this walk), so it goes to the class side,
where ty knows the attribute's declared type from `__init__`'s annotation:
`Handler`. The edge is `Handler.handle`. In `chain`, the same method on an
object built with a `LoudHandler` finds the state first, and the edge is
`LoudHandler.handle`.

### Yours

- `max_chain_fanout` in `eval_attribute`, with a counter.
- The `super()` case.
- `OriginKey::Entry(u8)`, the annotation fallback in binding, the enclosing
  class for an unannotated entry `self` / `cls`, and
  `Stats.annotation_fallbacks`.

<details><summary>Reference: this step's code</summary>

```rust
/// `super()` with no arguments.
fn is_zero_arg_super(e: &Expr) -> bool {
    matches!(e, Expr::Call(c)
        if c.arguments.is_empty()
        && matches!(&*c.func, Expr::Name(n) if n.id.as_str() == "super"))
}

/// The value of this method's first parameter, from the env.
fn first_parameter_value(&self, s: &Solver<'db>, env: &Env<'db>) -> Values<'db> {
    let parsed = s.sem.parsed(self.frame.func);
    let Some(node) = s.sem.function_node(self.frame.func, &parsed) else { return Values::new() };
    let Some(first) = node.parameters.iter_non_variadic_params().next() else { return Values::new() };
    env.get(&first.parameter.name.id).cloned().unwrap_or_default()
}

/// Give every OPAQUE instance in `vals` the given origin.
fn reorigin(&mut self, vals: Values<'db>, key: OriginKey) -> Values<'db> {
    let origin = self.origins.intern(key);
    vals.into_iter().map(|v| match v {
        AbstractValue::Instance { class, origin: o } if o == OriginId::OPAQUE =>
            AbstractValue::Instance { class, origin },
        other => other,
    }).collect()
}
```

The annotation is a type expression, so `type_of` on it gives the type of the
*expression* `Handler`, which is the class, not an instance. **[check]** the
right accessor: ty's declared type of the parameter (its `Definition`'s
declared type), or `instance_of` on the class.

</details>

---

## Run

```rust
#[test]
fn p03_07_chains() {
    check("03-classes/python/chains.py");
}
```

---

## Where you are: the end of part 03

```
AbstractValue = Function | Closure | Class | Instance { class, origin } | BoundMethod
              | Module | Ty | Pending | Unknown
OriginKey     = New { site, act } | Entry(param)          OriginId::OPAQUE reserved
Walk          { frame, activations, origins, values }
Env           { locals, attrs, scope, enclosing }
member(Instance): state → property → class side (ty), binding your receiver
bind_attribute:   strong for one object, weak for several, dropped for none or OPAQUE
```

Inline mode now handles the object-oriented code real projects are made of.
And it walks it exponentially: the wall of step 01.13, with classes. Part 04
brings summary mode to the same trees.

---

## Exercise

1. `a.b.c.f()` where `a.b` may be 3 objects, each `.c` may be 3 objects, and
   `max_chain_fanout` is 8. How many values reach `.f`, and what is counted?
2. A grandchild class: `class G(Child): def __init__(self, h): super().__init__(h)`.
   Trace `G(x).go()`: which `__init__`s run, on which object?
3. `def f(h: Handler): h.handler2 = X(); h.handler2.go()`. With `OPAQUE` for
   `h`, what is lost? With `Entry(0)`?

<details><summary>Answers</summary>

1. At most 8. The first hop gives 3; the second would give 9 after
   deduplication, so it stops at 8 and counts one `chain_fanout_hits`. If some
   of the 9 were the same object, deduplication may keep it under the cap.
2. `G.__init__` runs on the new `G` object. Its `super().__init__` is
   `Child.__init__` (the MRO after `G`), bound to the same object. Its
   `super().__init__` is `Base.__init__`, again on the same object. `self.h`
   lands on the `G` object, and `go` finds it.
3. With `OPAQUE`, the write is dropped, and `h.handler2.go()` falls to the
   class side, which probably knows no `handler2`: no edge. With `Entry(0)`,
   the write is kept on `h`'s origin, and `X.go` is found.

</details>

---

→ Next part: [`../04-class-summaries/README.md`](../04-class-summaries/README.md)
