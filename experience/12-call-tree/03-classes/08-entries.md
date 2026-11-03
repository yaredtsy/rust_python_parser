# 03.08 — Entries

**You build:** what an entry starts from. Every function the analyser is asked
about is its own system, started like `main`: callers never matter, and an
unknown parameter stays unknown. But a method can't start without an object,
and a closure can't start without its parent's run. So an entry starts with a
**prologue**: walk down from the module to the entry, through every enclosing
function and class, running what Python would run to get there. Each function
on the way is run (its run is what the next level reads free variables from);
each object a method needs is constructed (`__init__` walked). The prologue's
effects are kept; its nodes are not shown.
**Fixture:** `python/entries.py`
**Plan:** `plan/03-call-tree/16-entries.md`

---

## The fixture

```python
class Pipeline:
    def __init__(self, handler): self.handler = handler
class Svc:
    def __init__(self):
        self.p = Pipeline(JsonHandler())
        log()
    def run(self): self.p.handler.process()      # Svc.run > JsonHandler.process
    def build(self):
        def step(): self.p.handler.process()     # Svc.build.step > JsonHandler.process
        return step

class Child(Base):                               # Base.__init__(self, h): self.h = h
    def __init__(self): super().__init__(JsonHandler())
    def go(self): self.h.process()               # Child.go > JsonHandler.process

class Job:
    def __init__(self): self.h = A()
    def configure(self): self.h = B()
    def run(self): self.h.go()                   # Job.run > A.go   (only __init__ starts it)

class Reg:
    def __init__(self): log()
    @classmethod
    def create(cls): cls()                       # Reg.create > Reg > log
    @staticmethod
    def make(cb): cb()                           # Reg.make: cb is unknown

def outer():
    h = A()
    def inner(): h.go()                          # outer.inner > B.go
    h = B()

def with_param(h: Handler):                      # functions in functions
    def inner(): h.handle()                      # with_param.inner > Handler.handle   (outer's parameter)
def with_default():
    h = A()
    def inner(x=h): x.go()                       # with_default.inner > A.go   (a default: taken at `def`)
    h = B()
def siblings():
    def helper(): log()
    def inner(): helper()                        # siblings.inner > siblings.helper > log
def branches(c):
    if c: h = A()
    else: h = B()
    def inner(): h.go()                          # branches.inner > A.go, B.go
def recursive():
    def walk(n): log(); walk(n)                  # recursive.walk > log, recursive.walk [recursive]

def a():                                         # three levels of functions
    x = A()
    def b():
        y = B()
        def c(): x.go(); y.go()                  # a.b.c > A.go, B.go
        return c
    return b

def make():                                      # a class inside a function
    h = JsonHandler()
    class Local:
        def __init__(self): self.h2 = h
        def run(self): h.process(); self.h2.process()   # make.Local.run > JsonHandler.process ×2
    return Local

class Outer:                                     # a class inside a class
    class Inner:
        def __init__(self): self.h = JsonHandler()
        def m(self): self.h.process()            # Outer.Inner.m > JsonHandler.process

class Host:                                      # a class inside a method
    def __init__(self): self.h = JsonHandler()
    def meth(self):
        host = self
        class Local:
            def m(self): host.h.process()        # Host.meth.Local.m > JsonHandler.process
        return Local

def setup(holder):                               # not an entry: ordinary code
    h = A()
    def cb(): h.go()
    holder.cb = cb                               # the closure escapes
    h = B()
def fire():
    holder = Holder(); setup(holder); holder.cb()   # fire > …, setup.cb > B.go

def process(h: Handler, n=3): h.handle()         # process > Handler.handle
# and Box.h (a property), Tree.walk (a recursive __init__, hidden)
```

Today's code, step 07, gets the flat cases with no setup right and the rest
wrong:

