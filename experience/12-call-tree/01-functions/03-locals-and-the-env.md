# 01.03 — Locals and the env

**You build:** the environment: what each local name holds on this path.
Assignments bind into it, and a name is looked up there before ty is asked.
**Fixture:** `python/locals.py`
**Plan:** `plan/03-call-tree/05-the-walk.md` § "The environment", § "Straight-line statements"

---

## The fixture

```python
def a(): pass
def b(): pass
def pick(): return a

def alias():
    f = a
    f()

def rebinding():
    f = a
    f()
    f = b
    f()

def chained():
    f = g = b
    g()

def once():
    f = g = pick()

def walrus():
    (f := b)()
    f()

def unpack():
    f, g = a, b
    g()

# expect:
# alias
#   a
# expect:
# rebinding
#   a
#   b
# expect:
# chained
#   b
# expect:
# once
#   pick
# expect:
# walrus
#   b
#   b
# expect:
# unpack
#   b
```

## Predict

1. Run this fixture against your step 02 code, before writing anything. How
   many blocks pass?
2. In `once`, how many `pick` nodes would there be if each assignment target
   evaluated the right-hand side again?
3. In `unpack`, you don't model tuple targets. What should `g` be after the
   assignment: `Unknown`, its old value, or something else?

---

## An honest note first

Run the fixture now. Most blocks already pass: ty tracks local assignments
inside a function, flow-sensitively, and your step 02 `eval` asks ty about
every name.

So why build an env? Because from step 07 on, a name can hold a value **ty
cannot know**: a parameter, bound to whatever this particular caller passed.
ty sees `def run(cb): cb()` once, for all callers, and `cb` is `Unknown`. Your
walk sees `run` on one path, with `cb = on_ok`. That fact has to live
somewhere, and it is the env.

You build it now, on locals, where ty can check every answer. If your env and
ty ever disagree on this fixture, your env is wrong.

---

## Build

### Decision 1: the env answers first, ty second

```rust
Expr::Name(n) => match env.get(&n.id) {
    Some(v) => v.clone(),                       // this path's value
    None => self.lift(self.sem.type_of(&self.frame.model, expr.into())),   // ty's, for every path
},
```

**Why this order:** the env holds what is true on the path being walked. ty
holds what is true on every path. When both have an answer, the path's answer
is at least as precise. If ty answered first, a parameter would always be its
declared type, and the call tree would lose exactly the edges it exists to
find.

The rule of thumb from the plan: **ty is asked about things that don't depend
on the path.** Module-level names, imports, builtins, literal types. The
moment a path-specific value exists, the env owns it.

### Decision 2: the env is a persistent map

```rust
// src/interp/env.rs
#[derive(Clone, Default)]
pub struct Env<'db> {
    pub locals: rpds::HashTrieMap<Name, Values<'db>>,
}
```

**Why persistent:** in step 09 every `if` copies the env once per arm, and in
part 02 every call takes a snapshot of it. A persistent map shares structure,
so a copy is a pointer copy and an insert is O(log n). An `FxHashMap` copy
allocates the whole map each time, and at one copy per branch and per call it
would be most of the run time.

**Rejected:** one mutable map plus an undo log. It works for a single path,
but step 09 needs two arms alive at once, to join them.

### Decision 3: blocks pass the env by value, expressions by `&mut`

```rust
fn walk_stmts(&mut self, body: &[Stmt], env: Env<'db>) -> Env<'db>
fn eval(&mut self, expr: &Expr, env: &mut Env<'db>) -> Values<'db>
```

A block takes the state before it and returns the state after it. That is
exactly what step 09 needs: give each arm its own copy, get each arm's result
back, join them.

An expression can change the state too. `(f := b)` binds `f` in the middle of
an expression, and in part 03 a call inside an expression can change an
object that a later argument reads. So `eval` gets the env mutably, and
Python's left-to-right order decides who sees what.

**Rejected:** `eval(…, env: &Env)`. It can't carry a walrus binding or a
call's effects to the next argument.

### Decision 4: every right-hand side is evaluated once

```rust
Stmt::Assign(s) => {
    let v = self.eval(&s.value, &mut env);      // ONCE, however many targets
    for target in &s.targets {
        self.bind_target(target, v.clone(), &mut env);
    }
}
```

`f = g = pick()` has two targets and one call. Evaluating the value per target
would record `pick` twice.

### Decision 5: a target you don't model gets ty's type

A plain name is bound to the value. For any other target (`f, g = …`,
`*rest`, `d[k] = …`), every name inside it is bound to **ty's type for that
name**, lifted, and the other sub-expressions are evaluated for their calls
(`d[make()] = v` calls `make`).

```rust
/// A target the walker doesn't model: each name in it gets ty's type for it.
fn bind_from_ty(&mut self, target: &Expr, env: &mut Env<'db>) {
    match target {
        Expr::Name(n) => {
            let ty = self.sem.type_of(&self.frame.model, ExprRef::from(target));
            let v = self.lift(ty);
            env.bind(&n.id, v);
        }
        Expr::Tuple(t) => for e in &t.elts { self.bind_from_ty(e, env) },
        Expr::List(l) => for e in &l.elts { self.bind_from_ty(e, env) },
        Expr::Starred(st) => self.bind_from_ty(&st.value, env),
        other => { self.eval(other, env); }      // `d[k]`, `a.b`: their calls
    }
}
```

**Why not `Unknown`:** ty does model tuple unpacking, so after
`f, g = a, b` it knows `g` is `b`. Binding `Unknown` would throw that away.
**Why not leave the old value:** after `f = a` then `f, g = b, c`, keeping
`f = a` is a wrong answer, not an imprecise one.
**Why not remove the name, so the lookup falls through to ty:** because a
missing name already means something else. In step 09, when two branches
join, a name missing from one branch means "never assigned on that path". A
target ty should answer must be stored, not left out.

