# 04.07 — Aliases

**You build:** the alias vector. A key says which of its parameters hold the
same object, and the walk gives them one origin. Without it, `f(x, x)` and
`f(x, y)` with equal-looking objects share one summary, and a write through
one parameter is invisible through the other.
**Fixture:** `python/aliases.py`
**Plan:** `plan/03-call-tree/08-keys.md` § "Fingerprinting" (Alias),
`15-checks-and-build-order.md` § "The alias vector covers top-level parameters only"

---

## The fixture

```python
def set_and_read(a, b):
    a.h = JsonHandler()
    b.h.process()

def same():
    x = Box(CsvHandler())
    set_and_read(x, x)        # JsonHandler.process
def different():
    x = Box(CsvHandler())
    y = Box(CsvHandler())
    set_and_read(x, y)        # CsvHandler.process

class Node:
    def link(self, other): self.peer = other
    def touch(self, other):
        other.h = JsonHandler()
        self.peer.h.process()   # Python: JsonHandler.process
def nested():
    a = Node(); b = Node()
    a.link(b)
    a.touch(b)                # summary mode: no edge. A known limit
```

## Predict

1. Write `set_and_read`'s key in `same` and in `different`, with step 06's
   code. Are they equal?
2. In `set_and_read`'s walk for `same`, `a` and `b` are materialized from two
   fingerprints. How many origins? What does `b.h.process()` find?
3. Inline mode gets all three right. Why?

---

## Build

### Decision 1: the key records which parameters are the same object

```rust
pub struct Context<'db> {
    pub func: Definition<'db>,
    pub args: SmallVec<[FpId; 4]>,
    pub alias: SmallVec<[u8; 4]>,            // ← new: alias[i] = the first parameter holding the same object as i
    pub free: SmallVec<[(Name, FpId); 2]>,
    pub defaults: SmallVec<[(Name, FpId); 2]>,   // step 02.07
}
```

`same` gives `alias = [0, 0]`; `different` gives `[0, 1]`. Two keys, two walks.

**Why the fingerprints alone can't say it:** a fingerprint describes what an
object holds, never which object it is (step 01, decision 3). `x` and `y` hold
the same things, so their fingerprints are equal. Whether they are one object
is a fact about identity, and identity needs its own place in the key.

```rust
fn alias_vector(&self, combo: &Combo<'db>) -> SmallVec<[u8; 4]> {
    let origin_of = |vals: &Values<'db>| match vals.as_slice() {
        [AbstractValue::Instance { origin, .. }] if self.holds_state(*origin) => Some(*origin),
        _ => None,                                   // several values, or no identity: aliases nothing
    };
    (0..combo.slots.len()).map(|i| {
        let me = origin_of(&combo.slots[i]);
        (0..i).find(|&j| me.is_some() && origin_of(&combo.slots[j]) == me).unwrap_or(i) as u8
    }).collect()
}
```

Only a slot holding exactly one object with an identity can alias. After CPA
(step 06) that is the usual case. A collapsed slot with several values aliases
nothing: whether two such slots are the same object is not a single fact.

### Decision 2: aliased parameters get one origin in the walk

`materialize` gives parameter `i` the origin of parameter `alias[i]` when
`alias[i] != i`, instead of a new `Entry(Param(i))`. In `same`, `a` and `b`
are both `Entry(Param(0))`. `a.h = JsonHandler()` is a strong write on that
origin, and `b.h` reads it.

The summary names the object by its first parameter: the effect is
`Param(0).h = [Obj(0)]`. The caller resolves `Param(0)` to `x`, which is also
what it passed as parameter 1. One write, landing once.

### Decision 3: only top-level parameters, and that is a known limit

`nested` passes `a` and `b`, two different objects: `alias = [0, 1]`. But
`a.peer` *is* `b`. In `touch`'s walk, `self.peer` is materialized as
`Entry(Param(0).peer)` and `other` as `Entry(Param(1))`: two origins for one
object. The write to `other.h` is not seen through `self.peer`, and the edge is
lost. Inline mode, which has one object, finds it. The fixture pins the
difference.

The fix is to record aliasing inside the key as pairs of paths
(`Param(0).peer == Param(1)`), listed for later
(`plan/03-call-tree/15` § "Growth path", v1.2). The same gap exists for two
deep paths that name one object (step 05).

**Why accept it now:** every alias costs sharing (a key per aliasing pattern),
and the pairs-of-paths version costs a walk over every pair of reachable
objects at every call. Top-level aliasing (`f(x, x)`, `self.merge(self)`) is
the common case and is cheap.

### Yours

- `Context.alias`, `alias_vector`, and `key` filling it per combo.
- `materialize` sharing origins for aliased parameters.
- The stable encoding of a key (part 05) will include `alias`; nothing to do
  for that now.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/key.rs: materialize, parameters
let ctx = &s.contexts[ctx_id];
let mut param_values: SmallVec<[Values<'db>; 4]> = SmallVec::new();
for (i, (name, &fp)) in param_names(node).zip(&ctx.args).enumerate() {
    let a = ctx.alias[i] as usize;
    let v = if a != i {
        param_values[a].clone()                         // the same object as an earlier parameter
    } else {
        self.from_fp(s, fp, Path::param(i as u8), &mut env)
    };
    env.bind(name, v.clone());
    param_values.push(v);
}
```

</details>

---

## Run

```rust
#[test]
fn p04_07_aliases() {
    check_both("04-class-summaries/python/aliases.py");
}
```

---

## Where you are

```
Context { func, args, alias, free }
```

That completes the key, as `00-values/02-the-context.md` shows it.

---

## Exercise

1. `self.merge(self)`. What is the alias vector, and why does it matter for a
   method that writes `other.x` and reads `self.x`?
2. `f(x, y)` where `y = x if c else Box()`. After CPA, how many contexts, and
   what is each alias vector?
3. Why is aliasing by *first* parameter, and not, say, a set of equal pairs?

<details><summary>Answers</summary>

1. `[0, 0]`: the receiver and the argument are one object. Without it, the
   write to `other.x` would land on `Entry(Param(1))` and `self.x` would read
   `Entry(Param(0))`, missing the write.
2. Two: `y = x` gives `alias = [0, 0]`, `y = the new Box` gives `[0, 1]` (the
   fingerprints may be equal, the aliasing isn't).
3. "Same object" is an equivalence relation, so pointing every member of a
   group at its first member describes it completely, in one byte per
   parameter, and makes equal aliasing patterns compare equal with `==`.

</details>

---

→ Next: [`08-recursion-with-objects.md`](08-recursion-with-objects.md)