| entry | step 07 | wanted |
|---|---|---|
| `Svc.run` | nothing | `JsonHandler.process` |
| `Child.go` | nothing | `JsonHandler.process` |
| `Job.run` | `A.go`, `B.go` | `A.go` |
| `outer.inner`, `with_param.inner`, `with_default.inner`, `siblings.inner`, `branches.inner`, `recursive.walk`, `a.b.c`, `Svc.build.step`, `make.Local.run`, `Host.meth.Local.m` | `(no function …)` | found, reading their parents' runs |
| `fire` (ordinary code) | `setup.cb > A.go` | `setup.cb > B.go` |

## Predict

1. Step 07 gives `Svc.run`'s `self` a fresh `Svc` with no state. Where does
   `self.p` come from then, and why does `.handler.process()` find nothing?
2. Why does step 07 show `B.go` for `Job.run`? Is that wrong?
3. `Svc.run`'s prologue constructs a `JsonHandler` and a `Pipeline`, and calls
   `log()`. Should those be nodes in `Svc.run`'s tree? Should `Reg.create`'s
   `cls()` be?
4. `make.Local.run` reads `h`. `h` is a local of `make`, and `Local` sits in
   between. Which run does `run` read `h` from, and what is `Local`'s part in
   it?
5. `Outer.Inner.m`: should the prologue construct an `Outer`?

---

## Build

```
inline_tree(entry)
  ├─ prologue(entry)                      new, src/interp/entry.rs
  │    for each scope from the module down to the entry:
  │      a class     → remember it (nothing runs)
  │      a function  → its receiver: a method of the remembered class gets an object
  │                    (build_object: walk __init__, hidden)
  │                    the entry? stop: Start { receiver, enclosing }
  │                    otherwise run it (walk_inline, hidden): enclosing = its run
  ├─ path = [entry]                       after the prologue: a clean path
  └─ walk the entry with Start's receiver and enclosing run, and the objects' state
```

### Decision 1: every function is its own system, started like `main`

The analyser runs per function, when it is opened and when it changes. So an
entry's tree may only depend on the entry and what it reaches from its own
start, never on who calls it. A start is "what Python must run to reach this
function", with unknown arguments.

For `Job.run`, that is a fresh `Job`, then `run`: `A.go`. `B.go` needs a
`configure()` first, which is not part of the start. Step 07 gave `self` no
state, so every read went to the class side, where ty's type of `h` is the
union of every assignment in the class: `A | B`. That answers "any `Job` at any
time", which is a different question.

**Rejected: seeding from callers.** "Use the data the callers pass" makes the
tree depend on which callers exist and have been analysed. A change in one
caller would change another function's tree.

### Decision 2: the scope chain, from ty's index

```rust
// src/sem.rs
pub enum Scope<'db> { Class(Definition<'db>), Function(Definition<'db>) }

/// The classes and functions around `def`, outermost first, then `def` itself.
pub fn scope_chain(&self, def: Definition<'db>) -> Vec<Scope<'db>> {
    let file = def.program_file(self.db);
    let index = semantic_index(self.db, file);
    let parsed = self.parsed(def);
    let model = self.model(file);
    let mut chain: Vec<Scope<'db>> = index
        .ancestor_scopes(def.scope(self.db).file_scope_id(self.db))
        .filter_map(|(_, scope)| match scope.node() {
            NodeWithScopeKind::Class(c) => Some(Scope::Class(c.node(&parsed).definition(&model))),
            NodeWithScopeKind::Function(f) => Some(Scope::Function(f.node(&parsed).definition(&model))),
            _ => None, // the module; type-parameter scopes (`def f[T]`, `class C[T]`) hold nothing
        })
        .collect();
    chain.reverse();
    chain.push(Scope::Function(def));
    chain
}
```

`Svc.build.step` gives `[Class(Svc), Function(build), Function(step)]`.
`make.Local.run` gives `[Function(make), Class(Local), Function(run)]`.

A lambda or a comprehension is a scope too, but a `def` or `class` statement
can't be inside one, so they never appear between the module and an entry.

### Decision 3: walk down, running each function and building each object a method needs

