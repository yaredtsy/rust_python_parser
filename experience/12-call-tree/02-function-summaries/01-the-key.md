# 02.01 — The key

**You build:** fingerprints and contexts. At every call, inline mode now also
computes the call's key and interns it, without using it yet. The counter
tells you how many walks reuse would save.
**Fixture:** `python/keys.py`
**Plan:** `plan/03-call-tree/08-keys.md` § "The key", § "Fingerprinting"

---

## The fixture

```python
def run(cb): cb()
def note(msg): pass

def same_twice():  run(a); run(a)          # one key, twice
def different():   run(a); run(b)          # two keys
def literals():    note("x"); note("y"); note(1)   # two keys: str, int
def either(c):
    f = a if c else b
    run(f)                                 # one key, holding a union

def main():
    same_twice(); different(); literals(); either(True)
```

Inline mode walks **19** bodies for `main`. There are **12** distinct keys.

## Predict

1. List the 12 keys. (Every function called, including `main`, `a` and `b`,
   with what its parameters hold.)
2. `note("x")` and `note("y")`: why can they share a key? What would have to
   be true of the walker for them *not* to?
3. Why can't the key hold the `AbstractValue`s themselves?

---

## Build

### Decision 1: a call is its function plus what its parameters hold

```rust
// src/interp/key.rs
pub struct Context<'db> {
    pub func: Definition<'db>,
    pub args: SmallVec<[FpId; 4]>,     // one per parameter, in order, defaults filled
    // `free` (a closure's captured values) arrives in step 07
}
```

Not the call site, not the caller, not the depth, not the path. Part 01's last
step listed what a walk depends on: its arguments, its free variables, module
globals (the same on every path) and the code (the same for the whole
request). The key is the first two.

**Why this is enough:** two calls with equal keys start their walks from equal
envs, read the same globals and the same code, and so find the same calls and
return the same values. That is rule 1, and step 02 makes the walk obey it.

### Decision 2: the key holds fingerprints, not values

```rust
pub enum Fp<'db> {
    Unknown,
    Data(Type<'db>),               // a value only ty describes. Literals widened
    Function(Definition<'db>),
    Module(Type<'db>),
    Union(SmallVec<[FpId; 4]>),    // several values: sorted, deduplicated
    // Closure and Back: step 07. Instance, Opaque, Bound: part 04
}
```

**Why not `AbstractValue`:** some values contain ids that only mean something
inside one walk. A `Closure` holds an `ActivationId`: activation 3 of one walk
is a different run from activation 3 of another. Part 04's objects hold
`OriginId`s with the same problem. A key has to mean the same thing in every
walk that uses it, so values are reduced to a description that does:
**a fingerprint**.

For the values you have now, the mapping is short:

| value | fingerprint |
|---|---|
| `Function(d)` | `Function(d)` |
| `Module(t)` | `Module(t)` |
| `Ty(t)` | `Data(widen(t))` |
| `Unknown` | `Unknown` |
| no value at all (empty) | `Unknown` |
| several values | `Union` of their fingerprints |
| `Closure { … }` | step 07. For now `Unknown`, and only inline mode computes keys |

### Decision 3: literals widen to their class

`note("x")` and `note("y")` both give `Data(str)`, so one key.

**Why:** the walker never reads a literal's value. Conditions are not
evaluated (step 01.09), and a string is never called. Two calls that differ
only in a literal's value walk identically. Keeping the literal would give
`log("starting")`, `log("done")` and 200 other calls 200 keys and 200 walks
that find the same thing.

**What would change it:** a policy that does evaluate some conditions (the
plan lists keeping `bool` and `None` literals as a v1.2 option). That would be
a deliberate change to the key, with its own fixtures.

`sem.widen(t)` asks ty for the class of a literal type: `Literal[3]` is `int`,
`Literal["x"]` is `str`. **[check]** the accessor (ty's literal fallback); it
may need a `vnoc` wrapper.

### Decision 4: several values make one `Union`

`either` passes `f = a | b`. The key gets one fingerprint, `Union([a, b])`,
sorted by `FpId` and deduplicated, so `a | b` and `b | a` are the same key.

Part 04 adds the alternative, splitting the call into one key per value. It
matters once values have identity and state. For functions it would only
change how keys are shared.

### Decision 5: intern keys, and compare them in full

```rust
pub struct Interner<K, I> {
    items: IndexVec<I, K>,
    ids: FxHashMap<K, I>,
}
// fps:      Interner<Fp<'db>, FpId>
// contexts: Interner<Context<'db>, ContextId>
```

Equal keys get the same `ContextId`. The map compares whole values, so two
different keys can never share an id.

**Rejected:** a 64-bit hash as the id. A collision is rare, and when it
happens it is a silently wrong tree with no way to find out why. Part 05 adds
a 128-bit digest, only for identity outside the process.

### Decision 6: binding produces slots, not an env

`bind_parameters` from step 01.07 builds the callee's env directly. A key
needs the bound values per parameter, in order. Split it:

```rust
pub struct Bound<'db> {
    pub slots: SmallVec<[Values<'db>; 4]>,   // one per parameter, in order, defaults filled
}

fn bind_parameters(&mut self, def: &StmtFunctionDef, args: &EvaluatedArgs<'db>) -> Bound<'db>
fn env_from_bound(&self, def: &StmtFunctionDef, bound: &Bound<'db>) -> Env<'db>
```

Inline mode builds its env with `env_from_bound`, exactly as before. The key
is built from the same slots. Step 02 adds a second way to build an env:
from the key.

### Decision 7: measure before you build the cache

Inline mode computes and interns each call's key, and `Stats` gains
`contexts`. Nothing uses the keys yet.

**Why:** the number of distinct keys is the number of walks summary mode will
do. If it were close to the number of inline walks, summaries would not be
worth building. On `keys.py` it is 12 against 19. On a 10 × 6 wall, 61 against
a million.

### Yours

- `interp/key.rs`: `Fp`, `FpId`, `Context`, `ContextId`, `Interner`,
  `fingerprint(values) -> FpId`, `key(func, &Bound) -> ContextId`.
- Somewhere to keep the interners for the whole request. A small
  `Keys { fps, contexts }` struct in `Walk` is enough for now. Step 03 moves it
  into the solver.
- `Bound`, `env_from_bound`.
- `sem.widen(ty)`.
- `Stats.contexts`, and the entry's key (no arguments: every parameter
  `Unknown`, or its default).

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/key.rs
use ruff_index::{IndexVec, newtype_index};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::hash::Hash;

