# 01.12 — Callbacks and decorators

**You build:** almost nothing for callbacks. That is the point of this step.
For decorators: a name bound by a decorated `def` resolves to the `def`
itself, and binding stays safe when a decorator may change the signature.
**Fixture:** `python/callbacks.py`
**Plan:** `plan/03-call-tree/11-closures-and-callbacks.md` § "Callbacks",
§ "Decorators", `06-binding-arguments.md` point 5

---

## The fixture

```python
def run(task, cb): cb(task())
def handle_ok():  run(fetch, on_ok)       # run > fetch, on_ok
def handle_err(): run(fetch, on_err)      # run > fetch, on_err
def outer(cb): middle(cb)
def middle(cb): cb()
def factory(): return on_ok
def from_a_factory(): run(fetch, factory())

def retry(fn):
    def wrapper(*args, **kwargs):
        return fn(*args, **kwargs)
    return wrapper

@retry
def flaky(): on_ok()
@retry
def flaky_with(cb): cb()

def uses_decorated():
    flaky()                               # flaky > on_ok
    flaky_with(on_err)                    # flaky_with > on_err

def uses_stdlib():
    p = functools.partial(run, fetch)
    p(on_ok)                              # nothing
    sorted([1], key=on_err)               # nothing
```

## Predict

Run it with your step 11 code first.

1. Which blocks pass without any change? Why did they need nothing new?
2. What does ty say `flaky` is? What tree do you get for `uses_decorated`
   before this step?
3. `functools.partial(run, fetch)` will call `run` at runtime. Why does the
   expected tree show nothing under `uses_stdlib`?

---

## Build

### Decision 1: a call dispatches on the callee's value, never its syntax

Run predict question 1: `main`, `two_levels` and `from_a_factory` already
pass. `cb` is a parameter holding `Function(on_ok)`, and `eval_call` asked
"what value is `cb`, and is it callable", never "is `cb` a name, an attribute
or a parameter".

**Higher-order code is first-order code where the value arrived through a
parameter.** If the call above failed while binding (step 07) and returns
(step 10) work, the bug is in one of those, not in "callback support".

**Guard this in review.** A quick `if let Expr::Name(n) = &*call.func { … }`
special case is how this generality gets lost: it handles `helper()` and
silently drops `cb()`, `make()()`, `self.handler.process()`.

### Decision 2: decorators are transparent

```python
@retry
def flaky(): on_ok()
```

At runtime `flaky` is `retry(flaky)`, which is `wrapper`. ty types it that
way: `flaky` is the function literal `retry.wrapper`. Calling it walks
`wrapper` with `fn = Unknown`, and the tree shows `retry.wrapper` with nothing
under it.

The rule: **a name bound by a `def` statement resolves to that `def`,
whatever the decorators do.**

```rust
// eval, Name arm, after the env missed:
if let Some(defs) = self.sem.decorated_defs(&self.frame.model, name) {
    return defs.into_iter().map(AbstractValue::Function).collect();   // the def itself
}
self.lift(self.sem.type_of(&self.frame.model, expr.into()))           // otherwise ty
```

`sem.decorated_defs` asks ty which definitions the name refers to
(`definitions_for_name`, public), and answers only if they are `def`
statements with decorators. Nested defs need nothing: the `FunctionDef` arm of
step 11 already binds the `def` itself.

**Why:** `@app.route`, `@pytest.fixture`, `@lru_cache`, `@retry` are
everywhere in real code. Treating a decorated function as the decorator's
return value would blank out large parts of a real project's tree. This is a
deliberate imprecision: a decorator that calls something else, or never calls
the function, is not modelled. A fixture pins it.

**[check]** a decorated function reached as a module attribute
(`util.flaky()`). `member` on a module goes through `vnoc::member`, which
returns the decorated type. Apply the same rule there, with
`definitions_for_attribute`, once a fixture shows the gap.

### Decision 3: binding is only trusted for decorators that keep the signature

Transparency means `flaky_with(on_err)` binds `on_err` to `flaky_with`'s own
`cb`. That is right only if the decorator passes the arguments through. Some
change the signature (inject an argument, like `click.pass_context`), and then
the binding would shift every argument by one: a wrong tree.

The rule, from the plan:

- a **project** decorator is trusted (count how often, **[check]** the
  count on a real project);
- a decorator on a small allow-list is trusted: `functools.wraps`,
  `functools.lru_cache`, `functools.cache`;
- anything else: every parameter binds `Unknown`, like a splat (step 08).