```rust
// src/interp/entry.rs
pub fn prologue(&mut self, sol: &mut Solver<'db>, entry: Definition<'db>,
                env: &mut Env<'db>) -> Start<'db> {
    let none = EvaluatedArgs::default();
    let mut enclosing = None;     // the run of the nearest enclosing *function*
    let mut class = None;         // the class just stepped into, if the next step is its method
    for scope in sol.sem.scope_chain(entry) {
        match scope {
            Scope::Class(c) => class = Some(c),            // a class body is not a run
            Scope::Function(f) => {
                let receiver = match class.take() {
                    Some(c) => self.receiver_for(sol, c, f, enclosing, env),
                    None => None,                          // a plain function or a closure
                };
                if f == entry {
                    return Start { receiver, enclosing };
                }
                let run = self.walk_inline(sol, f, enclosing, receiver, &none, env);   // hidden
                sol.stats.prologue_walks += 1;
                enclosing = run.act;
            }
        }
    }
    unreachable!("the chain ends with the entry")
}
```

Three rules make this right at any depth:

- **`enclosing` skips classes.** Python looks up a free variable in the
  enclosing *functions*, never in a class body. So stepping into a class leaves
  `enclosing` alone: `Local`'s methods read `make`'s run.
- **An object is built only for a method.** `class` is consumed by the next
  step (`take`). A class inside a class replaces it: `Outer.Inner.m` builds an
  `Inner`, never an `Outer`, because `Inner`'s methods can't see an `Outer`
  instance.
- **Each function run gets the receiver its own step decided.** `Svc.build.step`
  builds a `Svc` for `build`, runs `build` on it, then `step` reads `self` as a
  free variable from that run.

`Start` is what the entry's walk begins with:

```rust
#[derive(Default)]
pub struct Start<'db> {
    pub receiver: Option<AbstractValue<'db>>,
    pub enclosing: Option<ActivationId>,
}
```

### Decision 4: ty says what a method is

```rust
// src/interp/entry.rs
/// `func` is defined in `class`: what it is called on.
fn receiver_for(&mut self, sol: &mut Solver<'db>, class: Definition<'db>, func: Definition<'db>,
                enclosing: Option<ActivationId>, env: &mut Env<'db>) -> Option<AbstractValue<'db>> {
    match sol.sem.entry_kind(class, func) {
        EntryKind::Method => Some(self.build_object(sol, class, enclosing, env)),
        EntryKind::ClassMethod => Some(AbstractValue::Class(class)),
        EntryKind::StaticMethod => None,
    }
}
```

```rust
// src/sem.rs
pub enum EntryKind { Method, ClassMethod, StaticMethod }

/// A function in `class`, by what ty makes of it on an instance: the same order as `member`.
pub fn entry_kind(&self, class: Definition<'db>, func: Definition<'db>) -> EntryKind {
    let name = self.function_name(func);
    if self.property_parts(class, &name).is_some() {
        return EntryKind::Method; // a property's getter runs on an object
    }
    let file = class.program_file(self.db);
    match self.member(self.instance_type(class), &name, file).map(|t| (t, self.shape(t, file))) {
        Some((t, Shape::BoundMethod { .. })) => match self.bound_to(t) {
            BoundTo::Class => EntryKind::ClassMethod,
            _ => EntryKind::Method,
        },
        Some((_, Shape::Function(_))) => EntryKind::StaticMethod, // no receiver on an instance
        _ => EntryKind::Method, // ty can't tell (an unknown decorator): a method
    }
}
```

**Why ty, not the decorator list:** `@classmethod`, `classmethod(f)` and a
decorator ty sees through all give the same answer, and it is the answer
`member` already uses when the method is called (steps 02, 06).

### Decision 5: building the object

```rust
// src/interp/entry.rs
/// A new `class` object with `__init__` walked on it, hidden.
fn build_object(&mut self, sol: &mut Solver<'db>, class: Definition<'db>,
                enclosing: Option<ActivationId>, env: &mut Env<'db>) -> AbstractValue<'db> {
    let site = sol.sem.def_range(class);                  // the class statement: one object
    let origin = self.origins.intern(OriginKey::New { site, act: self.frame.act });
    let obj = AbstractValue::Instance { class, origin };
    if let Some(init) = sol.sem.init_of(class).filter(|&i| sol.sem.is_project(i)) {
        // an inherited `__init__` lives in another scope chain: it gets no run of ours
        let defined_here = matches!(sol.sem.scope_chain(init).iter().rev().nth(1),
                                    Some(Scope::Class(c)) if *c == class);
        let run = if defined_here { enclosing } else { None };
        self.walk_inline(sol, init, run, Some(obj), &EvaluatedArgs::default(), env);
        sol.stats.prologue_walks += 1;
    }
    obj
}
```

