# 03.06 — `@staticmethod`, `@classmethod`, `@property`

**You build:** the three decorators that change what a method's first
parameter is. A static method has no receiver, a class method's receiver is
the class, and a property's getter is called when the attribute is read, its
setter when it is written. Inline mode only, like method calls.
**Fixture:** `python/special.py`
**Plan:** `plan/03-call-tree/06-binding-arguments.md` § "Five things that are
easy to get wrong" (the receiver table), `07-attributes-and-self.md`
§ "Properties"

---

## The fixture

```python
class Registry:
    def __init__(self, h): self.h = h
    @staticmethod
    def make(cb): cb()
    @classmethod
    def create(cls, h): return cls(h)
    @property
    def active(self): return self.h
    def fire(self): self.active.handle()
    def get_h(self): return self.h
    current = property(get_h)            # a property without the decorator

def main():
    Registry.make(on_ok)                 # Registry.make > on_ok
    r = Registry.create(JsonHandler())   # JsonHandler; Registry.create > Registry
    r.active.handle()                    # Registry.active; JsonHandler.handle
    r.fire()                             # Registry.fire > Registry.active, JsonHandler.handle
    r.make(on_ok)                        # Registry.make > on_ok
    r.current.handle()                   # Registry.get_h; JsonHandler.handle

class Box:
    def __init__(self, h): self._h = h
    @property
    def handler(self): return self._h
    @handler.setter
    def handler(self, h): self._h = h

def swap():
    b = Box(JsonHandler())
    b.handler = CsvHandler()             # Box.handler: the setter runs
    b.handler.handle()                   # Box.handler: the getter; CsvHandler.handle only

class Lazy:
    def __init__(self, h): self.h = h
    @cached_property
    def active(self): return self.h

def lazy():
    Lazy(JsonHandler()).active.handle()  # known gap: no Lazy.active, no JsonHandler.handle
```

## Predict

1. `r.make(on_ok)` goes through an instance. With step 02's rule (bind the
   object as receiver), which parameter would `on_ok` land on?
2. `Registry.create(JsonHandler())`: what is `cls` inside `create`, and what
   does `cls(h)` do with it?
3. `r.active` has no parentheses. Why must it still be a node, and where does
   `.handle` look for its method?

---

## Build

### Decision 1: the decorator decides the receiver

| decorator | through the class `C.m(…)` | through an instance `o.m(…)` |
|---|---|---|
| none | no receiver: `o` is passed explicitly | receiver `o` (step 02) |
| `@staticmethod` | no receiver | **no receiver** |
| `@classmethod` | receiver `C` | receiver **the object's class**, not the object |
| `@property` | the property object itself | **the getter is called**, with receiver `o` |

ty already applies the descriptor rules when you ask it for the attribute, so
part of this comes for free: a static method comes back from `vnoc::member` as
a plain function (no `BoundMethod`), and `lift` makes it `Function`. Nothing
to bind.

The off-by-one test of step 02, again: `r.make(on_ok)` must bind `cb = on_ok`.
If `member` wrapped it as `BoundMethod { make, r }`, `cb` would be `r`.

### Decision 2: a class method's receiver is the class

`Registry.create` through the class: ty answers with a method bound to the
class. `lift` (step 04) gives `BoundMethod { create, receiver: Class(Registry) }`,
and `cls` is the class. `cls(h)` is then a call on a `Class` value: a
construction (step 01), node `Registry` under `Registry.create`, and the value
is the new object.

Through an instance, step 02 already gets it right. ty binds the classmethod
to the class, and `sem.bound_to` reads that from the variant of `__self__`
(`BoundTo::Class`), so the receiver is the object's class. Nothing new here,
and no decorator check: an earlier plan read the `def`'s decorator list
(`is_classmethod`), and step 02's rule replaces it.

### Decision 3: a property read is a call

`r.active.handle()` looks like two attribute reads and one call. In Python it
is two calls: reading `active` runs the getter. If the walker only read the
attribute, `.handle` would be looked up on whatever ty says the property
returns, which for an unannotated getter is `Unknown`. No edge.

So when `member` finds a property, it calls the getter right there, as a
project call with the object as receiver, **at the attribute expression's
site**:

```rust
// member, Instance arm, after the state read (inline mode only)
if sol.opts.mode == Mode::Inline {
    if let Some(getter) = sol.sem.property_parts(class, name).and_then(|p| p.getter) {
        // through inline_node (03.01): a getter that reads itself is a [recursive] leaf.
        // The value of `r.active` is what the getter returns.
        return self.call_function(sol, getter, None, Some(recv), site, &EvaluatedArgs::default(), env);
    }
}
```

`member` gains `site: TextRange`, the attribute expression's range, passed
from `eval_attribute`.

`property_parts` must be asked before the plain lookup: `vnoc::member` on the
*instance* applies the descriptor and returns the getter's return type, so the
property itself is only visible on the *class*. Ask ty for the attribute on the
class, and if it is a `Type::PropertyInstance`, take its getter and setter.

**`property(get_h)` is the same property, with a different getter.** ty builds
a `PropertyInstance` for it too (`types/call/bind.rs:3161`). But for
`@property def active` the getter is a `FunctionLiteral`, and for
`property(get_h)` it is a `Callable`: `get_h` converted to a signature type,
which `Type::definition` has no arm for. `shape` gives `Other`, so without
help `current` has no getter, and the tree loses `Registry.get_h` and the
`JsonHandler.handle` after it.

ty does keep the function: a `Callable` made from a `def` carries its
definition in its signature (`Signature::definition`, `signatures.rs:601`).
One wrapper reads it, `vnoc::callable_definition`, and `property_parts` tries
it when `shape` doesn't give a `Function`.

**Why only in `property_parts`, not in `shape`:** `shape` feeds `lift`
everywhere. A decorator that returns a `Callable` typed with the original
signature would then lift to the undecorated `def`, and the walker would walk
a body the program never calls directly. Here the context is known: a
property's getter is called as the getter.

**Rejected: read the class body.** Finding `current = property(get_h)` in the
AST and evaluating `get_h` is syntax-level logic that ty already did. It also
only covers that one spelling.

**Only project accessors.** A project class can inherit a property from a
library class. Its getter is a stub (`...`): walking it would put a library
node in the tree, and the walk falls off the end, so the value would be `None`
instead of ty's declared type. So `property_parts` keeps a getter or setter
only when it is project code. With no project getter, `member` falls through to
ty's lookup, which gives the declared type.

**Why inline mode only:** in summary mode, `call_function` → `walk_inline`
swaps in a frame that summary mode never reads. The getter's node and the
edges its calls record go into that frame and are thrown away. Nothing
crashes, but edges vanish. Since 03.04 summary mode does see objects (the
`OPAQUE` ones `lift` makes), so this path is reachable there. Until part 04,
summary mode falls through to ty's lookup, as with method calls.

**Why not one lookup, on the class, for everything:** the class lookup misses
what only an instance has (`self.h = h` in `__init__`), and binding methods to
the object is exactly the descriptor work ty does on the instance lookup. Both
lookups are cached by ty (salsa), so the second one costs nothing to speak of.

**[check]** parity: the Jedi driver finds calls by syntax, so it shows no node
for `r.active`. The edge is needed either way (its value is what makes
`JsonHandler.handle` resolve). Whether the node is *shown* is a parity
decision: compare with the golden files, and if Jedi doesn't show it, record
the edge and skip the node at render.

### Decision 4: a property write runs the setter, and stores nothing

`b.handler = CsvHandler()` doesn't put anything in `b.__dict__` under
`handler`. Python calls the setter. So `bind_attribute`, per receiver:

```rust
for r in recvs.iter().copied() {
    let AbstractValue::Instance { class, origin } = r else { continue };
    match sol.sem.property_parts(class, &attr.attr.id) {
        // a property: its setter runs, and nothing is stored under its name
        Some(parts) => {
            if let (Some(setter), Mode::Inline) = (parts.setter, sol.opts.mode) {
                let args = EvaluatedArgs { positional: smallvec![v.clone()], ..Default::default() };
                self.call_function(sol, setter, None, Some(r), attr.range(), &args, env);
            }
        }
        None if origin.holds_state() => env.bind_attr(origin, &attr.attr.id, v.clone(), strong),
        None => {}
    }
}
```

`strong` is still counted on every receiver, before this loop (03.04).

**Why walk the setter, not just drop the write:** a setter usually stores the
value somewhere the getter reads (`self._h`). Three choices for `swap`:

| | the tree for `b.handler = CsvHandler(); b.handler.handle()` |
|---|---|
| store `(b, handler)` | value right, but the getter never runs: no `Box.handler` node |
| drop the write | the getter runs, but `_h` was never written: `JsonHandler.handle`, wrong |
| walk the setter | `Box.handler` twice (setter, getter), `CsvHandler.handle` |

And since state never holds a property's name now, `member`'s order (state
first, then property) can't hide a getter.

**The setter's decorator must be trusted.** `@handler.setter` is a call to
`property.setter`, a library method. `decorator_changes_signature` would
distrust it, and every parameter after `self` would bind `Unknown`: the setter
would store `Unknown`. So `SIGNATURE_KEEPING` gains `builtins.property.setter`,
and `decorator_changes_signature` trusts a `Shape::BoundMethod { func, .. }`
decorator on the list, as it does a function. **[check]** the qname.

### Decision 5: `cached_property` is a pinned gap

`functools.cached_property` is not a `PropertyInstanceType` to ty. It is an
instance of a library class with `__get__`, so `property_parts` doesn't see
it, and `lazy` shows neither `Lazy.active` nor `JsonHandler.handle`. It is
common in real code, so the fixture pins today's answer now. When support
lands (`property_parts` learning ty's type for it), this expected tree changes
on purpose, in the same commit.

### Yours

- `sem.property_parts(class, name)`: the project getter and setter.
  `PropertyInstanceType`'s `getter` and `setter` accessors are `pub`, so
  reading them needs no wrapper.
- `vnoc::callable_definition`, for a getter that arrives as a `Callable`
  (`property(get_h)`).
- The property check in `member`'s instance arm, inline mode only, and
  `site` on `member`.
- The setter in `bind_attribute` (Decision 4), and `builtins.property.setter`
  in `SIGNATURE_KEEPING` with the `BoundMethod` arm in
  `decorator_changes_signature`.
- Class methods need nothing new (Decision 2).

<details><summary>Reference: this step's code</summary>

```rust
// src/sem.rs
/// A property's project accessors. `None` when `name` isn't a property on `class`.
pub struct PropertyParts<'db> {
    pub getter: Option<Definition<'db>>,
    pub setter: Option<Definition<'db>>,
}

pub fn property_parts(&self, class: Definition<'db>, name: &str) -> Option<PropertyParts<'db>> {
    let file = class.program_file(self.db);
    let Type::PropertyInstance(p) = self.member(self.class_type(class), name, file)? else {
        return None;
    };
    // `property(get_h)` holds `get_h` as a `Callable`. Kept local: `shape` must not
    // turn every Callable that has a definition into a Function.
    let project = |t: Option<Type<'db>>| {
        let t = t?;
        let d = match self.shape(t, file) {
            Shape::Function(d) => d,
            _ => vnoc::callable_definition(self.db, t)?,
        };
        // a library accessor (a stub) is not walked: `member` falls through to ty
        self.is_project(d).then_some(d)
    };
    Some(PropertyParts { getter: project(p.getter(self.db)), setter: project(p.setter(self.db)) })
}
```

```rust
// vendor/ruff/crates/ty_python_semantic/src/types/vnoc.rs (one more wrapper)
/// The function a callable type was made from, when ty kept it: `property(get_h)`
/// holds `get_h` as a `Callable`. `Signature::definition` (`signatures.rs:1133`), pub(crate).
/// Overloads: no single function to walk.
pub fn callable_definition<'db>(db: &'db dyn Db, ty: Type<'db>) -> Option<Definition<'db>> {
    match ty {
        Type::Callable(c) => match c.signatures(db).overloads.as_slice() {
            [sig] => sig.definition(),
            _ => None,
        },
        _ => None,
    }
}
```

```rust
// src/interp/member.rs: the Instance arm, in full
AbstractValue::Instance { class, origin } => {
    // 1. the object's own state (step 03)
    if origin.holds_state() {
        if let Some(v) = env.attrs.get(&(origin, Name::from(name))) {
            return v.clone();
        }
    }
    // 2. a property: call the getter here (inline mode only)
    if sol.opts.mode == Mode::Inline {
        if let Some(getter) = sol.sem.property_parts(class, name).and_then(|p| p.getter) {
            return self.call_function(sol, getter, None, Some(recv), site, &EvaluatedArgs::default(), env);
        }
    }
    // 3. the class side, through ty (step 02's rule, unchanged)
    let t = sol.sem.member(sol.sem.instance_type(class), name, file);
    match t.map(|t| (t, sol.sem.shape(t, file))) {
        Some((t, Shape::BoundMethod { func, .. })) => {
            let receiver = match sol.sem.bound_to(t) {
                BoundTo::Object => recv,
                BoundTo::Class => AbstractValue::Class(class), // a classmethod
                BoundTo::Other => return self.lift(sol, t),
            };
            let receiver = self.values.intern(receiver);
            smallvec![AbstractValue::BoundMethod { func, receiver }]
        }
        Some((t, _)) => self.lift(sol, t),     // a static method arrives here as a plain Function
        None => smallvec![AbstractValue::Unknown],
    }
}
```

```rust
// src/interp/member.rs: bind_attribute, in full
pub fn bind_attribute(&mut self, sol: &mut Solver<'db>, attr: &ExprAttribute,
                      v: Values<'db>, env: &mut Env<'db>) {
    let recvs = self.eval(sol, &attr.value, env);
    let strong = recvs.len() == 1; // counted before any filter (03.04)
    for r in recvs.iter().copied() {
        let AbstractValue::Instance { class, origin } = r else { continue };
        match sol.sem.property_parts(class, &attr.attr.id) {
            Some(parts) => {
                if let (Some(setter), Mode::Inline) = (parts.setter, sol.opts.mode) {
                    let args = EvaluatedArgs { positional: smallvec![v.clone()], ..Default::default() };
                    self.call_function(sol, setter, None, Some(r), attr.range(), &args, env);
                }
            }
            None if origin.holds_state() => env.bind_attr(origin, &attr.attr.id, v.clone(), strong),
            None => {}
        }
    }
}
```

```rust
// src/sem.rs
// SIGNATURE_KEEPING gains "builtins.property.setter".   [check] the qname
// decorator_changes_signature, next to the Function and Class arms:
//     Shape::BoundMethod { func, .. } => SIGNATURE_KEEPING.contains(&sol.sem.qname(func).as_str()),
```

`call_function` must be reachable from `member.rs` (`pub(crate)`).

</details>

---

## Run

```rust
#[test]
fn p03_06_special() {
    check("03-classes/python/special.py");
}
```

---

## Where you are

The receiver rule is complete: none, the object, or its class, decided by
what ty bound the method to. An attribute read can be a call (the getter), and
so can an attribute write (the setter). `cached_property` is a pinned gap.

---

## Exercise

1. A subclass `class Special(Registry)` calls `Special.create(h)`. What is
   `cls`, and which node does `cls(h)` produce?
2. A property whose getter writes `self.cache = X()`. Does the write reach the
   object? Why?
3. `member` checks the object's own state before the property. Python does
   the opposite: a property beats the instance's attributes. When does the
   order make a difference, and what does the tree lose?
4. `current = property(get_h)` and `@property def active` are both
   `PropertyInstance`s to ty. Why did `current` need `callable_definition`,
   and why does that fallback live in `property_parts` and not in `shape`?

<details><summary>Answers</summary>

1. `cls` is `Special`: ty binds a class method to the class it was looked up
   on. `cls(h)` constructs `Special`: a `Special` node, with
   `Registry.__init__`'s calls under it (inherited, step 01).
2. Yes. The getter is walked like any call (`call_function`, then
   `walk_inline`), so its exits carry its state back to the caller (step 03,
   decision 4).
3. Never, after Decision 4. The order could only matter if state held the
   property's name, and a property write runs the setter instead of storing.
   Before Decision 4, `r.active = X` stored `(r, active) = X`, and every later
   `r.active` returned `X` from state: the `Registry.active` node and anything
   the getter calls were lost. **[check]** setter nodes against the golden
   files.
4. The decorator form stores the getter as a `FunctionLiteral`, which `shape`
   turns into `Function(def)`. Through `property(get_h)`, ty stores `get_h`
   as a `Callable` (a signature type), and `Type::definition` has no arm for
   that, so `shape` says `Other`. The definition survives inside the
   callable's signature, which the wrapper reads. In `shape` the same rule
   would apply to every `Callable` the walker lifts, including a decorator's
   result typed with the wrapped function's signature. That would walk the
   undecorated body, where Python calls the wrapper. Inside `property_parts`
   the callable is known to be the getter.

</details>

---

→ Next: [`07-chains-super-and-annotations.md`](07-chains-super-and-annotations.md)