use crate::interp::bind::Bound;
use crate::interp::value::{AbstractValue, Values};
use crate::sem::{Definition, Sem, Type};

#[newtype_index]
#[derive(Ord, PartialOrd)]          // `fingerprint` sorts FpIds
pub struct FpId;

#[newtype_index]
pub struct ContextId;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Fp<'db> {
    Unknown,
    Data(Type<'db>),
    Function(Definition<'db>),
    Module(Type<'db>),
    Union(SmallVec<[FpId; 4]>),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Context<'db> {
    pub func: Definition<'db>,
    pub args: SmallVec<[FpId; 4]>,
}

pub struct Interner<K, I: ruff_index::Idx> {
    items: IndexVec<I, K>,
    ids: FxHashMap<K, I>,
}

impl<K: Clone + Eq + Hash, I: ruff_index::Idx> Interner<K, I> {
    pub fn intern(&mut self, k: K) -> I {
        if let Some(&id) = self.ids.get(&k) {
            return id;
        }
        let id = self.items.push(k.clone());
        self.ids.insert(k, id);
        id
    }
    pub fn len(&self) -> usize { self.items.len() }
}

impl<K, I: ruff_index::Idx> std::ops::Index<I> for Interner<K, I> {
    type Output = K;
    fn index(&self, i: I) -> &K { &self.items[i] }
}

#[derive(Default)]
pub struct Keys<'db> {
    pub fps: Interner<Fp<'db>, FpId>,
    pub contexts: Interner<Context<'db>, ContextId>,
}

impl<'db> Keys<'db> {
    pub fn fingerprint(&mut self, sem: &Sem<'db>, vals: &Values<'db>) -> FpId {
        let mut ids: SmallVec<[FpId; 4]> = vals.iter().map(|v| self.fp_one(sem, *v)).collect();
        ids.sort();
        ids.dedup();
        match ids.len() {
            0 => self.fps.intern(Fp::Unknown),     // no value reaches: keyed as Unknown
            1 => ids[0],
            _ => self.fps.intern(Fp::Union(ids)),
        }
    }

    fn fp_one(&mut self, sem: &Sem<'db>, v: AbstractValue<'db>) -> FpId {
        let fp = match v {
            AbstractValue::Function(d) => Fp::Function(d),
            AbstractValue::Module(t) => Fp::Module(t),
            AbstractValue::Ty(t) => Fp::Data(sem.widen(t)),
            AbstractValue::Unknown => Fp::Unknown,
            AbstractValue::Closure { .. } => Fp::Unknown,   // placeholder until step 07
        };
        self.fps.intern(fp)
    }

    pub fn key(&mut self, sem: &Sem<'db>, func: Definition<'db>, bound: &Bound<'db>) -> ContextId {
        let args = bound.slots.iter().map(|v| self.fingerprint(sem, v)).collect();
        self.contexts.intern(Context { func, args })
    }
}
```

`Default` for `Interner` needs a manual impl (`IndexVec::new()`,
`FxHashMap::default()`), since `K` and `I` don't implement `Default`.

</details>

---

## Run

```rust
#[test]
fn p02_01_keys() {
    check("02-function-summaries/python/keys.py");
    let stats = stats_for("02-function-summaries/python/keys.py", "main", &Options::default());
    assert_eq!(stats.walks, 19);
    assert_eq!(stats.contexts, 12);
}
```

Also add a `contexts` assertion to step 01.13's test: `wall.py` has 7.

The 12 is right for now. Step 04 lowers it to 11 (`note` never reads `msg`,
so its three calls share a key), and says so when you get there.

---

## Where you are

```
Bound   { slots: [Values] }                     one per parameter
Context { func, args: [FpId] }                  interned → ContextId
Fp      = Unknown | Data | Function | Module | Union
```

Every call has a key. Nothing reads them yet.

---

## Exercise

1. `def f(x): pass` is called as `f(a)`, `f(a if c else a)`, and `f()` with
   `x=a` as its default. How many keys?
2. Why does an empty `Values` (no value reaches the argument) key as
   `Unknown`, and not as an empty `Union`?
3. `wall.py`'s 7 keys: is any of them a key with arguments? What does that say
   about why the wall was so wasteful?

<details><summary>Answers</summary>

1. One. `a if c else a` is `[a, a]`, deduplicated to `[a]`, and the default
   fills the slot with the same value, so all three are
   `Context { f, [Function(a)] }`.
2. The call still gets an edge and its callee is walked (step 01.10), so it
   needs a key. "Nothing known about this parameter" is exactly what `Unknown`
   says. An empty `Union` would be a second spelling of the same thing, and
   two spellings of one key means two walks that find the same.
3. None of them has an argument. Every one of the 15 walks had exactly the
   same inputs as some other walk of the same function: the wall is pure
   repetition, the best case for reuse.

</details>

---

→ Next: [`02-the-summary.md`](02-the-summary.md)