`init_of` goes through the MRO, so `Child.go` runs `Child.__init__`, whose
`super().__init__(JsonHandler())` runs `Base.__init__` on the same object
(step 07). `__init__`'s own parameters get the entry rules: defaults,
annotations, or `Unknown`.

**Why `walk_inline`, not `construct`:** `construct` makes a node for the class
and puts `__init__`'s calls under it. `walk_inline` only walks: no node, and its
`children` are dropped. That is what "hidden" means. Its effects stay: the
object's state is in `env.attrs` when `walk_inline` returns (step 03).

**Why `__init__` gets the class's `enclosing` run:** in `make.Local`,
`self.h2 = h` reads `make`'s local. `__init__` is a function inside `Local`
inside `make`, so its free variables come from `make`'s run, like `run`'s. An
`__init__` inherited from a class defined somewhere else has a different chain,
so it gets none.

A class whose `__init__` is not project code (none, a library base, a
dataclass) gives an object with no state, and every read goes to the class
side, as before.

### Decision 6: free variables are read at the end of the parent's run

Python reads a free variable when the closure runs, and an entry closure runs
after its parent set it up. In `outer`, that is after `h = B()`, so `B.go`.

`walk_inline` needs two small changes:

```rust
pub struct InlineRun<'db> {
    pub children: Vec<Node>,
    pub returns: Values<'db>,
    pub act: Option<ActivationId>,                        // ← new: the run, for the prologue
}

// walk_inline: keep the run's id, and store the env it ended with
let new = self.new_frame(sol, def);
let act = new.act;                                        // ← new
// … walk, as before …
if let Some(exit) = Env::join(callee.exits.into_iter().map(Some).collect()) {
    self.activations[act] = exit.clone();                 // ← new: the run as it ended
    env.attrs = exit.attrs;
}
```

`walk_stmts` stores the env *before* each statement, for closures called during
the run. Nothing stored the env *after* the last one. Without it, `inner` would
read the env before `h = B()`, and show `A.go`.

This applies to every inline call, not only prologues: a closure that escapes
its run and is called later now reads the end of that run. That is Python's
late binding. `fire` tests it in ordinary code: `setup` stores `cb` on an
object, then sets `h = B()` as its last statement, and `fire` calls `cb` after
`setup` returned. Without the store, `cb` reads `h = A()`. Run every earlier
test after this change.

Summary mode has the same gap: `walk_context` doesn't store the end env either,
and `finish` reads a returned closure's free values from that run. Store it
there too.

If a run never reaches the next `def` or `class` (it comes after an
unconditional `return`), the next level has no run state for it. Its free
variables then fall to ty, as they do with no run at all.

### Decision 7: hidden nodes, a clean path

```rust
// src/interp/mod.rs, inline_tree
let mut w = Walk::new(&s, def);
let mut objects = Env::default();                         // what the prologue leaves behind
let start = w.prologue(&mut s, def, &mut objects);
s.path.push(qname.clone());                               // after the prologue
s.stats.walks += 1;                                       // the entry
let bound = w.bind_parameters(&mut s, def, node, start.receiver, &EvaluatedArgs::default(), start.enclosing);
s.key(&w, def, &bound, start.enclosing);
let mut env = w.env_from_bound(node, &bound);
env.scope = Some(s.sem.body_scope(def, node));
env.enclosing = start.enclosing;
env.attrs = objects.attrs;                                // the objects' state
w.walk_stmts(&mut s, &node.body, env);
```

