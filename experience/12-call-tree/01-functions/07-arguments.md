# 01.07 — Arguments

**You build:** argument evaluation at the call site, and binding: each of the
callee's parameters gets the values this caller passed, a default, or
`Unknown`. The callee's env starts with them.
**Fixture:** `python/arguments.py`
**Plan:** `plan/03-call-tree/06-binding-arguments.md`, `01-what-jedi-actually-does.md`
§ "What `as_context(arguments)` gives you"

---

## The fixture

```python
def run(cb): cb()
def run_kw(first, second): second()
def run_default(cb=fallback): cb()
def outer(cb): middle(cb)
def middle(cb): inner(cb)
def inner(cb): cb()
def missing(cb): cb()
def with_module(m): m.helper()
def shadow(cb): leak()
def leak(): cb()

def main():
    run(on_ok)                          # run > on_ok
    run(on_err)                         # run > on_err
    run_kw(second=on_ok, first=on_err)  # run_kw > on_ok
    run_default()                       # run_default > fallback
    run_default(on_ok)                  # run_default > on_ok
    outer(on_err)                       # outer > middle > inner > on_err
    missing()                           # missing, no child
    with_module(util)                   # with_module > util.helper
    shadow(on_ok)                       # shadow > leak, no child

def both():
    read_ok()                           # read_ok > run > on_ok
    read_err()                          # read_err > run > on_err

def entry(cb): cb()                     # entry, no child
```

`both` is the fixture that matters most in this whole tutorial: the same
function, `run`, called from two different functions, with two different
results. That is what "per path" means.

## Predict

1. In `outer(on_err)`, `inner` is two calls away from `main`. How does `inner`'s
   `cb` end up being `on_err`? Count how many times the expression `on_err` is
   evaluated.
2. `run_default()`: where does `fallback` come from: `main`'s env, `run_default`'s
   env, or somewhere else?
3. `missing()` passes nothing. Python raises `TypeError`. Should the tree show
   `missing` at all?

---

## Build

### Decision 1: arguments are evaluated once, eagerly, and bound as values

```rust
pub struct EvaluatedArgs<'db> {
    pub positional: SmallVec<[Values<'db>; 4]>,
    pub keywords:   SmallVec<[(Name, Values<'db>); 2]>,
}
```

At the call site, each argument expression is evaluated once, in the caller's
env, in source order. The callee gets the **values**.

**Why:** the Jedi driver is lazy. It keeps each argument as an expression plus
the caller's context, and re-evaluates it whenever the callee looks the
parameter up. A parameter passed through three functions is re-evaluated
through three contexts, every time it is read, and the driver needs recursion
guards to keep that finite.

Eager evaluation flattens the chain. `outer` binds `cb = on_err`. When `outer`
calls `middle(cb)`, `cb` is evaluated in `outer`'s env, which already holds the
value. `inner` gets the same value. The expression `on_err` was evaluated once,
in `main`.

**What it costs:** an argument the callee never reads is evaluated anyway. Its
calls had to be found anyway, so the cost is small.

### Decision 2: bind by the callee's own parameter list

For each parameter of the callee's `def`, in order:

```rust
// positional-only and positional-or-keyword
let v = args.keyword(&param.name)                 // a keyword with its name
    .or_else(|| positional.next())                // else the next positional
    .or_else(|| self.default_value(param))        // else its default
    .unwrap_or_else(|| smallvec![AbstractValue::Unknown]);   // else nothing known

// keyword-only: a keyword with its name, else its default, else Unknown
```

The result binds each parameter's name in a fresh env. That is the callee's
starting env (step 05, decision 3), and it holds nothing else.

`*args` and `**kwargs` parameters are step 08. The receiver of a method is
part 03.

### Decision 3: a default belongs to where the `def` ran

```python
def run_default(cb=fallback): cb()
```

`fallback` is evaluated when the `def` statement runs, at import time, in the
module's scope. Not when `run_default` is called, and not in the caller's env.
So for a module-level function the default's value is ty's type of the default
expression, lifted, using the **callee's** file:

```rust
fn default_value(&mut self, param: &ParameterWithDefault) -> Option<Values<'db>> {
    let expr = param.default.as_deref()?;
    let ty = self.sem.type_of(&self.frame.model, expr.into());   // frame = the callee's, already
    Some(self.lift(ty))
}
```

Binding never evaluates the default *as an expression in this walk*. If it did,
a call inside a default (`def f(h=make())`) would be recorded on every call to
`f`, under the wrong parent.

Getting this backwards fails quietly. If `main` had its own local `fallback`,
reading the default from the caller's env would pick it up, and the tree would
show a call Python never makes. Nested defs, whose defaults can read the
enclosing function's locals, are step 11.

### Decision 4: a parameter with no value is `Unknown`, and the call still shows

`missing()` binds `cb = Unknown`. `cb()` gives no edge, and the `missing` node
is still in the tree. Python would raise, but the Jedi driver finds calls by
syntax and shows it, and a node with no children is the honest answer.

### Decision 5: an entry's parameters are `Unknown`

`entry(cb)` is walked with no caller, so `cb` is `Unknown`. Part 03 adds the
annotation fallback: `def entry(h: Handler)` starts `h` from its annotation.

### Yours

