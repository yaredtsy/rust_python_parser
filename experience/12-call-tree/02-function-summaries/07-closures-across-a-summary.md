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

def chain(n):
    if n: r = chain(n - 1)
    else: r = a
    def g(): r()
    return g
def chained(): chain(3)()                               # chain.g > a, chain.g [recursive]

def make_walker():
    def walk(n): walk(n)
    return walk                                         # a returned closure that captures itself
def use_walker(): make_walker()(1)                      # make_walker.walk > make_walker.walk [recursive]

def with_default():
    def walk(n, cb=a): cb(); walk(n)                    # the recursive call takes cb's default
    return walk
def use_default(): with_default()(1)                    # with_default.walk > a, with_default.walk [recursive]
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
        free: Box<[(Name, SVals<'db>)]>,       // captured variables
        defaults: Box<[(Name, SVals<'db>)]>,   // the defaults computed when its `def` ran (step 01.11)
    },
    Back(u8),                                  // ← new: the closure k levels up (decision 5)
}
```

**Why `Box<[…]>` and not `SmallVec`:** `SVals` is a `SmallVec`, which keeps
its first two elements inline. A `SmallVec` of `SVals` inside an `SVal` would
put `SVal`s inside an `SVal` by value: a type of infinite size, which Rust
rejects (E0072). A box is a pointer, so the cycle goes through the heap, the
same way `Bound { recv: Box<SVal> }` will in part 04. Closures in summaries
are rare, so the allocation costs nothing measurable, and a boxed slice is
fixed once built, like the rest of a summary. It collects from an iterator
(`.collect()`), and `same_named` already takes slices, so the code below
doesn't change. Rejected: `SVals = Vec<…>` everywhere (every summary's
returns would pay a heap allocation for a closure-only problem), and
interning closures by id (another table, for no measured gain).

`finish` converts a returned `Closure { func, activation }` by reading each of
`func`'s free variables (and its defaults) from that activation, and
converting those the same way. `to_sval` becomes recursive:

```rust
// src/interp/summary.rs
/// Walk-local values as summary data. `resolved` is what each pending value
/// came to (step 05). `depth` counts the closures these values sit inside, and
/// `cl` holds them (decision 5), so a closure that captures itself is `Back(k)`.
fn to_svals(&self, s: &Solver<'db>, vals: &Values<'db>, resolved: &IndexVec<DeferId, Values<'db>>,
            depth: u8, cl: &mut ClosureStack<'db>) -> SVals<'db> {
    let mut out = SVals::new();
    for &v in vals {
        match v {
            AbstractValue::Pending(id) => out.extend(self.to_svals(s, &resolved[id], resolved, depth, cl)),
            AbstractValue::Closure { func, activation } => match cl.position(func, activation) {
                Some(k) => out.push(SVal::Back(k)),
                None if depth == s.opts.fp_depth => out.push(self.closure_at_limit(s, func, activation)),
                None => out.push(cl.with(func, activation, |cl| SVal::Closure {
                    func,
                    free: s.sem.free_variables(func).iter()
                        .map(|n| (n.clone(), self.to_svals(s, &self.lookup_in(s, func, activation, n), resolved, depth + 1, cl)))
                        .collect(),
                    defaults: self.defaults.get(&(func, activation)).into_iter().flatten()
                        .map(|(n, v)| (n.clone(), self.to_svals(s, v, resolved, depth + 1, cl)))
                        .collect(),
                })),
            },
            other => out.push(SVal::Plain(plain(other))),   // step 02's to_sval, for plain values
        }
    }
    // Two activations can give equal summary values: keep one.
    let mut unique = SVals::new();
    for x in out {
        if !unique.iter().any(|y| same_sval(&x, y)) { unique.push(x) }
    }
    unique
}
```

`closure_at_limit` keeps the function and makes every captured value and
default `[Plain(Unknown)]`.

`lookup_in` reads a closure's free variable from the run where its `def` ran.
It is step 01.11's `lookup`, not a second rule:

```rust
// src/interp/walk.rs
/// A free variable of the closure `(func, act)`, read exactly as its own body
/// would: `lookup` from an empty env of `func`'s scope whose `enclosing` is
/// `act`. Empty if no run has bound it.
pub fn lookup_in(&self, s: &Solver<'db>, func: Definition<'db>, act: ActivationId, name: &Name) -> Values<'db> {
    let parsed = s.sem.parsed(func);
    let Some(node) = s.sem.function_node(func, &parsed) else { return Values::new() };
    let probe = Env { scope: Some(s.sem.body_scope(func, node)), enclosing: Some(act), ..Env::default() };
    self.lookup(s, &probe, name).unwrap_or_default()
}
```

**Why not "the nearest activation that holds the name":** that rule differs
from `lookup` when the scope that binds the name hasn't assigned it on the
walked path:

```python
def outer():
    f = a
    def mid():
        def inner(): f()
        return inner        # mid binds f, but never assigns it before returning
        f = b
```

Python: `inner()` raises, because `mid`'s `f` is unbound. "Nearest that holds
it" skips `mid`'s activation and finds `outer`'s `f = a`: an edge Python
never takes. `lookup` asks ty which scope binds `f` (`mid`), reads that run
only, and finds nothing.

`finish_round` converts the returns with
`self.to_svals(s, &self.frame.returns, resolved, 0, &mut ClosureStack::default())`.
The closure stack is the one decision 5 builds for keys: the same value is
treated the same way in a key and in a summary.

`make`'s summary for `make(f = a)` returns
`Closure { run, free: [(f, [Plain(Function(a))])], defaults: [] }`. Nothing in
it is walk-local.

**Why the free values go through `resolved` too:** a closure can capture a
recursive call's value (`r = chain(n - 1); def g(): r()`). Its free value is
then `Pending(id)`, which means nothing outside this walk. `SVal` has no
variant for it, so it can't leak, but without substituting it would turn into
`Unknown`, and `g` would lose its edge. Substituting gives it this round's
value, exactly like the returns.

**Why a depth limit, `fp_depth` (2):** substituting lets a summary contain the
previous round's summary, one level deeper every round. In `chain`, write
`g[X]` for a closure of `g` whose `r` is `X`:

| round | `chain` returns |
|---|---|
| 0 | `g[a]` |
| 1 | `g[g[a], a]` |
| 2 | `g[g[g[a], a], a]` |
| … | one level deeper |

No two rounds are equal, so step 05's fixpoint would never stop. This isn't
an exotic shape: it is how a middleware chain is built. With the limit, a
closure inside `fp_depth` other closures keeps its function, and every value
it captures becomes `Unknown`. Write `g[?]` for that. Round 2 is
`g[g[g[?], a], a]`, round 3 is the same, and the fixpoint stops. The tree
loses nothing: the renderer stops at `chain.g [recursive]` before the depth
matters. Keys need the same limit (decision 5): a deferred call is keyed with
each round's values, and a key that grew every round would be a new context,
and a new walk, every round.

**Why keep the function at the limit, not cut to `Unknown`:** the closure is
still callable, and its body's own calls (to module functions, to its
parameters) don't depend on what it captured. In a chain of *different*
handlers, `h1` capturing `h2` capturing `h3`, cutting to `Unknown` would never
walk `h3` at all. Keeping the function walks it, with its captures unknown.
It is still finite: a closure at the limit holds no closure. (This is the
plan's rule, `plan/03-call-tree/08-keys.md` § "Depth".)

`fp_depth` is `Options.fp_depth`, 2. Part 04 uses the same limit for objects'
attributes, and step 08 moves it into `Limits`.

**Why `Back` in summaries too:** a closure can be returned while it captures
itself (`make_walker`). Without `Back`, `to_svals` would expand `walk` inside
`walk` until the depth limit cut it: `walk[walk[Unknown]]`. The caller would
rebuild two closures over two activations, and the outer one's walk would
call the inner one: a second context of `walk`, whose `walk(n)` calls
`Unknown`. The tree hides it (the renderer stops at
`make_walker.walk [recursive]`), but there would be two contexts where one
belongs, and a key and a summary would disagree about the same value. With
the closure stack, `make_walker` returns `walk[walk = Back(0)]`, the caller
rebuilds one closure whose `walk` is itself, and there is one context.

**Comparing closures:** step 05's `same_sval` was `==`. A closure's captures
are value sets too, so their order must not matter either:

```rust
fn same_sval<'db>(x: &SVal<'db>, y: &SVal<'db>) -> bool {
    match (x, y) {
        (SVal::Closure { func: f, free: a, defaults: da },
         SVal::Closure { func: g, free: b, defaults: db }) => f == g && same_named(a, b) && same_named(da, db),
        _ => x == y,
    }
}

/// Both lists come in a fixed order: free variables sorted, defaults in parameter order.
fn same_named<'db>(a: &[(Name, SVals<'db>)], b: &[(Name, SVals<'db>)]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|((n, x), (m, y))| n == m && same_svals(x, y))
}
```

**Why this is rule 2 again:** activation 7 of `make`'s walk is meaningless to
`two_levels`'s walk. If the summary carried it, `two_levels` would read its own
activation 7, some unrelated run. In part 01 one walk held every activation,
which is the only reason escaping closures worked there.

**When the captured values are read.** A *returned* closure is read when its
function's walk ends. That is right: `make`'s run is over and its variables
never change again, which is the moment Python's cell holds its final value.
A closure *passed as an argument* is read at the call (decision 3). That is
right when the callee calls it before the caller rebinds the variable, and
wrong when the callee stores it and it runs later:

```python
def setup():
    f = a
    def h(): f()
    register(h)       # keyed with f = a
    f = b             # Python's h, run later, calls b
```

Today the stored closure is lost anyway (containers and objects arrive in
part 04), but once an object can hold it, it keeps `f = a`. So there are two
known limits, both about a captured variable rebound after the closure left:
this one, and two closures that share a variable and rebind it with
`nonlocal` (each carries its own copy). The plan pins both:
`plan/03-call-tree/15` § "Known limits".

`sem.free_variables(func)` lists every name used in `func` or in a function
nested inside it that resolves to a scope *outside* `func` that is a function.
Module globals are not free variables: ty answers them on every path. A
`def` can't sit inside a lambda or a comprehension, so for now "a function"
means a `def`. If lambdas are ever entered (step 01.12), names bound in a
lambda or comprehension scope count too: `lambda f: lambda: f()` and
`[lambda: g() for g in (a, b)]`.

The closure's **defaults** travel the same way. Step 01.11 keeps them in
`Walk.defaults[(func, activation)]`, keyed by an activation, which is
walk-local. So `finish` writes them into the `SVal::Closure` beside the free
values, and whoever rebuilds the closure puts them back under its new
activation.

### Decision 2: the caller resolves a closure into a new activation of its own

```rust
// src/interp/summary.rs
pub fn resolve(&mut self, s: &Solver<'db>, vals: &SVals<'db>) -> Values<'db> {
    self.resolve_in(s, vals, &mut Vec::new())
}