```rust
// bind_parameters, first thing:
if args.has_splat || self.decorator_changes_signature(def_node) {
    // every parameter Unknown
}
```

### Decision 4: standard-library higher-order functions are not followed

`functools.partial`, `map`, `sorted(key=…)`, `threading.Thread(target=…)`:
the callee is standard-library code, so it is not walked, and the callback it
would call never appears. The arguments are still evaluated, so any call
*inside* them is found.

**Why keep it:** this is what the Jedi driver does today. Special-casing a
list of stdlib functions would be a behaviour change, to be measured and
decided on its own, not slipped in here.

### Lambdas

Still not entered (step 02, decision 6). **[check]** the Jedi driver on a call
inside a lambda passed as a callback (`run(fetch, lambda: on_ok())`), and match
it.

### Yours

- `sem.decorated_defs(model, name_expr)`.
- The decorated-name rule in `eval`.
- `decorator_changes_signature` and the check in `bind_parameters`.

<details><summary>Reference: this step's code</summary>

```rust
// src/sem.rs (added)
use ruff_python_ast::AnyNodeRef;
use ty_python_semantic::{ImportAliasResolution, ResolvedDefinition, definitions_for_name};

/// If `name` refers to `def` statements with decorators, those defs.
pub fn decorated_defs(&self, model: &SemanticModel<'db>, name: &ast::ExprName) -> Option<Vec<Definition<'db>>> {
    let found = definitions_for_name(
        model,
        name.id.as_str(),
        AnyNodeRef::ExprName(name),
        ImportAliasResolution::ResolveAliases,
    );
    let mut defs = Vec::new();
    for r in found {
        let ResolvedDefinition::Definition(def) = r else { return None };   // [check] variant name
        let parsed = self.parsed(def);
        let node = self.function_node(def, &parsed)?;      // not a def: not this rule
        if node.decorator_list.is_empty() {
            return None;                                    // undecorated: ty's answer is the def anyway
        }
        defs.push(def);
    }
    (!defs.is_empty()).then_some(defs)
}

const SIGNATURE_KEEPING: &[&str] = &["functools.wraps", "functools.lru_cache", "functools.cache"];
```

```rust
// src/interp/bind.rs (added)
fn decorator_changes_signature(&mut self, def: &StmtFunctionDef) -> bool {
    for dec in &def.decorator_list {
        // `@retry` and `@functools.lru_cache(maxsize=1)` alike: look at the callee.
        let target = match &dec.expression {
            Expr::Call(c) => &*c.func,
            e => e,
        };
        let ty = self.sem.type_of(&self.frame.model, ExprRef::from(target));
        let trusted = match self.sem.shape(ty, self.frame.file) {
            Shape::Function(d) => self.sem.is_project(d) || SIGNATURE_KEEPING.contains(&self.sem.qname(d).as_str()),
            _ => false,
        };
        if !trusted {
            return true;
        }
    }
    false
}
```

The decorator expression is typed with the callee's model: `bind_parameters`
already runs with the callee's frame active (step 07).

</details>

---

## Run

```rust
#[test]
fn p01_12_callbacks() {
    check("01-functions/python/callbacks.py");
}
```

---

## Where you are

The walker handles every way a plain function value moves: by name, through
an import, as an argument, as a return, captured by a closure, and wrapped in
a decorator. Part 03 adds objects. First, one more step, to see what all of
this costs.

---

## Exercise

1. `handlers = {"ok": on_ok}` and `handlers["ok"]()`. What does the tree show?
   Which design rule says so?
2. A project decorator `@inject_db` calls the function as `fn(db, *args)`. With
   the rule in decision 3, what does `def save(db, item): item.store()` bind
   `item` to when called as `save(x)`? What should it bind?
3. Why is "dispatch on the value" a review rule and not only a coding rule?

<details><summary>Answers</summary>

1. No edge from that call: containers are not modelled, so `handlers["ok"]` is
   ty's type for the subscript, which is not a project function. The plan
   lists functions in containers as out of scope until measured.
2. The rule trusts project decorators, so `db = x`, `item = Unknown`: shifted
   by one, and wrong if `x` were an object with a `store`. Python binds
   `db` to the injected connection and `item = x`. That is why the plan marks
   the trust in project decorators **[check]**: count how often a project
   decorator changes arity before relying on it.
3. Because the shortcut that breaks it passes every simple fixture: `helper()`
   is a `Name`. The failure only shows up on callbacks, factories and method
   chains, which a quick review of a small diff won't exercise.

</details>

---

→ Next: [`13-the-wall.md`](13-the-wall.md)
