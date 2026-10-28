# 03.06 — `@staticmethod`, `@classmethod`, `@property`

**You build:** the three decorators that change what a method's first
parameter is. A static method has no receiver, a class method's receiver is
the class, and a property's getter is called when the attribute is read.
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

def main():
    Registry.make(on_ok)                 # Registry.make > on_ok
    r = Registry.create(JsonHandler())   # JsonHandler; Registry.create > Registry
    r.active.handle()                    # Registry.active; JsonHandler.handle
    r.fire()                             # Registry.fire > Registry.active, JsonHandler.handle
    r.make(on_ok)                        # Registry.make > on_ok
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

Through an instance, step 02's `member` arm would bind the object. Check the
decorator first:

```rust
Some((_, Shape::BoundMethod { func, .. })) if s.sem.is_classmethod(func) => {
    let receiver = self.values.intern(AbstractValue::Class(class));   // the object's class
    smallvec![AbstractValue::BoundMethod { func, receiver }]
}
```

### Decision 3: a property read is a call

`r.active.handle()` looks like two attribute reads and one call. In Python it
is two calls: reading `active` runs the getter. If the walker only read the
attribute, `.handle` would be looked up on whatever ty says the property
returns, which for an unannotated getter is `Unknown`. No edge.

So when `member` finds a property, it calls the getter right there, as a
project call with the object as receiver, **at the attribute expression's
site**:

```rust
if let Some(getter) = s.sem.property_getter(class, name) {
    let run = self.walk_inline(s, getter, None, Some(recv), &EvaluatedArgs::default(), env);
    self.push_node(s, getter, attr.range(), run.children);
    return run.returns;                       // the value of `r.active` is what the getter returns
}
```

`property_getter` must be asked before the plain lookup: `vnoc::member` on the
*instance* applies the descriptor and returns the getter's return type, so the
property itself is only visible on the *class*. Ask ty for the attribute on the
class, and if it is a property, take its getter. That needs one more `vnoc`
wrapper. **[check]** how ty exposes a property's getter
(`PropertyInstanceType`).

**[check]** parity: the Jedi driver finds calls by syntax, so it shows no node
for `r.active`. The edge is needed either way (its value is what makes
`JsonHandler.handle` resolve). Whether the node is *shown* is a parity
decision: compare with the golden files, and if Jedi doesn't show it, record
the edge and skip the node at render.

### Yours

- `sem.is_classmethod(func)`, `sem.property_getter(class, name)`, and the
  `vnoc` wrapper for the getter.
- The two checks in `member`'s instance arm, before step 02's rule.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/member.rs: the Instance arm, in full
AbstractValue::Instance { class, origin } => {
    // 1. the object's own state (step 03)
    if self.holds_state(origin) {
        if let Some(v) = env.attrs.get(&(origin, name.clone())) {
            return v.clone();
        }
    }
    // 2. a property: call the getter here
    if let Some(getter) = s.sem.property_getter(class, name) {
        let run = self.walk_inline(s, getter, None, Some(recv), &EvaluatedArgs::default(), env);
        self.push_node(s, getter, attr.range(), run.children);
        return run.returns;
    }
    // 3. the class side, through ty
    let file = self.frame.file;
    match s.sem.member(s.sem.instance_type(class), name, file).map(|t| (t, s.sem.shape(t, file))) {
        Some((_, Shape::BoundMethod { func, .. })) => {
            let receiver = if s.sem.is_classmethod(func) {
                AbstractValue::Class(class)
            } else {
                recv
            };
            let receiver = self.values.intern(receiver);
            smallvec![AbstractValue::BoundMethod { func, receiver }]
        }
        Some((t, _)) => self.lift(s, t),     // a static method arrives here as a plain Function
        None => smallvec![AbstractValue::Unknown],
    }
}
```

`is_classmethod` reads the `def`'s decorator list: a decorator whose type is
the builtin `classmethod`.

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

The receiver rule is complete: none, the object, or its class, decided by the
decorator. And an attribute read can be a call.

---

## Exercise

1. A subclass `class Special(Registry)` calls `Special.create(h)`. What is
   `cls`, and which node does `cls(h)` produce?
2. A property whose getter writes `self.cache = X()`. Does the write reach the
   object? Why?
3. `member` checks the object's own state before the property. Python does
   the opposite: a property beats the instance's attributes. When does the
   order make a difference, and what does the tree lose?

<details><summary>Answers</summary>

1. `cls` is `Special`: ty binds a class method to the class it was looked up
   on. `cls(h)` constructs `Special`: a `Special` node, with
   `Registry.__init__`'s calls under it (inherited, step 01).
2. Yes. The getter is walked with `walk_inline`, so its exits carry its state
   back to the caller like any other call (step 03, decision 4).
3. Only when the code assigns to the property's name, which needs a setter:
   `r.active = X` makes `bind_attribute` store `(r, active) = X`, and a later
   `r.active` returns `X` from state without calling the getter. The tree
   loses the `Registry.active` node and anything the getter calls. The value is
   usually right, since a setter usually stores what the getter returns. The
   fix, if a fixture shows it matters: `bind_attribute` calls the setter when
   the attribute is a property. **[check]** setters against the golden files.

</details>

---

→ Next: [`07-chains-super-and-annotations.md`](07-chains-super-and-annotations.md)