/// `cl`: the closures being rebuilt, innermost last. `Back(k)` is the k-th from the top.
fn resolve_in(&mut self, s: &Solver<'db>, vals: &SVals<'db>,
              cl: &mut Vec<(Definition<'db>, ActivationId)>) -> Values<'db> {
    let mut out = Values::new();
    for sv in vals {
        out.push(match sv {
            SVal::Plain(p) => from_plain(p),                       // step 02's resolve
            SVal::Back(k) => {
                let (func, activation) = cl[cl.len() - 1 - *k as usize];
                AbstractValue::Closure { func, activation }
            }
            SVal::Closure { func, free, defaults } => {
                let activation = self.activations.push(Env::flat()); // 1. the activation, empty
                cl.push((*func, activation));                         // 2. so a Back inside can reach it
                for (name, vals) in free.iter() {                     // 3. its values
                    let v = self.resolve_in(s, vals, cl);
                    self.activations[activation].bind(name, v);
                }
                let d = defaults.iter().map(|(n, v)| (n.clone(), self.resolve_in(s, v, cl))).collect();
                self.defaults.insert((*func, activation), d);
                cl.pop();                                             // 4.
                AbstractValue::Closure { func: *func, activation }
            }
        });
    }
    out
}
```

The new activation is **flat**: it doesn't stand for one run of one scope, it
holds the free values by name. `lookup` (step 01.11) gains one case: a flat
activation answers any name it holds, whatever scope ty says binds it. The
four numbered steps are the rule for rebuilding any closure (decision 5).

**Optional: resolve equal closures to one activation.** Every `resolve` makes
a new activation, so two equal summary closures become two values with
different activations, and so does the same closure in every fixpoint round.
That costs a duplicate value and a duplicate `get`, never correctness: keys
and summaries compare captured values, not activations. If profiling shows it
matters, keep a per-walk `FxHashMap<SVal, ActivationId>` and reuse the
activation for a top-level closure (`cl` empty) seen before. Only at the top
level: a nested one can hold a `Back` that points outside itself. It is safe
because nothing writes to a flat activation (`SVal` then derives `Hash`).

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
    pub free: SmallVec<[(Name, FpId); 2]>,       // ← new: the called closure's own free values
    pub defaults: SmallVec<[(Name, FpId); 2]>,   // ← new: and its own defaults
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

**Why the called closure's defaults too:** `args` already holds a default
wherever the call left a parameter out. But the walk can call the closure
again, from inside (`walk(n)` in `with_default`), and that call takes its
defaults from the closure's activation. `materialize` has to put them there,
and rule 1 says it can only take them from the key. Without them, the
recursive call binds `cb = Unknown`, keys a second context, and walks it
with less. `Context` now mirrors `Fp::Closure`: function, captures, defaults.

**Rejected:** a `Context` that holds the called closure's whole fingerprint
(`closure: Option<FpId>`) instead of `free` and `defaults`. It would remove a
special case or two, but part 04's paths name a captured value as
`Root::Free(i)`, an index into `Context.free`, so the list stays.

### Decision 4: a walk starts its free variables from the key

`materialize` puts `Context.free` into a new flat activation and makes it the
walk's `enclosing`. A `Closure` fingerprint in a parameter becomes a closure
over a new flat activation, the same way. Both follow the rebuilding rule at
the end of decision 5. The walk reads nothing else: rule 1.

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
    None if depth == self.opts.fp_depth => self.fp_closure_at_limit(func, activation),   // decision 1
    None => closures.with(func, activation, |cl| Fp::Closure {
        func,
        free: self.fp_free(w, activation, cl, depth + 1),
        defaults: self.fp_defaults(w, func, activation, cl, depth + 1),   // from w.defaults[(func, activation)]
    }),
},
```

`fp_closure_at_limit` is `Fp::Closure` with the function kept and every
captured value and default `Unknown`, matching `closure_at_limit` in
summaries.

`fingerprint` gains a `depth` argument: 0 for a value in a parameter slot,
one more inside each closure. The called closure counts as a closure too: its
own free values (`Context.free`) are fingerprinted at depth 1, exactly where
a returned closure's free values sit in a summary. So a key and a summary keep
the same number of nested closures. Part 04 counts objects' attributes with
the same `depth`.

`walk`'s key is `Context { walk, [Unknown], free: [(walk, Back(0))] }`. Inside
its walk, `walk(n)` keys to the same context, so the recursion is handled as
recursion: `Pending`, a `[recursive]` leaf, one context. Two nested defs that
call each other are the same rule, with `Back(1)`.

**Rebuilding a closure, wherever it appears.** Two places turn data back into
closures: `materialize` (from a key's `Fp`) and `resolve` (from a summary's
`SVal`, decision 2). Both use one rule, with a stack of the closures being
rebuilt:

1. create the closure's flat activation, empty;
2. push `(func, activation)`;
3. bind its free values and defaults, rebuilding nested closures by the same
   rule; `Back(k)` becomes a closure over the k-th entry from the top;
4. pop.

The activation has to exist before its values are bound, because a value
inside it can point back at it. In `materialize`, the stack starts with the
called closure itself, so `Back(0)` in `Context.free` is the walked closure,
and `Context.defaults` go under `(func, activation)` like any rebuilt
closure's defaults.
A self-capturing closure passed as an argument (`call_it(walk)`) has
`Back(0)` inside its parameter's fingerprint, and two nested defs that call
each other give `Back(1)`. All of them rebuild by these four steps:

```rust
// from_fp, the closure arms. `cl` as in resolve_in.
Fp::Back(k) => {
    let (func, activation) = cl[cl.len() - 1 - k as usize];
    smallvec![AbstractValue::Closure { func, activation }]
}
Fp::Closure { func, free, defaults } => {
    let activation = self.activations.push(Env::flat());
    cl.push((*func, activation));
    for (name, fp) in free {
        let v = self.from_fp(s, *fp, cl);
        self.activations[activation].bind(name, v);
    }
    let d = defaults.iter().map(|(n, fp)| (n.clone(), self.from_fp(s, *fp, cl))).collect();
    self.defaults.insert((*func, activation), d);
    cl.pop();
    smallvec![AbstractValue::Closure { func: *func, activation }]
}
```

### Yours

- `SVal::Closure`, `Fp::Closure`, `Fp::Back`, `Context.free`.
- `sem.free_variables(func)`.
- `SVal::Back`, `Context.defaults`.
- `lookup_in`, through step 01.11's `lookup`.
- `closure_at_limit` and `fp_closure_at_limit`.
- `to_svals` with `resolved`, `depth` and the closure stack; `same_sval` for
  closures; `Options.fp_depth` (2); `fingerprint`'s `depth`.
- `finish` and `resolve` for closures, with the rebuilding rule; flat
  activations and the `lookup` case.
- `fingerprint` with the closure stack; `key` fills `free` (at depth 1) when
  the callee is a closure; `materialize` builds `enclosing` from it, by the
  same rebuilding rule.
- `contexts_of`, for the tests.
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
and fingerprints its free values and defaults at depth 1 (into
`Context.free` and `Context.defaults`), so `walk`'s own free value `walk`
comes out as `Back(0)`. `to_svals` uses the same `ClosureStack`.

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
    let st = stats_for("02-function-summaries/python/closures_across.py", "chained", &summary());
    assert_eq!(st.rounds_capped, 0);  // the depth limit, not the round cap, ends chain's fixpoint
    let f = "02-function-summaries/python/closures_across.py";
    assert_eq!(contexts_of(f, "chained", "chain.g"), 3);                // the depth limit, pinned without the renderer
    assert_eq!(contexts_of(f, "use_walker", "make_walker.walk"), 1);    // Back in the summary
    assert_eq!(contexts_of(f, "use_default", "with_default.walk"), 1);  // Context.defaults
}
```

```rust
// src/interp/fixture.rs
/// How many contexts of `func` summary mode makes, solving from `entry`.
pub fn contexts_of(fixture: &str, entry: &str, func: &str) -> usize {
    let path = std::fs::canonicalize(format!("experience/12-call-tree/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let module = path.file_stem().unwrap().to_str().unwrap();
    let (s, _) = solve(&db, file, entry, &summary()).unwrap();
    let want = format!("{module}.{func}");
    s.contexts.iter().filter(|(_, c)| s.sem.qname(c.func) == want).count()
}
```

`chain.g` has three contexts: the closure `chained` calls, the one it
captured, and the one at the depth limit, whose `r` is `Unknown`. That last
one is still walked, because at the limit a closure keeps its function. Two
would mean the limit cuts the function too; four or more would mean the
limit isn't applied to keys. `with_default.walk` has one: its recursive call
finds `cb`'s default in the rebuilt closure, so it keys the same context.

Every part 01 fixture now runs in both modes.

---

## Where you are

```
Fp      += Closure { func, free, defaults }, Back(k)
Context += free
SVal    += Closure { func, free: [(Name, SVals)], defaults: [(Name, SVals)] }, Back(k)
activations: per walk. A flat activation holds free values by name
closures nest at most fp_depth (2) deep, in keys and in summaries
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