**Why the path is pushed after the prologue:** the prologue walks other
functions, and if one of them calls the entry, that is not the entry recursing.
`Tree.walk`'s prologue runs `Tree.__init__`, which constructs a `Tree`, which
runs `__init__` again: a `[recursive]` leaf, inside the prologue, hidden.
`Tree.walk`'s tree is just `log`.

**What is shown and what isn't:** the prologue's calls are setup, not part of
the entry. `Svc.run`'s tree has no `JsonHandler`, `Pipeline` or `log` node:
those are `Svc.__init__`'s, and `Svc.__init__` is its own entry with its own
tree. `Reg.create`'s `cls()` is the entry's own call, so `Reg > log` is shown.

### Decision 8: retire the enclosing-class `self`, name nested functions

Step 07 filled an unannotated `self` / `cls` from the enclosing class
(`enclosing_self`, called from `annotation_fallback` when there is no
receiver). The prologue replaces it at entries. Remove it, with its `no_receiver`
parameter and `enclosing_class` (`scope_chain` covers it).

Away from an entry it also fired for any call with no receiver and an unknown
first argument (`Greeter.greet(u, cb)` with `u` unknown), and invented an object
there. With it gone, an unknown first parameter stays unknown, like any other.

`sem.entry(file, "a.b.c")` stepped into classes only. One more arm:

```rust
Stmt::ClassDef(c) if !last && c.name.as_str() == part => next = Some(&c.body[..]),
Stmt::FunctionDef(f) if !last && f.name.as_str() == part => next = Some(&f.body[..]),   // ← new
```

### Not yet

- **Methods of a local class called during the entry's walk.** In
  `make.Local.run`, the prologue gives `run` and `__init__` `make`'s run. But a
  `self.other()` inside `run` is a bound method call, and `call_function` walks
  `other` with no enclosing run: a `Class` value doesn't carry the run its class
  statement ran in. Its free variables fall to ty. The fix is a `Class` value
  that carries its run, like `Closure` does. Add it when a fixture shows the
  loss.
- **Summary mode.** The same prologue, walked in the root walk of `solve`; the
  entry is keyed with its values. Step 04.03, where part 03's fixtures switch to
  both modes.
- **The module.** The walk starts below the module: module globals still come
  from ty (step 03.04's `OPAQUE` objects). Running the module body would be one
  more level of the same walk, with its own costs; it is a separate decision.

### Yours

- `sem.scope_chain`, `sem.entry_kind`, `sem.def_range`, `sem.function_name`;
  the `FunctionDef` arm in `sem.entry`.
- `src/interp/entry.rs`: `Start`, `prologue`, `receiver_for`, `build_object`.
  `walk_inline` becomes `pub(crate)`.
- `InlineRun.act`, and the run's end env stored in `walk_inline`.
- `inline_tree`: the prologue before the path, the entry walked from `Start`.
- `Stats.prologue_walks`.
- Retire `enclosing_self`, `enclosing_class`, `annotation_fallback`'s
  `no_receiver`.

<details><summary>Reference: the small helpers</summary>

```rust
// src/sem.rs
pub fn def_range(&self, def: Definition<'db>) -> TextRange {
    def.full_range(self.db, &self.parsed(def)).range()
}
pub fn function_name(&self, def: Definition<'db>) -> String {
    let parsed = self.parsed(def);
    self.function_node(def, &parsed).map_or_else(String::new, |f| f.name.to_string())
}
```

```rust
// src/interp/entry.rs: imports
use crate::interp::bind::EvaluatedArgs;
use crate::interp::env::Env;
use crate::interp::solver::Solver;
use crate::interp::value::{AbstractValue, ActivationId, OriginKey};
use crate::interp::walk::Walk;
use crate::sem::{Definition, EntryKind, Scope};
```

</details>

---

## Run

```rust
#[test]
#[ignore = "step 03.08 (entries) is not built yet"]   // already in fixture.rs: remove this line
fn p03_08_entries() {
    check("03-classes/python/entries.py");
}

#[test]
fn p03_08_prologue_runs_the_parents() {
    let f = "03-classes/python/entries.py";
    let walks = |entry| stats_for(f, entry, &inline()).prologue_walks;
    assert_eq!(walks("process"), 0);           // a module function: nothing to set up
    assert_eq!(walks("Reg.create"), 0);        // a classmethod: no object
    assert_eq!(walks("outer.inner"), 1);       // outer ran
    assert_eq!(walks("with_param.inner"), 1);  // with_param ran, h from its annotation
    assert_eq!(walks("with_default.inner"), 1);
    assert_eq!(walks("siblings.inner"), 1);    // helper is not run: it is only defined
    assert_eq!(walks("branches.inner"), 1);
    assert_eq!(walks("recursive.walk"), 1);
    assert_eq!(walks("a.b.c"), 2);             // a, then b
    assert_eq!(walks("Svc.run"), 1);           // Svc.__init__
    assert_eq!(walks("Svc.build.step"), 2);    // Svc.__init__, then build
    assert_eq!(walks("Host.meth.Local.m"), 2); // Host.__init__, then meth (Local has no __init__)
}
```

The trees show the prologue's *effects* (an edge that only exists if the parent
ran). The counter shows *what* ran, so a tree that comes out right for another
reason doesn't hide a missing step.

