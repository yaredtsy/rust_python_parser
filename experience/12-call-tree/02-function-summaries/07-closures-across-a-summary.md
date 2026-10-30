# 02.07 — Closures across a summary

**You build:** closures in summary mode. A closure value holds an
`ActivationId`, which means nothing outside its walk. So when a closure
crosses a call, in a key or in a summary, it carries its **free values**
instead. The receiving walk builds a new activation from them.
**Fixtures:** `python/closures_across.py`, and `../01-functions/python/closures.py`
joins summary mode
**Plan:** `plan/03-call-tree/11-closures-and-callbacks.md` § "Escaping closures",
`08-keys.md` § "Fingerprinting" (closures, `Back`), § "Materializing",
`09-summaries.md` § "Values relative to the inputs"

---

## The fixture

```python
def make(f):
    def run(): f()
    return run
def call_it(cb): cb()
def wrap(f): return make(f)

def passed_along(): call_it(make(a))                    # call_it > make.run > a
def two_makers():   call_it(make(a)); call_it(make(b))  # … > a, … > b
def two_levels():   wrap(b)()                           # wrap > make; make.run > b

def outer(root):
    def walk(n):
        walk(n)
    walk(root)                                          # outer.walk > outer.walk [recursive]
```

## Predict

1. `make`'s summary returns "the closure `run`". In step 01.11 that value was
   `Closure { func: run, activation: 7 }`. Why can't the summary say that?
2. `call_it(make(a))` and `call_it(make(b))`: same key for `call_it`, or two?
3. In `outer`, `walk` calls itself through its own free variable. What goes
   wrong if the key of `walk` includes "the fingerprint of the value of
   `walk`"?

---

## Build

### Decision 1: a closure leaves a walk carrying its free values

```rust
pub enum SVal<'db> {
    Plain(PlainVal<'db>),
    Closure {                                                     // ← new
        func: Definition<'db>,
        free: SmallVec<[(Name, SVals<'db>); 2]>,      // captured variables
        defaults: SmallVec<[(Name, SVals<'db>); 2]>,  // the defaults computed when its `def` ran (step 01.11)
    },
}
```

`finish` converts a returned `Closure { func, activation }` by reading each of
`func`'s free variables from that activation:

```rust
AbstractValue::Closure { func, activation } => SVal::Closure {
    func,
    free: s.sem.free_variables(func).iter()
        .map(|name| (name.clone(), self.lookup_in(activation, name).into_iter().map(to_sval).collect()))
        .collect(),
}
```

`make`'s summary for `make(f = a)` returns
`Closure { run, free: [(f, [Plain(Function(a))])] }`. Nothing in it is
walk-local.

**Why this is rule 2 again:** activation 7 of `make`'s walk is meaningless to
`two_levels`'s walk. If the summary carried it, `two_levels` would read its own
activation 7, some unrelated run. In part 01 one walk held every activation,
which is the only reason escaping closures worked there.

**Why reading the activation at the end is right:** when `make` has returned,
its run is over and its variables never change again. That is the moment
Python's cell holds its final value. (One known limit remains: two closures
that share a variable and rebind it with `nonlocal` each carry their own copy.
The plan pins it; `plan/03-call-tree/15` § "Known limits".)

`sem.free_variables(func)` lists every name used in `func` or in a function
nested inside it that resolves to a scope *outside* `func` that is a function.
Module globals are not free variables: ty answers them on every path.

The closure's **defaults** travel the same way. Step 01.11 keeps them in
`Walk.defaults[(func, activation)]`, keyed by an activation, which is
walk-local. So `finish` writes them into the `SVal::Closure` beside the free
values, and whoever rebuilds the closure puts them back under its new
activation.

### Decision 2: the caller resolves a closure into a new activation of its own

