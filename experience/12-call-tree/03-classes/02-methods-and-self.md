# 03.02 — Methods and `self`

**You build:** `obj.method(...)`. ty finds which function `.method` is on the
object's class, through the MRO. The walker keeps that function, and binds
**its own** object as the receiver.
**Fixture:** `python/methods.py`
**Plan:** `plan/03-call-tree/07-attributes-and-self.md` § "Reading: `member`",
§ "Bind your receiver, not ty's", `06-binding-arguments.md` § "Bound call and
unbound call"

---

## The fixture

```python
class Greeter:
    def greet(self, cb): cb()
    def twice(self, cb):
        self.greet(cb)
        self.greet(cb)
class Loud(Greeter): pass
class Quiet(Greeter):
    def greet(self, cb): pass

def main():
    g = Greeter()
    g.greet(on_ok)              # Greeter.greet > on_ok
    Greeter.greet(g, on_err)    # Greeter.greet > on_err
    g.twice(on_ok)              # Greeter.twice > Greeter.greet > on_ok, twice
    Loud().greet(on_err)        # Loud, then Greeter.greet > on_err
    Quiet().greet(on_ok)        # Quiet, then Quiet.greet
    m = g.greet
    m(on_err)                   # Greeter.greet > on_err
```

## Predict

1. `g.greet(on_ok)` passes one argument. `greet` has two parameters. Which
   one gets `on_ok`, and what puts `g` in the other?
2. `Greeter.greet(g, on_err)` passes two. Same binding, or different?
3. Inside `twice`, `self.greet(cb)`: what is `self`, and how does the walker
   know `.greet` is `Greeter.greet` and not `Quiet.greet`?

---

## Build

### Decision 1: ty answers "what is `.name` on this class"

```rust
// member(), the Instance arm
AbstractValue::Instance { class, .. } => {
    let t = s.sem.member(s.sem.instance_type(class), name, self.frame.file);
    // …
}
```

`vnoc::member` on the instance's type is ty's own attribute lookup: the MRO,
overrides, descriptors, `__getattr__`, metaclasses. `Loud().greet` finds
`Greeter.greet` through inheritance; `Quiet().greet` finds the override. You
write none of that.

`sem.instance_type(class)` is the `vnoc::instance_of` wrapper (already in the
repository) on `sem.class_type(class)`.

**Why not walk the MRO yourself:** a second copy of Python's attribute rules
would disagree with ty exactly where ty is right. Exercise 08 made the
decision: one wrapper, no logic, in the fork.

### Decision 2: bind your receiver, not ty's

For a method, ty answers with a bound method whose receiver is ty's idea of
the object: "some `Greeter`", a type. Lifting that would lose which object
the method was called on, and inside the method `self` would be an object you
know nothing about. In step 03 that means `self.handler` finds nothing.

So keep only the function, and bind the object `member` was called on:

```rust
match t.map(|t| s.sem.shape(t, self.frame.file)) {
    Some(Shape::BoundMethod(func)) => {
        let receiver = self.values.intern(recv);        // OUR object
        smallvec![AbstractValue::BoundMethod { func, receiver }]
    }
    Some(_) => self.lift(s, t.unwrap()),                // a class variable, a declared attribute
    None => smallvec![AbstractValue::Unknown],          // no such attribute
}
```

```rust
pub enum AbstractValue<'db> {
    // …
    BoundMethod { func: Definition<'db>, receiver: ValueId },   // ← new
}
```

### Decision 3: the receiver is interned, so values stay `Copy`

`BoundMethod` needs its receiver, which is itself an `AbstractValue`. A `Box`
would make the enum non-`Copy`, and values are copied constantly (into envs,
through joins, into arguments). So receivers are interned in a per-walk arena,
and the method holds a `ValueId`:

```rust
pub struct ValueArena<'db> {
    values: IndexVec<ValueId, AbstractValue<'db>>,
    dedup: FxHashMap<AbstractValue<'db>, ValueId>,
}
```

The same receiver is interned once: `self.greet` twice in `twice` gives the
same `ValueId`.

### Decision 4: the receiver goes to the first parameter, by position

Calling `BoundMethod { func, receiver }` passes `receiver` as the receiver of
step 01's `bind_parameters`: it goes to the **first** parameter, and the
call's positionals are matched against the rest.

**The off-by-one bug:** binding the call's first positional to the first
parameter, then the receiver somewhere else, shifts every argument of every
method call by one. `g.greet(on_ok)` would bind `self = on_ok` and `cb =
Unknown`, and the tree loses the edge. It looks like broken inference, not
like a binding bug, which is why the fixture tests it first.

