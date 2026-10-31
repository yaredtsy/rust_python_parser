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
if let Some(getter) = s.sem.property_getter(class, name) {
    // through inline_node (03.01), so a getter that reads itself is a [recursive] leaf
    // the value of `r.active` is what the getter returns
    return self.call_function(s, getter, None, Some(recv), attr.range(), &EvaluatedArgs::default());
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

- `sem.property_getter(class, name)`, and the `vnoc` wrapper for the getter.
- The property check in `member`'s instance arm, before step 02's rule.
  Class methods need nothing new (Decision 2).

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
        return self.call_function(s, getter, None, Some(recv), attr.range(), &EvaluatedArgs::default());
    }
    // 3. the class side, through ty (step 02's rule, unchanged)
    let file = self.frame.file;
    match s.sem.member(s.sem.instance_type(class), name, file).map(|t| (t, s.sem.shape(t, file))) {
        Some((t, Shape::BoundMethod { func, .. })) => {
            let receiver = match s.sem.bound_to(t) {
                BoundTo::Object => recv,
                BoundTo::Class => AbstractValue::Class(class), // a classmethod
                BoundTo::Other => return self.lift(s, t),
            };
            let receiver = self.values.intern(receiver);
            smallvec![AbstractValue::BoundMethod { func, receiver }]
        }
        Some((t, _)) => self.lift(s, t),     // a static method arrives here as a plain Function
        None => smallvec![AbstractValue::Unknown],
    }
}
```

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
what ty bound the method to. And an attribute read can be a call.

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
2. Yes. The getter is walked like any call (`call_function`, then
   `walk_inline`), so its exits carry its state back to the caller (step 03,
   decision 4).
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