```rust
SVal::Closure { func, free, defaults } => {
    let mut env = Env::flat();                        // holds free values by name, no scope
    for (name, vals) in free {
        let v = self.resolve(s, vals);
        env.bind(name, v);
    }
    let activation = self.activations.push(env);
    let defaults = defaults.iter().map(|(n, v)| (n.clone(), self.resolve(s, v))).collect();
    self.defaults.insert((*func, activation), defaults);
    AbstractValue::Closure { func: *func, activation }
}
```

The new activation is **flat**: it doesn't stand for one run of one scope, it
holds the free values by name. `lookup` (step 01.11) gains one case: a flat
activation answers any name it holds, whatever scope ty says binds it.

### Decision 3: a closure in a key is its function plus its free values

```rust
pub enum Fp<'db> {
    // …
    Closure {
        func: Definition<'db>,
        free: SmallVec<[(Name, FpId); 2]>,       // sorted by name
        defaults: SmallVec<[(Name, FpId); 2]>,   // the defaults it was made with
    },
    Back(u8),                                                               // decision 5
}

pub struct Context<'db> {
    pub func: Definition<'db>,
    pub args: SmallVec<[FpId; 4]>,
    pub free: SmallVec<[(Name, FpId); 2]>,   // ← new: the called closure's own free values
}
```

The free values are read from **the closure's own activation**, never from the
caller's env. That is step 01.11's rule, now in the key.

`call_it(make(a))` and `call_it(make(b))` give `call_it` two keys:
`Closure { run, [(f, Function(a))] }` and `Closure { run, [(f, Function(b))] }`.
And `run` itself, when called, gets `Context { run, [], free: [(f, …)] }`, so
the two `run`s are two contexts, with two different children.

**Why not just the function:** two closures over the same `def` with different
captured values do different things. That is the whole of `two_makers`.

### Decision 4: a walk starts its free variables from the key

`materialize` puts `Context.free` into a new flat activation and makes it the
walk's `enclosing`. A `Closure` fingerprint in a parameter becomes a closure
over a new flat activation, the same way. The walk reads nothing else: rule 1.

### Decision 5: a closure that captures itself is `Back(k)`

```python
def outer(root):
    def walk(n):
        walk(n)
```

`walk` has one free variable, `walk`, whose value is the closure itself.
Fingerprinting it naively recurses forever: the fingerprint of `walk` contains
the fingerprint of its free value, which is `walk`.

While fingerprinting a closure, keep a stack of the closures in progress,
compared by `(func, activation)`. A free value equal to one on the stack
becomes `Back(k)`, its distance up the stack:

```rust
AbstractValue::Closure { func, activation } => match closures.position(func, activation) {
    Some(k) => Fp::Back(k),
    None => closures.with(func, activation, |cl| Fp::Closure {
        func,
        free: self.fp_free(w, activation, cl),
        defaults: self.fp_defaults(w, func, activation, cl),   // from w.defaults[(func, activation)]
    }),
},
```

`walk`'s key is `Context { walk, [Unknown], free: [(walk, Back(0))] }`. Inside
its walk, `walk(n)` keys to the same context, so the recursion is handled as
recursion: `Pending`, a `[recursive]` leaf, one context. Two nested defs that
call each other are the same rule, with `Back(1)`.

`materialize` turns `Back(0)` in `Context.free` back into the walked closure
itself: create the flat activation, then bind the name to a closure over that
same activation.

### Yours

- `SVal::Closure`, `Fp::Closure`, `Fp::Back`, `Context.free`.
- `sem.free_variables(func)`.
- `finish` and `resolve` for closures; flat activations and the `lookup` case.
- `fingerprint` with the closure stack; `key` fills `free` when the callee is
  a closure; `materialize` builds `enclosing` from it.
- Switch `closures.py`'s test to `check_both`.

<details><summary>Reference: this step's code</summary>

