# 03.04 — Two instances, and objects from ty

**You build:** nothing new for two objects: origins already keep them apart,
and this step proves it. Then the objects you didn't build: a module global,
a stdlib return, an annotated parameter. ty knows their class but not which
object they are, so they share one reserved origin, `OPAQUE`, which never
holds state.
**Fixture:** `python/isolation.py`
**Plan:** `plan/03-call-tree/04-values.md` § "`OriginId::OPAQUE`",
§ "`lift`", `15-checks-and-build-order.md` § "Writes to module globals are dropped"

---

## The fixture

```python
def two_alive():
    a = Pipeline(JsonHandler())
    b = Pipeline(CsvHandler())
    a.run(1)                        # JsonHandler.process only
    b.run(2)                        # CsvHandler.process only

def make(h): return Pipeline(h)
def one_site():
    a = make(JsonHandler())
    b = make(CsvHandler())          # the same `Pipeline(h)` site, built twice
    a.run(1)                        # JsonHandler.process only
    b.run(2)                        # CsvHandler.process only

LOG_A = Logger()
LOG_B = Logger()
GREET = Greeter().greet

def opaque():
    LOG_A.handler = FileHandler()
    LOG_B.emit()                    # must NOT find FileHandler.write
def global_write():
    LOG_A.handler = FileHandler()
    LOG_A.emit()                    # Python finds FileHandler.write; here, nothing
def global_method():
    GREET(on_ok)                    # Greeter.greet > on_ok
```

## Predict

1. `two_alive` and `one_site` probably pass already. Which part of the origin
   key makes `one_site` pass?
2. `LOG_A` and `LOG_B` are module globals. What does ty say each one is? If
   `lift` gave both the same origin, and writes to it were kept, what would
   `opaque` show?
3. Why not give each of them a fresh origin, every time they are read?

---

## Build

### Decision 1: different origins never share state

Nothing to write: `attrs` is keyed by origin, so `a`'s `handler` and `b`'s
`handler` are two entries. `one_site` is the harder version: one `Pipeline(h)`
expression builds both objects, in two runs of `make`, and the run's
activation in the origin keeps them apart (step 01, decision 4).

This is the fixture to keep green forever. A cache of objects by class, or an
origin without the run, passes `two_alive` and fails `one_site`.

### Decision 2: one origin, `OPAQUE`, for every object ty hands you

```rust
impl OriginId {
    pub const OPAQUE: OriginId = OriginId::from_u32(0);   // interned first, in every walk
}

fn holds_state(&self, origin: OriginId) -> bool { origin != OriginId::OPAQUE }
```

`lift` gains its instance arms:

| ty's type | value |
|---|---|
| an instance of a class (`Logger`) | `Instance { class: Logger, origin: OPAQUE }` |
| a bound method ty made (`Greeter().greet` at module level) | `BoundMethod { func, receiver: Instance { Greeter, OPAQUE } }` |
| `type[Base]` | `Class(Base)` |
| a type alias (`type H = A \| B`) | what it stands for, lifted |

And the rule: **writes to an `OPAQUE` receiver are dropped, and reads skip the
state for it.** `member` goes straight to the class side; `bind_attribute`
filters on `holds_state`.

