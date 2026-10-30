# 01.11 — Closures

**You build:** nested `def`s as values. A closure remembers the run of the
function that defined it, and its free variables are read from that run, at
the moment they are used.
**Fixture:** `python/closures.py`
**Plan:** `plan/03-call-tree/11-closures-and-callbacks.md` § "Closures",
`05-the-walk.md` § "Activations"

---

## The fixture

```python
def make(f):
    def run():
        f()
    return run

def escaping():
    r = make(b)
    r()                       # make, make.run > b

def caller_has_same_name():
    f = a                     # must not leak into run
    r = make(b)
    r()                       # make, make.run > b

def late():
    f = None
    def run(): f()
    f = b
    run()                     # late.run > b

def two_runs():
    x = make(a)
    y = make(b)
    x()                       # make.run > a
    y()                       # make.run > b

def setup():
    def handler(cb=pick()):   # pick is called here
        cb()
    handler()                 # pick, setup.handler > a

def entry_closure():
    f = pick()
    def inner(): f()
    inner()                   # pick, entry_closure.inner > a
```

(`same_frame` is in the file too.)

## Predict

1. In `escaping`, `run` is called after `make` has returned. Where does `f`'s
   value live at that moment?
2. `caller_has_same_name` has its own `f = a`. Which rule from step 05 keeps it
   out of `run`?
3. `late` assigns `f = b` *after* the `def`. Does Python's `run` see `None` or
   `b`? What does that say about *when* a free variable must be read?

---

## Build

### Decision 1: a `def` statement binds a value. It does not run the body

```rust
Stmt::FunctionDef(d) => {
    for dec in &d.decorator_list {
        self.eval(&dec.expression, &mut env);         // runs now, in this function
    }
    let mut defaults = Vec::new();
    for p in d.parameters.iter_non_variadic_params() {
        if let Some(default) = p.default.as_deref() {
            let v = self.eval(default, &mut env);     // runs now: `pick()` is setup's call
            defaults.push((p.parameter.name.id.clone(), v));   // and its value is kept (decision 6)
        }
    }
    let func = self.sem.definition_of(&self.frame.model, d);
    self.defaults.insert((func, self.frame.act), defaults);
    env.bind(&d.name.id, smallvec![AbstractValue::Closure { func, activation: self.frame.act }]);
}
```

**Why:** the body of `handler` runs when `handler()` is called, so its calls
belong under that node. The decorators and defaults run when the `def`
statement runs, so their calls belong to the function containing the `def`.
That is why `pick` is a child of `setup`, not of `handler`.

This is also the plan's warning about scope entry: a walk that starts at
`def.body` instead of at the `def` statement silently drops the default's
call.

### Decision 2: a closure remembers the run that made it

```rust
pub enum AbstractValue<'db> {
    Function(Definition<'db>),
    Closure { func: Definition<'db>, activation: ActivationId },   // ← new
    Module(Type<'db>),
    Ty(Type<'db>),
    Unknown,
}
```

An **activation** is one run of one body: a slot in the walk holding that
run's env. Every frame gets one when it starts.

```rust
pub struct Walk<'a, 'db> {
    pub sem: &'a Sem<'db>,
    pub frame: Frame<'db>,
    pub path: Vec<String>,
    pub activations: IndexVec<ActivationId, Env<'db>>,   // ← new: every run so far
}
pub struct Frame<'db> { /* … */ pub act: ActivationId }   // this run's slot
```

`two_runs` calls `make` twice: two runs, two activations, two different
closures with the same `func`. That is why the value carries the activation,
not just the function.

**A gap until step 03.03:** if two such closures reach *one* call site
(`h = make(a) if c else make(b); h()`), `eval_call` calls each, and inline
mode pushes two `make.run` nodes under that site. The rule is one node per
site and function (step 05). Step 03.03's `merge_into` fixes it, for this
case as well as for several receivers. No fixture in this part has that
shape, so nothing fails before then.

### Decision 3: free variables come from where the `def` ran, never the caller