```rust
// src/sem.rs
/// Names `func` (or a function nested in it) reads that are bound in an
/// enclosing FUNCTION scope. Sorted. [check] the scope walk below.
pub fn free_variables(&self, func: Definition<'db>) -> Vec<Name> {
    let parsed = self.parsed(func);
    let Some(node) = self.function_node(func, &parsed) else { return Vec::new() };
    let own = self.body_scope(func, node);
    let index = semantic_index(self.db, own.file);
    let mut out = Vec::new();
    for scope in std::iter::once(own.scope).chain(index.descendant_scopes(own.scope)) {
        for symbol in index.place_table(scope).symbols() {
            if !symbol.is_used() { continue }
            let at = BodyScope { file: own.file, scope };
            if let Some(bound) = self.free_variable_scope(at, symbol.name()) {
                // free in `scope`, and bound outside `func`
                if !index.is_ancestor_or_self(own.scope, bound.scope) {
                    out.push(symbol.name().clone());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}
```

`descendant_scopes` and `is_ancestor_or_self` stand for a walk over
`child_scopes` and `ancestor_scopes`, both public on the semantic index.

```rust
// src/interp/key.rs: the closure stack
pub struct ClosureStack<'db>(SmallVec<[(Definition<'db>, ActivationId); 2]>);

impl<'db> ClosureStack<'db> {
    pub fn position(&self, f: Definition<'db>, a: ActivationId) -> Option<u8> {
        self.0.iter().rev().position(|&(g, b)| g == f && b == a).map(|k| k as u8)
    }
    pub fn with<R>(&mut self, f: Definition<'db>, a: ActivationId, body: impl FnOnce(&mut Self) -> R) -> R {
        self.0.push((f, a));
        let r = body(self);
        self.0.pop();
        r
    }
}
```

`key` starts the stack with the callee itself when the callee is a closure,
so `walk`'s own free value `walk` comes out as `Back(0)`.

</details>

---

## Run

```rust
#[test]
fn p02_07_closures() {
    check_both("02-function-summaries/python/closures_across.py");
    check_both("01-functions/python/closures.py");
    let st = stats_for("02-function-summaries/python/closures_across.py", "outer", &summary());
    assert_eq!(st.contexts, 2);       // outer, and one context of walk
}
```

Every part 01 fixture now runs in both modes.

---

## Where you are

```
Fp      += Closure { func, free }, Back(k)
Context += free
SVal    += Closure { func, free: [(Name, SVals)], defaults: [(Name, SVals)] }
activations: per walk. A flat activation holds free values by name
```

This is the end of what plain functions need. The next step makes the solver
safe on inputs that are simply too big.

---

## Exercise

1. `def make(f): def run(): f(); return run`, and `make(a)` is called from
   two different functions. How many contexts of `make`, of `run`?
2. Why does the closure stack compare `(func, activation)` and not `func`
   alone? Find a case where `func` alone gives the wrong key.
3. `late` from step 01.11 (`f = None; def run(): f(); f = b; run()`) runs
   inside one walk in summary mode. When `run()` is keyed, where are its free
   values read from, and what do they hold?

<details><summary>Answers</summary>

1. One of each. Both calls give `make` the key `[Function(a)]`, and the
   returned closure's free values are equal, so `run` keys the same both
   times.
2. Two different runs of the same `def` can be on the stack at once: a closure
   whose free variable holds *another* closure over the same `def` from a
   different run (`make(make(a))`'s `run` captures the inner `run`). Comparing
   `func` alone would call the inner one `Back(0)`, a self-reference, and the
   key would claim the closure captures itself. With the activation included,
   the inner one gets its own `Closure` fingerprint.
3. From `run`'s own activation: the activation of `late`'s run, the one the
   `def` bound. The walk writes that slot before every statement, so at the
   call it holds the env after `f = b`. The key is
   `Context { run, [], free: [(f, Function(b))] }`.

</details>

---

→ Next: [`08-cost-limits-and-cuts.md`](08-cost-limits-and-cuts.md)