- `interp/bind.rs`: `EvaluatedArgs`, `eval_arguments`, `bind_parameters`
  (returning the callee's starting env), `default_value`.
- `eval_call` evaluates the arguments into `EvaluatedArgs` before calling, and
  passes them to `call_function`.
- `call_function` switches to the callee's frame, *then* binds (so defaults
  are lifted with the callee's model), then walks.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/bind.rs
use ruff_python_ast::{ArgOrKeyword, Arguments, ExprRef, ParameterWithDefault, StmtFunctionDef};
use smallvec::{SmallVec, smallvec};

use crate::interp::env::Env;
use crate::interp::value::{AbstractValue, Values};
use crate::interp::walk::Walk;
use crate::sem::Name;

#[derive(Clone, Default)]
pub struct EvaluatedArgs<'db> {
    pub positional: SmallVec<[Values<'db>; 4]>,
    pub keywords: SmallVec<[(Name, Values<'db>); 2]>,
}

impl<'db> EvaluatedArgs<'db> {
    fn keyword(&self, name: &str) -> Option<&Values<'db>> {
        self.keywords.iter().find(|(n, _)| n.as_str() == name).map(|(_, v)| v)
    }
}

impl<'a, 'db> Walk<'a, 'db> {
    /// In the caller's frame and env, left to right, each argument once.
    pub fn eval_arguments(&mut self, args: &Arguments, env: &mut Env<'db>) -> EvaluatedArgs<'db> {
        let mut out = EvaluatedArgs::default();
        for arg in args.iter_source_order() {
            match arg {
                ArgOrKeyword::Arg(e) => {
                    let v = self.eval(e, env);
                    out.positional.push(v);
                }
                ArgOrKeyword::Keyword(kw) => {
                    let v = self.eval(&kw.value, env);
                    if let Some(name) = &kw.arg {
                        out.keywords.push((name.id.clone(), v));
                    }
                }
            }
        }
        out
    }

    /// The callee's starting env. Must run with the callee's frame active.
    pub fn bind_parameters(&mut self, def: &StmtFunctionDef, args: &EvaluatedArgs<'db>) -> Env<'db> {
        let mut env = Env::default();
        let mut positional = args.positional.iter();
        let params = &def.parameters;

        for p in params.posonlyargs.iter().chain(&params.args) {
            let name = &p.parameter.name.id;
            let v = args
                .keyword(name)
                .cloned()
                .or_else(|| positional.next().cloned())
                .or_else(|| self.default_value(p))
                .unwrap_or_else(|| smallvec![AbstractValue::Unknown]);
            env.bind(name, v);
        }
        for p in &params.kwonlyargs {
            let name = &p.parameter.name.id;
            let v = args
                .keyword(name)
                .cloned()
                .or_else(|| self.default_value(p))
                .unwrap_or_else(|| smallvec![AbstractValue::Unknown]);
            env.bind(name, v);
        }
        env
    }

    fn default_value(&mut self, p: &ParameterWithDefault) -> Option<Values<'db>> {
        let expr = p.default.as_deref()?;
        let ty = self.sem.type_of(&self.frame.model, ExprRef::from(expr));
        Some(self.lift(ty))
    }
}
```

```rust
// src/interp/call.rs (changed parts)
pub fn eval_call(&mut self, call: &ExprCall, env: &mut Env<'db>) -> Values<'db> {
    let callees = self.eval(&call.func, env);
    let args = self.eval_arguments(&call.arguments, env);
    for callee in &callees {
        if let AbstractValue::Function(def) = *callee
            && self.sem.is_project(def)
        {
            self.call_function(def, call.range(), &args);
        }
    }
    let ty = self.sem.type_of(&self.frame.model, ExprRef::from(call));
    self.lift(ty)
}

fn call_function(&mut self, def: Definition<'db>, site: TextRange, args: &EvaluatedArgs<'db>) {
    // … the guard from step 06 …
    self.path.push(qname.clone());
    let caller = std::mem::replace(&mut self.frame, Frame::new(self.sem, def));
    let env = self.bind_parameters(node, args);   // the callee's frame is active: defaults use its model
    self.walk_stmts(&node.body, env);
    let callee = std::mem::replace(&mut self.frame, caller);
    self.path.pop();
    // … push the node …
}
```

For the entry, `call_tree` binds with `EvaluatedArgs::default()`, so every
parameter is its default or `Unknown`.

</details>

---

## Run

```rust
#[test]
fn p01_07_arguments() {
    check("01-functions/python/arguments.py");
}
```

If `run_kw` shows `on_err`, keywords are being matched by position. If
`run_default()` shows nothing, the default is being read from the wrong place:
print which model `default_value` uses.

If `with_module` shows no `util.helper` child, `eval` is not sending
`Expr::Attribute` to `eval_attribute` (step 04). `m.helper` then falls to
ty's type for the whole expression, and ty sees `m` as an unknown parameter.
This is the first fixture where only your own `member` knows the answer.

---

## Where you are

```
EvaluatedArgs { positional, keywords }    built in the caller
bind_parameters → Env                     the callee's starting env: parameters only
```

Values now cross calls. `both` shows the per-path tree: `run` has a different
child under each parent.

---

## Exercise

1. Write the tree for `main` if `call_function` bound the arguments *before*
   switching to the callee's frame. Which line changes, and why only in some
   projects?
2. `def f(a, b=g()): ...` at module level. Which function's tree shows `g`?
3. In `outer(on_err)`, how many `Values` objects are created for the three
   `cb` parameters, and how many times does the walker look `on_err` up in ty?

<details><summary>Answers</summary>

1. Nothing changes in this fixture, because `fallback` is defined in the same
   file as `main`. It changes when the callee is in another file: the default
   expression would be typed with the caller's model, which ty may answer
   wrongly or panic on (step 05, decision 4).
2. None of the trees in this tutorial: the default runs when the `def` runs,
   at module level, and module-level code is not an entry. Per the Jedi driver,
   it belongs to the module, not to `f` and not to any caller of `f`.
3. Three bindings, each a clone of the same small `Values` (a clone of one
   `Function` value). ty is asked once, in `main`, for the name `on_err`. Every
   later `cb` is an env hit.

</details>

---

→ Next: [`08-star-args-and-splats.md`](08-star-args-and-splats.md)
