# 03.05 — The walk

One walk evaluates one function body under one context. It records every call
it reaches, collects the returns, and tracks what it wrote. This chapter is the
statement and expression machinery. What happens at a call is
[`10`](10-one-call.md); how a walk starts from its key is
[`08`](08-keys.md#materializing).

---

## The environment

```rust
/// Persistent and structurally shared. Cloning is a pointer copy.
#[derive(Clone)]
pub struct Env<'db> {
    /// Local bindings of this run: parameters and assigned names.
    locals:  rpds::HashTrieMap<Name, Values<'db>>,

    /// Attribute state of every traced object: entry objects and objects
    /// built in this walk. → 07
    attrs:   rpds::HashTrieMap<(OriginId, Name), Values<'db>>,

    /// Which attributes this walk wrote, and whether on every path. → below
    written: rpds::HashTrieMap<(OriginId, Name), Write>,

    /// The body scope these locals belong to, as (file, FileScopeId).
    /// Not a fresh ScopeId: building one outside a tracked function panics.
    scope:   BodyScope,

    /// The activation where this function's `def` ran. Lookup of a free
    /// variable reads it. NEVER the caller. `None` for module-level functions,
    /// whose free names are module globals and go to ty.
    enclosing: Option<ActivationId>,
}

pub enum Write { Must, May }
```

**There is no caller link.** Two different things could be called "parent":

| | follows | lives in | used for |
|---|---|---|---|
| enclosing | where the function was defined | `Env.enclosing` | free variables of closures |
| caller | where the function was called from | the solver's stack and the call edges | recursion groups, the rendered tree |

Values from a caller arrive one way only: as the context's key, materialized
into the callee's starting env ([`08`](08-keys.md)). Nothing looks a name up
through the caller. If it did, `g` below would see `f`'s `x`:

```python
def g(): x.write()      # x is not local and not free here → ty
def f(x): g()
f(JsonWriter())         # g's x.write must NOT resolve through f's env
```

**Why persistent maps.** One snapshot per call site, one fork per branch arm.
`rpds` insert is O(log n) with structural sharing, and a clone is a pointer
copy. An `FxHashMap` clone is O(n) allocation, and at this call volume it would
be the whole profile.

**Why `scope` is a field.** Two unrelated functions can both have an `x`. A
lookup checks the scope, or walks `enclosing` only for a real free variable.

### Activations

An activation is one run of one body inside this walk, with a slot holding that
run's current env. A walk has:

- its own run, updated after every statement;
- one activation per closure value it builds from its key, holding the
  closure's free values ([`11`](11-closures-and-callbacks.md)), and one for
  its own free variables if the walked function is a closure.

A `def` statement in this walk binds its name to
`Closure { func, activation: <this run> }`. Activations end with the walk.
A closure that leaves the walk becomes an `SVal::Closure` carrying its free
values ([`09`](09-summaries.md)).

### Lookup: locals, then `enclosing` only for a free variable

```rust
fn lookup<'e>(&'e self, env: &'e Env<'db>, name: &Name) -> Option<&'e Values<'db>> {
    if let Some(v) = env.locals.get(name) { return Some(v) }

    // Walk outward ONLY if ty's semantic index says `name` is a free variable
    // of this scope, bound in an enclosing FUNCTION scope.
    let def_scope = free_variable_binding_scope(self.db, env.scope, name)?;
    let mut cur = env.enclosing;
    while let Some(act) = cur {
        let outer = &self.activations[act];      // that run's env RIGHT NOW
        if outer.scope == def_scope { return outer.locals.get(name) }
        cur = outer.enclosing;
    }
    None   // a plain Function: no run to read, the caller falls back to ty
}
```

**[check]** `free_variable_binding_scope` stands for ty's answer to "which
enclosing scope binds this name". Confirm the accessor, and whether it needs a
`types/vnoc.rs` wrapper.

Reading the slot at lookup time gives late binding inside one walk, as in
Python: `h = A(); def inner(): h.go(); h = B(); inner()` resolves to `B.go`.

---

## The walk state

```rust
struct Walk<'db> {
    ctx:         ContextId,
    env:         Env<'db>,
    origins:     Interner<OriginKey, OriginId>,      // identity inside this walk only (04)
    values:      ValueArena<'db>,
    activations: IndexVec<ActivationId, Env<'db>>,
    calls:       Vec<CallEdge<'db>>,                 // every call site reached, evaluation order (09)
    exits:       Vec<Env<'db>>,                      // env at every `return` and at fall-through
    returns:     Values<'db>,
    deferred:    Vec<Deferred<'db>>,                 // empty unless the walk met recursion (12)
    gaps:        Gaps,                               // precision gaps found by this walk (09)
}
```

Functions take `(s: &mut Solver, w: &mut Walk)`, so a call can re-enter the
solver without fighting the borrow checker.

---

## Statements

```rust
/// `None` = this block never falls through (it always returns or raises),
/// so it contributes nothing at a join.
fn walk_stmts(s: &mut Solver<'db>, w: &mut Walk<'db>, stmts: &[ast::Stmt],
              mut env: Env<'db>, act: ActivationId) -> Option<Env<'db>> {
    for stmt in stmts {
        w.activations[act] = env.clone();                  // the slot closures read
        env = match stmt {
            // Fork and join. Every arm starts from the env BEFORE the `if`.
            ast::Stmt::If(st) => {
                eval(s, w, &st.test, &env);                // value unused; its calls are walked
                let before = env;
                let mut arms = vec![walk_stmts(s, w, &st.body, before.clone(), act)];
                for clause in &st.elif_else_clauses {
                    if let Some(t) = &clause.test { eval(s, w, t, &before); }
                    arms.push(walk_stmts(s, w, &clause.body, before.clone(), act));
                }
                if !has_else(st) { arms.push(Some(before)); }   // skipping is an arm
                Env::join(arms)?                                 // every arm left → so does this
            }
            ast::Stmt::Match(m) => { /* eval the subject, then as If: one arm per case */ }

            // Runs zero or more times: walk the body ONCE, join with "didn't run".
            ast::Stmt::For(_) | ast::Stmt::While(_) => {
                eval(s, w, loop_head(stmt), &env);
                let before = env;
                let body = walk_stmts(s, w, loop_body(stmt), bind_loop_target(stmt, before.clone()), act);
                let after = Env::join(vec![Some(before), body])?;
                walk_stmts(s, w, loop_orelse(stmt), after, act)?
            }

            // A handler can start after any statement of the body;
            // approximate its start as before ∪ body-end.
            ast::Stmt::Try(t) => {
                let before = env;
                let body = walk_stmts(s, w, &t.body, before.clone(), act);
                let start = Env::join(vec![Some(before), body.clone()]).unwrap();
                let mut arms = vec![body.and_then(|e| walk_stmts(s, w, &t.orelse, e, act))];
                for h in &t.handlers { arms.push(walk_stmts(s, w, h.body(), start.clone(), act)); }
                let joined = Env::join(arms)?;
                walk_stmts(s, w, &t.finalbody, joined, act)?
            }

            ast::Stmt::With(wi) => {                       // the body always runs
                let env = bind_with_targets(s, w, wi, env);   // evals each context expr, binds `as`
                walk_stmts(s, w, &wi.body, env, act)?
            }

            ast::Stmt::Return(r) => {
                let v = match &r.value {
                    Some(e) => eval(s, w, e, &env),        // walks any call in it
                    None    => none_value(s),              // a bare `return` returns None
                };
                w.returns.extend(v);
                w.exits.push(env);
                return None;
            }
            ast::Stmt::Raise(r) => { /* eval the exception expr(s) */ return None; }

            other => exec_simple(s, w, other, env, act),
        };
    }
    w.activations[act] = env.clone();
    Some(env)
}
```

Falling off the end of the body is an exit too, and returns `None`
(`none_value`: ty's `None`, lifted). A function's returns are empty only when
every path raises, or waits on a recursion group that hasn't produced a value
([`12`](12-recursion.md)).

`Env::join` unions per name: a name any arm wrote gets the union of every
reaching arm's value, and the "before" value counts for an arm that didn't
write it. `attrs` joins the same way. `written` joins as below. `None` arms are
skipped; all `None` gives `None`.

**Why fork and join, when conditions are never evaluated.** Not evaluating a
condition means every arm is possible, not that the arms run in sequence. A
single linear pass gets both directions wrong:

```python
handler = None
if c:
    handler = JsonHandler()
else:
    handler.process()      # linear pass: sees JsonHandler, an edge Python can't take

if c:   w = JsonWriter()
else:   w = CsvWriter()
w.write()                  # linear pass: CsvWriter only, loses JsonWriter.write
```

### Must-write tracking

`written` records every attribute write this walk made or applied, so the
summary can report effects ([`09`](09-summaries.md#finishing-a-walk)).

- An attribute write records `Must`. A weak write records `May`
  ([`07`](07-attributes-and-self.md#write-bind_attribute)).
- An effect applied from a callee records `Must` only if it replaced the old
  value, else `May` ([`10`](10-one-call.md#phase-2-write)).
- `Env::join` keeps `Must` only when every arm that reaches the join has
  `Must`. Anything else becomes `May`.

---

## Evaluation drives the walk

There is no call finder. Calls are found because evaluation reaches them:
`eval` on an `ast::Expr::Call` resolves the call ([`10`](10-one-call.md)) and
returns its value to whoever evaluated it. Python's order falls out of it:

```
eval(emit(make()))
  eval(emit)          → Function(emit)                       1. the function
  eval(make())        → gets make's summary, returns its value   2. the arguments, left to right
  call emit with it   → gets emit's summary                  3. then the call
```

**Every expression is evaluated exactly once per walk.** A second evaluation
would record the call site twice. So `x = y = f()` evaluates `f()` once, and
`exec_simple` never evaluates the right-hand side once for its calls and again
for its value.

**Positions evaluation must reach.** Calls also live outside plain statement
expressions. Each is evaluated by the statement that owns it, in the current
env:

| statement | evaluate |
|---|---|
| `def` | decorators, default values, annotations; then bind the name |
| `class` | decorators, base classes, keywords (`metaclass=`); then bind the name |
| `for` / `while` / `if` / `match` | the iterable, test or subject, before the body |
| `with` | each context expression, before binding `as` |
| `assert`, `del`, `raise`, augmented assignment, `x: T = v` | every expression in it |

**[check]** against the golden files: annotations (evaluated at runtime only
without `from __future__ import annotations`), class bodies inside a function,
comprehensions, and lambdas (a lambda body runs only when called). Match
whatever the Jedi driver reports. A walk that starts at `def.body` misses the
decorators and defaults; start at the `def` node.

Child order in the rendered tree is a separate question
([`14`](14-output.md#determinism)). Never change the evaluation order to get an
output order.

---

## Straight-line statements

```rust
/// Evaluate the statement's expressions once, in Python's order, in the env
/// BEFORE it; then bind.
fn exec_simple(s: &mut Solver<'db>, w: &mut Walk<'db>, stmt: &ast::Stmt,
               env: Env<'db>, act: ActivationId) -> Env<'db> {
    match stmt {
        ast::Stmt::Assign(a) => {
            let v = eval(s, w, &a.value, &env);           // ONCE, however many targets
            let mut env = env;
            for target in &a.targets {
                match target {
                    ast::Expr::Name(n)      => env = env.bind_local(&n.id, v.clone()),
                    ast::Expr::Attribute(x) => env = bind_attribute(s, w, x, v.clone(), env),   // 07
                    other => { eval_target_parts(s, w, other, &env); }   // `a[f()] = v`, tuples: Unknown
                }
            }
            env
        }
        ast::Stmt::AnnAssign(a) => { /* eval annotation [check], then the value once; bind as above */ }
        ast::Stmt::Expr(e)      => { eval(s, w, &e.value, &env); env }

        // A nested def: decorators and defaults run HERE, in this env, when
        // the `def` runs. The name points at THIS run, so a later call reads
        // this run's env as it is at that moment. At module level there is
        // no run; the name goes to ty.
        ast::Stmt::FunctionDef(d) => {
            for dec in &d.decorator_list { eval(s, w, &dec.expression, &env); }
            eval_defaults_and_annotations(s, w, &d.parameters, d.returns.as_deref(), &env);
            if !env.is_function_scope() { return env }
            let func = definition_of_def(s, d);          // decorated → still the def (04)
            env.bind_local(&d.name.id, smallvec![AbstractValue::Closure { func, activation: act }])
        }
        ast::Stmt::ClassDef(c) => {
            for dec in &c.decorator_list { eval(s, w, &dec.expression, &env); }
            for base in c.bases() { eval(s, w, base, &env); }
            for kw in c.keywords() { eval(s, w, &kw.value, &env); }
            env   // [check] class body inside a function: see the positions table
        }
        other => { eval_all_exprs(s, w, other, &env); env }   // assert, del, aug-assign, …
    }
}
```

**Rebinding works, and is common:**

```python
w = JsonWriter(); w.write(1)      # JsonWriter.write
w = CsvWriter();  w.write(2)      # CsvWriter.write
```

This beats ty even where ty could answer. ty narrows `w` here only because both
values are constructors. With `w = build(cfg)`, ty gives the declared return
type and the walk is needed.

### Scope limits

| not modelled | result |
|---|---|
| tuple and list destructuring `a, b = f()` | targets are `Unknown` |
| augmented assignment `x += f()` | `Unknown` |
| `global` / `nonlocal` | `Unknown` |
| walrus `:=` | bound, like a plain assignment |
| loop-carried values | body walked once, joined with "didn't run" |

Do not build a fixed point over loops (v1.2 may, [`15`](15-checks-and-build-order.md#growth-path)).

---

## `eval`

Small on purpose. It handles what carries path information and lifts the rest.

```rust
fn eval(s: &mut Solver<'db>, w: &mut Walk<'db>, expr: &ast::Expr, env: &Env<'db>) -> Values<'db> {
    match expr {
        ast::Expr::Name(n) => match w.lookup(env, &n.id) {
            Some(v) => v.clone(),
            None    => s.lift(w, n.inferred_type(s.model()).unwrap_or(Type::unknown())),
        },
        ast::Expr::Call(c)      => eval_call(s, w, c, env),          // 10
        ast::Expr::Attribute(a) => eval_attribute(s, w, a, env),     // 07
        ast::Expr::Await(a)     => eval(s, w, &a.value, env),        // transparent
        ast::Expr::Lambda(_)    => { /* [check] pin the Jedi driver's behaviour (11) */ }
        ast::Expr::If(t) => {
            eval(s, w, &t.test, env);
            let mut v = eval(s, w, &t.body, env);
            v.extend(eval(s, w, &t.orelse, env));
            v
        }
        ast::Expr::BoolOp(b) => b.values.iter().flat_map(|e| eval(s, w, e, env)).collect(),
        other => { eval_children(s, w, other, env); s.lift(w, other.inferred_type(s.model()).unwrap_or(Type::unknown())) }
    }
}
```

The fallback still evaluates the expression's children (`eval_children`), so
`[make() for _ in xs]` and `f"{make()}"` reach their calls. **[check]**
comprehensions against the golden files.

Each new arm is a place where semantics can diverge from the Jedi driver. The
fallback is already correct, if imprecise.

---

## Fixtures

```python
# 1. rebinding: two different leaves
w = JsonWriter(); w.write(1)
w = CsvWriter();  w.write(2)

# 2. param → local → callee
def f(h):
    x = h
    x.run()

# 3. fork: NO JsonHandler.process edge
handler = None
if c: handler = JsonHandler()
else: handler.process()

# 4. join: BOTH JsonWriter.write and CsvWriter.write
if c: w = JsonWriter()
else: w = CsvWriter()
w.write()

# 5. the return sees the local, not the entry env
def f(h):
    x = h
    return x
f(JsonWriter()).write()

# 6. ONE make edge (evaluated once)
x = y = make()

# 7. decorator and default calls belong to setup, evaluated when the def runs
def setup():
    @register(make())
    def handler(w=make_default()): ...

# 8. make's edge comes before emit's: arguments first
emit(make())

# 9. falling off the end returns None, so log still runs with a value
def setup(): pass
def main(): log(setup())        # log's edge, and log's effects apply
```

---

→ Next: [`06-binding-arguments.md`](06-binding-arguments.md)