**The function-in-function trees are the same as through a call.** Each nested
function, reached by an ordinary call (`with_default()()`, `siblings()()`, …),
gives exactly the subtree its entry block expects. That is the contract: an
entry differs from a call only in its arguments (unknown) and its context (the
prologue), never in how the body is walked.

Then every earlier test: `Service.call` in `chains.py` now gets its `Service`
from the prologue. `Service.__init__(self, handler: Handler)` runs with
`handler` from its annotation, so `self.handler` is a `Handler` object and the
edge is still `Handler.handle`, by a different route.

---

## Where you are

```
inline_tree:  prologue → path [entry] → walk the entry from Start
prologue:     down the scope chain: a function is run (its run is the next enclosing),
              a class is remembered; a method of it gets a built object
walk_inline:  returns its run; stores the env it ended with
retired:      enclosing_self, enclosing_class
```

---

## Exercise

1. Add `def reset(self): self.p = Pipeline(A())` to `Svc`. Does `Svc.run`'s tree
   change? Does `Svc.reset`'s?
2. Without the end-of-run store, what does `outer.inner` show, and why does
   `a.b.c` still come out right?
3. `Reg.create`'s prologue builds nothing, yet its tree has `Reg > log`. Why is
   that `log` shown when `Svc.run`'s `log` isn't?
4. `make.Local.run`: write the scope chain, and say what `enclosing` and
   `class` hold after each step.
5. Why must the prologue run before the entry's name goes on the path?

<details><summary>Answers</summary>

1. `Svc.run`: no. `reset` isn't part of the start, so `self.p` is still
   `__init__`'s `Pipeline` with its `JsonHandler`. `Svc.reset`'s tree has an `A`
   node then a `Pipeline` node: its own constructions, shown.
2. `A.go`: the run was last stored before its last statement, `h = B()`.
   `a.b.c` comes out right by luck: `a` and `b` end with `return`, and the env
   stored before a `return` already has everything.
3. `cls()` is a call the entry makes, so its node is in the entry's tree.
   `Svc.run`'s `log()` is made by `Svc.__init__` during the prologue: setup,
   which belongs to `Svc.__init__`'s own tree.
4. `[Function(make), Class(Local), Function(run)]`. After `make`: `enclosing` =
   `make`'s run, `class` = none. After `Local`: `enclosing` unchanged (a class is
   not a run), `class` = `Local`. At `run`: `class` is taken, `entry_kind` says
   method, `build_object(Local, make's run)` walks `Local.__init__` with
   `self.h2 = h` read from `make`'s run; `run` is the entry, so `Start` is that
   object and `make`'s run.
5. The prologue walks other functions. If one of them reached the entry, the
   guard would mark it `[recursive]` (a hidden node, but the walk would stop
   there and its effects would be lost), though the entry was never running.

</details>

---

→ Next part: [`../04-class-summaries/README.md`](../04-class-summaries/README.md)
