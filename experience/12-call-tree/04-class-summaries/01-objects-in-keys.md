# 04.01 — Objects in keys

**You build:** summary mode for a method called on an object. The object goes
into the key as its class plus its attributes' fingerprints, two steps deep.
The method's walk starts with an object rebuilt from that: named by its place
among the inputs, holding exactly the attributes the key carried. And
constructors start working in summary mode, minus `__init__`'s writes (step 03).
**Fixture:** `python/in_keys.py`
**Plan:** `plan/03-call-tree/08-keys.md` § "Fingerprinting" (instances,
depth), § "Materializing", `10-one-call.md` § "Constructors"

---

## The fixture

```python
class Box:                          # no __init__: the caller writes its state
    def run(self): self.handler.process()

def json_box(): b = Box(); b.handler = JsonHandler(); b.run()   # … JsonHandler.process
def csv_box():  b = Box(); b.handler = CsvHandler();  b.run()   # … CsvHandler.process

def shared():
    a = Box(); a.handler = JsonHandler()
    b = Box(); b.handler = JsonHandler()
    a.run()
    b.run()                         # one Box.run summary for both

def use(b): b.run()
def passed():
    b = Box(); b.handler = CsvHandler()
    use(b)                          # use > Box.run > CsvHandler.process

def deep():                         # s.app.config.handler, three steps from self
    …
    s.run()                         # inline: JsonHandler.process. summary: nothing
```

## Predict

1. Write `Box.run`'s key in `json_box`. What does it hold for `self`?
2. `shared` has two different `Box` objects. How many summaries of `Box.run`
   does summary mode compute? Is that right?
3. Why can't the key hold the object's origin?

---

## Build

### Decision 1: an object in a key is its class plus its attributes

```rust
pub enum Fp<'db> {
    // …
    Instance { class: Definition<'db>, attrs: SmallVec<[(Name, FpId); 4]> },   // sorted by name
    Opaque(Definition<'db>),                                                   // class only
    Bound { func: Definition<'db>, recv: FpId },                               // a bound method's receiver
}
```

`json_box` calls `run` with `self = Instance { Box, [handler: Instance { JsonHandler, [] }] }`.
`csv_box` gives `Instance { Box, [handler: Instance { CsvHandler, [] }] }`. Two
keys, two walks, two different children: the per-path tree, from the key.

The attributes come from the caller's state just before the call, so
fingerprinting needs the env: `key(func, bound, before: &Env)`.

**Why not the origin:** an origin is an id in the caller's walk. It means
nothing to the callee, which is walked once and reused by every caller with
the same key. Same reason as a closure's activation in step 02.07: rule 2, in
the key.

**Why the attributes:** the method's walk reads them. `run` reads
`self.handler`. If the key held only the class, both callers would share a
walk that can't know which handler it has.

### Decision 2: two steps deep, and then the class only

Every step counts: an attribute, a bound method's receiver, a closure's free
value. At `fp_depth` (2), an instance becomes `Opaque(class)`: its class, none
of its attributes. An `OPAQUE` instance is always `Opaque(class)`.

**Why a limit:** without one, a key holds everything reachable from the
arguments. `self.parent.parent.parent…` makes keys huge, and an object that
refers to itself (`node.root = node`) makes them infinite. A limit keeps every
key small and finite.

**What it costs:** `deep`. `run` reads `self.app.config.handler`, three steps
from `self`. The key stops at `self.app.config` as `Opaque(Config)`, so in the
walk `config` has no `handler`, and the class side knows none: no edge. Inline
mode, with no keys, finds it. The fixture's mode-specific blocks pin the
difference. It is a known limit (`plan/03-call-tree/15` § "`fp_depth = 2`
loses reads at depth 3"), to be measured: K = 2 against K = 3, on the fixtures
and one real project, before the default is final.

### Decision 3: where an object was built is not in the key

`shared`'s two boxes were built on different lines. Their fingerprints are
equal, so `b.run()` reuses `a.run()`'s summary. Walks: `shared`, `Box.run`,
`JsonHandler.process`: 3, and one reuse.

**This sharing is the point.** A key with allocation sites would give every
`Pipeline(JsonHandler())` in the project its own `run` walk, finding the same
thing every time. Type sensitivity, keyed by what an object holds, is what
makes the number of contexts small.

What is lost is *identity*: the callee can't tell your object from another
equal one. Step 02 gets it back, without the key: a summary names objects by
their place among the inputs.

### Decision 4: a walk rebuilds input objects from the key, named by path

```rust
pub enum OriginKey {
    Entry(Path),                                   // an input object, named by where it sits
    New { site: SiteId, act: ActivationId },       // built in this walk (step 06 widens this)
}

pub struct Path { pub root: Root, pub steps: SmallVec<[Step; 2]> }
pub enum Root { Param(u8), Free(u8) }
pub enum Step { Attr(Name), Free(u8), Recv }
```

`materialize` turns `Instance { class, attrs }` for parameter `i` into
`Instance { class, origin: Entry(Param(i)) }`, and each attribute into its own
values at `Param(i).name`, written into `attrs`. `Opaque(class)` gets an
`Entry` origin too, with no attributes: it is still one specific caller
object, so writes to it can be tracked (step 03).

`Entry(i)` from step 03.07 becomes `Entry(Path { root: Param(i), steps: [] })`.

`run`'s walk for `json_box`'s key: `self = Instance { Box, Entry(Param(0)) }`,
`(Entry(Param(0)), handler) = Instance { JsonHandler, Entry(Param(0).handler) }`.
`self.handler.process()` finds the state, then `JsonHandler.process`.

### Decision 5: constructors in summary mode

`Box()`: a new origin in the caller's walk, `New { site, act }`; then, if the
class has a project `__init__`, key it with `self = Instance { class, [] }` (a
new object holds nothing yet), `get` it, and record the edge as a construct:

```rust
pub enum Target<'db> {
    Call(ContextId),
    Construct { class: Definition<'db>, init: Option<ContextId> },   // ← new
}
```

`render` draws a `Construct` as a node named by the class, with its `init`
contexts' children under it. `init: None` (no `__init__` body anywhere in the
MRO) is a node with no children, as in inline mode.