**Why one shared origin:** ty says "some `Logger`". It doesn't say which one.
`LOG_A` and `LOG_B` are two, `get_logger()` might return either or a third.
If ty-given objects could hold state, they would all hold it in one place (they
can't be told apart), and a write through one would be read through another:
`opaque` would find `FileHandler.write`, an edge Python never takes.

**Why not a fresh origin per read:** then `LOG_A.handler = X` and `LOG_A.emit()`
two lines later would be two different objects. Each read of a module global
would be a stranger. Either way, a ty-given object can't hold state reliably,
so it holds none.

### Decision 3: writes to module globals are a known limit, pinned

`global_write` writes `LOG_A.handler` and calls `LOG_A.emit()`. Python finds
`FileHandler.write`. The walker drops the write (decision 2), so the tree has
no edge. That is the registry pattern, and it is a real loss.

It is listed as a known limit (`plan/03-call-tree/15` § "Writes to module
globals are dropped"), and the fixture pins today's answer. The fix is a root
of its own for module globals a function reads or writes, in keys and in
summaries. When it lands, this expected tree changes on purpose, in the same
commit.

### Decision 4: a bound method from ty gets an `OPAQUE` receiver

`GREET` is a module global whose type is a bound method: ty bound it, to "some
`Greeter`". `lift` makes it `BoundMethod { greet, receiver: Instance { Greeter,
OPAQUE } }`, and calling it walks `greet` with that receiver. You get the edge.
`greet` just can't read state on `self`. (This answers step 02's exercise 3.)

The receiver's class comes from ty: `vnoc::bound_method_self`, a new wrapper
over `BoundMethodType::self_instance` (`pub(super)`, `types/method.rs:29`).

### Yours

- `OriginId::OPAQUE`, `holds_state`, the filter in `member` and
  `bind_attribute`.
- `Shape::Instance(class)`, `Shape::BoundMethod` with its receiver type,
  `Shape::SubclassOf(class)`, `Shape::Alias(value)`, and the `lift` arms.
- The `vnoc::bound_method_self` wrapper.

<details><summary>Reference: this step's code</summary>

```rust
// vendor/ruff/crates/ty_python_semantic/src/types/vnoc.rs (one more wrapper)
/// The receiver ty bound into a bound method. [check] the accessor and its visibility.
pub fn bound_method_self<'db>(db: &'db dyn Db, ty: Type<'db>) -> Option<Type<'db>> {
    match ty {
        Type::BoundMethod(m) => Some(m.self_instance(db)),
        _ => None,
    }
}
```

```rust
// src/interp/value.rs: lift, the new arms
match s.sem.shape(ty, file) {
    Shape::Instance(class) => smallvec![AbstractValue::Instance { class, origin: OriginId::OPAQUE }],
    Shape::BoundMethod { func, receiver } => {
        let recvs = match receiver { Some(r) => self.lift(s, r), None => smallvec![AbstractValue::Unknown] };
        recvs.into_iter()
            .map(|r| AbstractValue::BoundMethod { func, receiver: self.values.intern(r) })
            .collect()
    }
    Shape::SubclassOf(Some(class)) => smallvec![AbstractValue::Class(class)],
    Shape::SubclassOf(None) => smallvec![AbstractValue::Unknown],      // type[Any], type[T]
    Shape::Alias(value) => self.lift(s, value),
    // … the arms from earlier steps …
}
```

`member` keeps its own `Shape::BoundMethod` arm from step 02, which ignores
ty's receiver and binds yours. `lift`'s arm is only for bound methods that
reach the walk without going through `member`.

</details>

---

## Run

```rust
#[test]
fn p03_04_isolation() {
    check("03-classes/python/isolation.py");
}
```

---

## Where you are

```
OriginId::OPAQUE    every object ty hands you. Holds no state
lift: instances, bound methods, type[X], aliases
```

---

## Exercise

1. `def f(): return Logger()` (a project function), and `f().handler = X;
   …`. Is that object `OPAQUE`?
2. `x = some_stdlib_call()` returns a project class instance (ty says so). Can
   the walker call methods on it? Can it track a write to it?
3. Write a fixture that a "one object per class" model passes and origins also
   pass, and one that only origins pass.

<details><summary>Answers</summary>

1. No. `f` is project code: inline mode walks it, and its `Logger()` is built
   by the walk, with a `New` origin. Only an object whose value came from ty
   (because nobody walked the code that built it) is `OPAQUE`.
2. It can call methods: `member` goes to the class side, which only needs the
   class. It can't track a write: the object is `OPAQUE`, so the write is
   dropped, and a later read of that attribute falls to ty's declared type.
3. Both pass: one `Pipeline(JsonHandler())` and its `run`. Only origins pass:
   `two_alive` (two objects of one class with different handlers), and
   `one_site` for an origin that also tells runs apart.

</details>

---

→ Next: [`05-writes-from-anywhere.md`](05-writes-from-anywhere.md)