ty's answer is always safe here: it is never wrong for this path, at worst
less precise. **[check]** that ty records a type for a name in store context
(`inferred_type` on the target `ExprName`). If it doesn't, bind `Unknown`.

Attribute targets (`self.h = …`) evaluate their receiver for now. Part 03
binds them.

### Yours

- `rpds = "1"` in `Cargo.toml`.
- `interp/env.rs`: `Env` with `get` and `bind`.
- `bind_target` and `bind_from_ty`.
- `walk_stmts` and every statement arm thread `env` through. Compound
  statements still walk their bodies one after the other (step 09 changes
  that), passing the env along.
- `bind_target`, plus walrus (`Expr::Named`) in `eval`.
- `call_tree` starts the entry with `Env::default()`.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/env.rs
use crate::interp::value::Values;
use crate::sem::Name;

/// What each name holds on the path being walked. Cloning is a pointer copy.
#[derive(Clone, Default)]
pub struct Env<'db> {
    pub locals: rpds::HashTrieMap<Name, Values<'db>>,
}

impl<'db> Env<'db> {
    pub fn get(&self, name: &Name) -> Option<&Values<'db>> {
        self.locals.get(name)
    }
    pub fn bind(&mut self, name: &Name, v: Values<'db>) {
        self.locals.insert_mut(name.clone(), v);
    }
}
```

```rust
// src/interp/walk.rs (changed parts)
pub fn walk_stmts(&mut self, body: &[Stmt], mut env: Env<'db>) -> Env<'db> {
    for stmt in body {
        env = self.walk_stmt(stmt, env);
    }
    env
}

fn walk_stmt(&mut self, stmt: &Stmt, mut env: Env<'db>) -> Env<'db> {
    match stmt {
        Stmt::Expr(s) => {
            self.eval(&s.value, &mut env);
        }
        Stmt::Assign(s) => {
            let v = self.eval(&s.value, &mut env);
            for target in &s.targets {
                self.bind_target(target, v.clone(), &mut env);
            }
        }
        Stmt::AnnAssign(s) => {
            if let Some(value) = &s.value {
                let v = self.eval(value, &mut env);
                self.bind_target(&s.target, v, &mut env);
            }
        }
        Stmt::If(s) => {
            self.eval(&s.test, &mut env);
            env = self.walk_stmts(&s.body, env);
            for clause in &s.elif_else_clauses {
                if let Some(t) = &clause.test {
                    self.eval(t, &mut env);
                }
                env = self.walk_stmts(&clause.body, env);
            }
        }
        // For, While, With, Try, Match: the same change, env threaded through.
        // ...
        other => {
            for e in simple_stmt_exprs(other) {
                self.eval(e, &mut env);
            }
        }
    }
    env
}

fn bind_target(&mut self, target: &Expr, v: Values<'db>, env: &mut Env<'db>) {
    match target {
        Expr::Name(n) => env.bind(&n.id, v),
        Expr::Attribute(a) => {
            self.eval(&a.value, env); // the receiver's calls. Binding: part 03
        }
        other => self.bind_from_ty(other, env), // decision 5
    }
}

pub fn eval(&mut self, expr: &Expr, env: &mut Env<'db>) -> Values<'db> {
    match expr {
        Expr::Name(n) => match env.get(&n.id) {
            Some(v) => v.clone(),
            None => {
                let ty = self.sem.type_of(&self.frame.model, ExprRef::from(expr));
                self.lift(ty)
            }
        },
        Expr::Named(n) => {
            let v = self.eval(&n.value, env);
            self.bind_target(&n.target, v.clone(), env);
            v
        }
        Expr::Call(call) => self.eval_call(call, env),
        Expr::Lambda(_) => smallvec![AbstractValue::Unknown],
        other => {
            for child in direct_children(other) {
                self.eval(child, env);
            }
            let ty = self.sem.type_of(&self.frame.model, ExprRef::from(other));
            self.lift(ty)
        }
    }
}
```

`eval_call` takes `env: &mut Env` and passes it to every `eval` inside it.

</details>

---

## Run

```rust
#[test]
fn p01_03_locals() {
    check("01-functions/python/locals.py");
}
```

Then run steps 01 and 02 again. `cargo test interp::fixture` runs them all.

---

## Where you are

```
Walk  { sem, frame }
Frame { func, file, model, children }
Env   { locals: Name → Values }          ← new. Passed by value through blocks
```

---

## Exercise

1. Write a three-line function where the env and ty would give **different**
   answers, if the env existed and parameters were bound. (You can't run it
   until step 07.)
2. After `f = a` and then `f, g = b, c`, what does `f()` resolve to with your
   code? What would it resolve to if `bind_target` left tuple targets alone?
3. Why can't `eval` take `&Env` and return the new bindings as a second
   result instead of `&mut Env`? What would that cost?

<details><summary>Answers</summary>

1. `def run(cb): cb()` called as `run(on_ok)`. ty: `cb` is `Unknown`, no edge.
   The env, once step 07 binds parameters: `cb = on_ok`, edge `on_ok`.
2. `b`: `f` was rebound to ty's type for it, and ty knows the unpacking. If
   tuple targets were left alone, the env would still say `f = a`, and the tree
   would show `a`, an edge Python never takes.
3. It could, but every `eval` call site would have to merge the returned
   bindings into its own env before evaluating the next sub-expression, in
   the right order. `&mut Env` gets that order for free from Rust's own
   evaluation order, and the persistent map makes the mutation cheap.

</details>

---

→ Next: [`04-imports-and-modules.md`](04-imports-and-modules.md)