What's missing: `__init__`'s writes to `self`. They are effects, step 03. Until
then, only fixtures whose state is written by the caller pass in summary mode,
which is why `Box` has no `__init__`.

### Yours

- `Fp::Instance`, `Fp::Opaque`, `Fp::Bound`, `fp_depth`, and `key` reading the
  caller's env.
- `Path`, `Root`, `Step`, `OriginKey::Entry(Path)`, and `materialize` for
  objects and bound methods.
- If your `attrs` is a flat `(origin, name)` map: fingerprinting lists one
  object's attributes, which wants a map per object
  (`HashTrieMap<OriginId, HashTrieMap<Name, Values>>`). Switch now.
- Constructors in summary mode, `Target::Construct`, and `render` for it.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/key.rs: fingerprinting one value
fn fp_one(&mut self, w: &Walk<'db>, v: AbstractValue<'db>, env: &Env<'db>, depth: u8,
          closures: &mut ClosureStack<'db>) -> FpId {
    use AbstractValue::*;
    let fp = match v {
        Instance { class, origin } if !w.holds_state(origin) || depth == self.opts.limits.fp_depth
            => Fp::Opaque(class),
        Instance { class, origin } => Fp::Instance {
            class,
            attrs: env.attrs_of(origin)                         // sorted by name
                .map(|(name, vals)| (name.clone(), self.fingerprint(w, vals, env, depth + 1, closures)))
                .collect(),
        },
        BoundMethod { func, receiver } =>
            Fp::Bound { func, recv: self.fp_one(w, w.values[receiver], env, depth + 1, closures) },
        // Function, Class, Module, Ty, Unknown, Closure, Back: as in part 02
        // …
        Pending(_) => unreachable!("deferred before keying"),
    };
    self.fps.intern(fp)
}
```

```rust
// src/interp/key.rs: materializing one fingerprint at a path
fn from_fp(&mut self, s: &Solver<'db>, fp: FpId, at: Path, env: &mut Env<'db>) -> Values<'db> {
    match &s.fps[fp] {
        Fp::Instance { class, attrs } => {
            let origin = self.origins.intern(OriginKey::Entry(at.clone()));
            for (name, a) in attrs {
                let v = self.from_fp(s, *a, at.attr(name), env);
                env.bind_attr(origin, name, v, true);
            }
            smallvec![AbstractValue::Instance { class: *class, origin }]
        }
        Fp::Opaque(class) => {
            let origin = self.origins.intern(OriginKey::Entry(at));
            smallvec![AbstractValue::Instance { class: *class, origin }]
        }
        Fp::Bound { func, recv } => {
            let r = self.from_fp(s, *recv, at.recv(), env);
            r.into_iter()
                .map(|r| AbstractValue::BoundMethod { func: *func, receiver: self.values.intern(r) })
                .collect()
        }
        Fp::Union(ids) => ids.iter().flat_map(|&i| self.from_fp(s, i, at.clone(), env)).collect(),
        // Function, Class, Module, Data, Unknown, Closure, Back: as in part 02
        _ => todo!(),
    }
}
```

The `Union` arm materializes every member at the same path. Two objects at one
path share one origin: a known imprecision that CPA (step 06) mostly removes.

</details>

---

## Run

```rust
#[test]
fn p04_01_in_keys() {
    check_both("04-class-summaries/python/in_keys.py");
    let st = stats_for("04-class-summaries/python/in_keys.py", "shared", &summary());
    assert_eq!((st.walks, st.reuse_hits), (3, 1));
}
```

`check_both` from now on: every fixture of this part runs in both modes.

---

## Where you are

```
Fp        += Instance { class, attrs }, Opaque(class), Bound { func, recv }
OriginKey  = Entry(Path) | New { site, act }
Target    += Construct { class, init }
```

A method called on an object works in summary mode, when the caller wrote the
object's state. Objects can go into a call. Step 02 brings them back out.

---

## Exercise

1. `b.handler = b` (an object that holds itself). Write its fingerprint at
   `fp_depth = 2`.
2. Two `Box` objects with *different* handlers are both passed to `use`.
   How many `use` contexts, how many `Box.run` contexts?
3. In `shared`, the second `b.run()` is reused. If `Box.run` wrote
   `self.count = 1`, would reusing it be wrong? (Step 03 answers it. Guess
   first.)

<details><summary>Answers</summary>

1. `Instance { Box, [handler: Instance { Box, [handler: Opaque(Box)] }] }`:
   depth 0 is the argument, depth 1 its `handler`, and at depth 2 the limit
   makes it `Opaque`. The self-reference ends there instead of looping.
2. Two of each: the two `b`s have different fingerprints, so `use` gets two
   keys, and each `use` walk calls `run` with a different `self`.
3. Not wrong, as long as the summary reports the write in terms of its input
   ("parameter 0's `count` is now 1") and each caller applies it to *its own*
   object. That is exactly what an effect is.

</details>

---

→ Next: [`02-objects-coming-back.md`](02-objects-coming-back.md)