```rust
fn lookup(&self, env: &Env<'db>, name: &Name) -> Option<Values<'db>> {
    if let Some(v) = env.get(name) {
        return Some(v.clone());                         // a local of this run
    }
    // Only a real free variable walks outward: ty's semantic index says which
    // enclosing *function* scope binds this name.
    let bound_in = self.sem.free_variable_scope(env.scope, name)?;
    let mut cur = env.enclosing;
    while let Some(act) = cur {
        let outer = &self.activations[act];
        if outer.scope == bound_in {
            return outer.get(name).cloned();
        }
        cur = outer.enclosing;
    }
    None                                                // no run to read: ty answers
}
```

`Env` gains two fields that never change during a run:

```rust
pub struct Env<'db> {
    pub locals: rpds::HashTrieMap<Name, Values<'db>>,
    pub scope: BodyScope<'db>,                 // which function body these locals belong to
    pub enclosing: Option<ActivationId>,       // where this function's `def` ran. NEVER the caller
}
```

When `call_function` calls a `Closure { func, activation }`, the callee's env
gets `enclosing = Some(activation)`. A module-level `Function` gets `None`:
its free names are module globals, and ty answers them.

**Why "enclosing" and not "caller":** `caller_has_same_name` calls `run` from a
function that has its own `f`. Python resolves `f` lexically, to `make`'s `f`.
Walking the caller's env would find `a`. This is the scoping rule a quick port
gets wrong, and the Jedi driver gets right: its contexts point to where a
function was *defined* (`plan/03-call-tree/01` § "Closures over
enclosing-function locals").

**Why ask ty whether a name is free:** a name not in the locals could be a free
variable, a module global, or a builtin. Only the first one walks outward.
ty's semantic index already knows which scope binds each name, so ask it
instead of guessing from the env.

**Rejected:** copying the free values into the closure when the `def` runs.
That breaks `late`: Python reads the variable when it is used, not when the
function was defined.

### Decision 4: an activation holds the run's env *now*

`walk_stmts` writes the env into its run's slot before each statement:

```rust
for stmt in body {
    self.activations[self.frame.act] = env.clone();     // what closures read
    env = self.walk_stmt(stmt, env)?;
}
```

When `run()` is called in `late`, the slot holds the env after `f = b`, so the
closure reads `b`. That is Python's late binding, for free.

### Decision 5: in inline mode, activations outlive their frame

`escaping` calls `run` after `make`'s frame is gone. Its activation is still in
`Walk.activations`, because inline mode is one walk for the whole request
(step 05, decision 2).

Part 02 can't do this. There, `make` is walked once, as its own walk, and its
summary must not contain an `ActivationId`, since that number means nothing to
any other walk. The closure has to carry its free values with it instead. You
will build that in `02.07`, and this fixture is its test.

### Decision 6: a nested def's defaults are the values computed when the `def` ran

Python evaluates a default once, when the `def` statement runs. Decision 1
already evaluates it right there, for its calls. So keep the value:

```rust
// in Walk
pub defaults: FxHashMap<(Definition<'db>, ActivationId), Vec<(Name, Values<'db>)>>,
```

When a closure `Closure { func, activation }` is called, `bind_parameters`
takes a missing argument's value from `defaults[(func, activation)]`. Only a
module-level function falls back to step 07's rule (ty's type of the default
expression), because the module body is not walked.

**Why not ask ty, as step 07 does:** ty types a call by the callee's
*annotated* return type, and never infers one from the body (ty's own tests:
`mdtest/function/return_type.md`). `def handler(cb=pick())` with an
unannotated `pick` would be `Unknown` for ty, and `handler`'s `cb()` would lose
its edge. Your walk has already computed `pick()`'s value on this path.

**Why keyed by the activation:** two runs of `setup` evaluate the default
twice, possibly to different values; each run's `handler` closure must use its
own. That is also exactly Python: a default is bound to the function object
the `def` made, once.