`Greeter.greet(g, on_err)` is an **unbound** call: `member` on the `Class`
value gives the plain function (`Function(greet)`, no receiver), and `g`
lands on `self` as the first positional. Same binding, different route. The
receiver rule is about position, never about the name `self`.

### Decision 5: a bound method is a value like any other

`m = g.greet` binds `m` to the `BoundMethod`. `m(on_err)` calls it later, with
`g` as the receiver. Nothing new: `eval_call` dispatches on the value
(step 01.12).

### Yours

- `BoundMethod`, `ValueId`, `ValueArena` in the walk.
- `Shape::BoundMethod(func)` in `sem::shape`.
- `member` for `Instance` and `Class` values.
- `eval_call`: a `BoundMethod` callee is `walk_inline` with the receiver, plus
  a node for the method.
- `lift` keeps a bound method ty hands you (one you didn't get through
  `member`) as `Ty` for now. Step 04 gives it a receiver.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/member.rs
fn member(&mut self, s: &mut Solver<'db>, recv: AbstractValue<'db>, name: &Name,
          attr: &ExprAttribute, env: &mut Env<'db>) -> Values<'db> {
    let file = self.frame.file;
    match recv {
        AbstractValue::Instance { class, .. } => {
            let t = s.sem.member(s.sem.instance_type(class), name, file);
            match t.map(|t| (t, s.sem.shape(t, file))) {
                Some((_, Shape::BoundMethod(func))) => {
                    let receiver = self.values.intern(recv);
                    smallvec![AbstractValue::BoundMethod { func, receiver }]
                }
                Some((t, _)) => self.lift(s, t),
                None => smallvec![AbstractValue::Unknown],
            }
        }
        AbstractValue::Class(c) => match s.sem.member(s.sem.class_type(c), name, file) {
            Some(t) => self.lift(s, t),          // a method stays an unbound Function
            None => smallvec![AbstractValue::Unknown],
        },
        AbstractValue::Module(m) => /* step 01.04 */,
        AbstractValue::Unknown => smallvec![AbstractValue::Unknown],
        _ => {
            let ty = s.sem.type_of(&self.frame.model, ExprRef::from(attr));
            self.lift(s, ty)
        }
    }
}
```

```rust
// src/sem.rs
pub fn instance_type(&self, class: Definition<'db>) -> Type<'db> {
    let env = ProgramEnvironment::from_file(class.program_file(self.db));
    vnoc::instance_of(self.db, &env, self.class_type(class)).unwrap_or(Type::unknown())
}

// shape(): Type::BoundMethod(_) => match ty.definition(self.db, &env) {
//     Some(TypeDefinition::Function(d)) => Shape::BoundMethod(d),
//     _ => Shape::Other,
// },
```

```rust
// src/interp/call.rs, in the callee loop of call_values
AbstractValue::BoundMethod { func, receiver } if s.sem.is_project(func) => {
    let recv = self.values[receiver];
    let run = self.walk_inline(s, func, None, Some(recv), args, env);
    self.push_node(s, func, site, run.children);
    out.extend(run.returns);
}
```

</details>

---

## Run

```rust
#[test]
fn p03_02_methods() {
    check("03-classes/python/methods.py");
}
```

If `Greeter.greet` shows no `on_ok` child, print the bound parameters: the
receiver is almost certainly on the wrong one.

---

## Where you are

```
AbstractValue += BoundMethod { func, receiver: ValueId }
Walk          += values: ValueArena
member(Instance) → ty finds the function; you bind the receiver
```

---

## Exercise

1. `class A: def run(self): self.step()` and `def step(self): log()`. Trace
   `A().run()`: every `member` call, and what `self` is in each frame.
2. Why does `member` on a `Class` value give an unbound `Function`, and not a
   `BoundMethod` with the class as receiver?
3. `f = Greeter().greet` at module level, and `def use(): f(on_ok)`. Who
   answers `f` inside `use`, and what edge do you get today?

<details><summary>Answers</summary>

1. `A()` makes object `o` and walks no `__init__`. `o.run` → `member(o, run)`
   → `BoundMethod { A.run, o }`. In `run`'s frame, `self = o`. `self.step` →
   `member(o, step)` → `BoundMethod { A.step, o }`. In `step`'s frame,
   `self = o`, and `log()` is a plain call.
2. Because Python does the same: `Greeter.greet` is the plain function. Its
   first parameter is filled by whatever the call passes first. Only an
   instance binds a receiver (and a classmethod binds the class, step 06).
3. ty: `f` is a module global. ty's type is a bound method with its own
   receiver, which `lift` keeps as `Ty` for now, so `f(on_ok)` has no edge.
   Step 04 lifts it to a `BoundMethod` over an object that holds no state,
   which gives the edge to `Greeter.greet`.

</details>

---

→ Next: [`03-attribute-state.md`](03-attribute-state.md)