`def inner(h=h)` (the idiom that captures a loop variable's current value)
works for the same reason: `h` is read when the `def` runs, not when `inner`
is called.

### Yours

- `ruff_index` in `Cargo.toml` (`{ path = "vendor/ruff/crates/ruff_index" }`)
  for `IndexVec`, and an `ActivationId` made with ruff's `#[newtype_index]`.
  It is an **attribute**, not a `macro!{}`: put it on a unit struct, as in the
  reference below. It derives `Copy`, `Clone`, `Eq`, `PartialEq`, `Hash` and
  `Debug`, and `Idx` (what `IndexVec` needs), but not `Ord`: add
  `#[derive(Ord, PartialOrd)]` when you need to sort ids, as ty does.
- `sem.definition_of(model, def_stmt)`, `sem.body_scope(def)`,
  `sem.free_variable_scope(scope, name)`.
- `Closure`, `activations`, `Frame.act`, `Env.scope`, `Env.enclosing`,
  `Walk.defaults`.
- The `FunctionDef` arm (keeping the defaults' values), `lookup`, closures in
  `eval_call` and `call_function`, and `default_value` for closures.
- `call_tree` starting the entry like a call: parameters bound, scope set.

<details><summary>Reference: this step's code</summary>

```rust
// src/sem.rs (added)
use ty_python_core::scope::{FileScopeId, ScopeKind};
use ty_python_core::semantic_index;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BodyScope<'db> {
    pub file: ProgramFile<'db>,
    pub scope: FileScopeId,
}

impl<'db> Sem<'db> {
    pub fn definition_of(&self, model: &SemanticModel<'db>, d: &ast::StmtFunctionDef) -> Definition<'db> {
        d.definition(model)
    }

    /// The scope of a function's body. [check] the accessor name.
    pub fn body_scope(&self, def: Definition<'db>, node: &ast::StmtFunctionDef) -> BodyScope<'db> {
        let file = def.program_file(self.db);
        let index = semantic_index(self.db, file);
        BodyScope { file, scope: index.node_scope(NodeWithScopeRef::Function(node)) }
    }

    /// If `name` is a free variable of `scope`, the enclosing FUNCTION scope
    /// that binds it. `None` for a local, a global or a builtin.
    pub fn free_variable_scope(&self, scope: BodyScope<'db>, name: &str) -> Option<BodyScope<'db>> {
        let index = semantic_index(self.db, scope.file);
        let mut ancestors = index.ancestor_scopes(scope.scope);
        ancestors.next(); // the scope itself
        if index.place_table(scope.scope).symbol_by_name(name).is_some_and(|s| s.is_bound()) {
            return None; // bound here: a local, even if not assigned yet on this path
        }
        for (id, s) in ancestors {
            match s.kind() {
                ScopeKind::Class | ScopeKind::TypeParams => continue, // skipped by name resolution
                ScopeKind::Module => return None,                    // a global
                _ => {
                    if index.place_table(id).symbol_by_name(name).is_some_and(|s| s.is_bound()) {
                        return Some(BodyScope { file: scope.file, scope: id });
                    }
                }
            }
        }
        None
    }
}
```

```rust
// src/interp/walk.rs (changed parts)
use ruff_index::{IndexVec, newtype_index};

#[newtype_index]
pub struct ActivationId;

pub struct Walk<'a, 'db> {
    pub sem: &'a Sem<'db>,
    pub frame: Frame<'db>,
    pub path: Vec<String>,
    pub activations: IndexVec<ActivationId, Env<'db>>,
}

impl<'a, 'db> Walk<'a, 'db> {
    pub fn new_frame(&mut self, func: Definition<'db>) -> Frame<'db> {
        let act = self.activations.push(Env::default());
        let file = func.program_file(self.sem.db);
        Frame { func, file, model: self.sem.model(file), children: Vec::new(), returns: Values::new(), act }
    }
}
```

```rust
// src/interp/call.rs: closures are callable too
for callee in &callees {
    match *callee {
        AbstractValue::Function(def) if self.sem.is_project(def) =>
            out.extend(self.call_function(def, None, site, &args)),
        AbstractValue::Closure { func, activation } if self.sem.is_project(func) =>
            out.extend(self.call_function(func, Some(activation), site, &args)),
        _ => ask_ty = true,
    }
}

// in call_function(def, enclosing, site, args), after switching frames:
let mut env = self.bind_parameters(def, node, args, enclosing);   // defaults[(def, enclosing)]
env.scope = Some(self.sem.body_scope(def, node));
env.enclosing = enclosing;
```

```rust
// src/interp/bind.rs: a missing argument's default
fn default_value(&mut self, func: Definition<'db>, enclosing: Option<ActivationId>,
                 p: &ParameterWithDefault) -> Option<Values<'db>> {
    let expr = p.default.as_deref()?;
    if let Some(act) = enclosing {
        // a nested def: the value computed when its `def` ran
        return self.defaults.get(&(func, act))?
            .iter().find(|(n, _)| *n == p.parameter.name.id).map(|(_, v)| v.clone());
    }
    let ty = self.sem.type_of(&self.frame.model, ExprRef::from(expr));   // module level: step 07
    Some(self.lift(ty))
}
```

**The entry needs the same start as a call.** `call_tree` must start the
entry's walk exactly the way `call_function` starts a callee: bind the
parameters (no arguments, so each is its default or `Unknown`) and set
`env.scope` to the entry's body scope. If the entry's env has no scope, a
closure defined in the entry finds the entry's activation but can't match its
scope, so `lookup` falls through to ty. ty knows simple cases (`f = a`), which
hides the bug; `entry_closure` in the fixture is the case it doesn't know.
The simplest way to get this right is to make `call_tree` use the same
function as a call does, for the entry.

`Env::default()` with `scope: None` is fine for blocks inside a run (they
inherit it), never for the start of one. Joins keep the first arm's `scope`
and `enclosing`: every arm is the same run.

</details>

---

## Run

```rust
#[test]
fn p01_11_closures() {
    check("01-functions/python/closures.py");
}
```

If `caller_has_same_name` shows `a`, `lookup` is reading the caller's env:
check what `enclosing` is set to for the call. If `late` shows nothing,
the activation slot is being read before `f = b` was written into it.

---

## Where you are

```
Walk  { sem, frame, path, activations: ActivationId → Env }
Frame { func, file, model, children, returns, act }
Env   { locals, scope, enclosing }
AbstractValue = Function | Closure { func, activation } | Module | Ty | Unknown
```

---

## Exercise

1. `def outer(): x = a; def mid(): def inner(): x(); return inner; return mid()()`.
   Trace `x()` in `inner`: which activations does `lookup` visit?
2. Why does `free_variable_scope` return `None` for a name bound in the scope
   itself, even when the env has no value for it yet?
3. In part 02, `make` is walked once and its summary is reused by both of
   `two_runs`'s calls. What must the summary say about the returned `run` so
   that `x()` finds `a` and `y()` finds `b`?

<details><summary>Answers</summary>

1. `inner`'s env has no `x`; ty says `x` is bound in `outer`'s scope. `lookup`
   starts at `inner`'s `enclosing`, `mid`'s activation: wrong scope, so it
   follows `mid`'s `enclosing`, `outer`'s activation: right scope, `x = a`.
2. A name bound anywhere in a function is local to that whole function in
   Python. If the env has no value yet, Python raises `UnboundLocalError` at
   that point. Walking outward would find an outer variable with the same
   name, which Python never reads.
3. That it returns a closure over `run` whose free variable `f` is "whatever
   the caller passed as `make`'s parameter 0". Each caller then fills that in
   with its own argument: `a` for `x`, `b` for `y`. That is `02.02` (values
   relative to the inputs) and `02.07` (closures).

</details>

---

→ Next: [`12-callbacks-and-decorators.md`](12-callbacks-and-decorators.md)
