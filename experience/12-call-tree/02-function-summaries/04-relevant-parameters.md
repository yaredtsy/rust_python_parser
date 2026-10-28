# 02.04 — Relevant parameters

**You build:** a pass over a function's code that decides, once, which of its
parameters can affect its walk. The others go into every key as `Unknown`, so
callers that differ only there share one summary.
**Fixture:** `python/relevant.py`
**Plan:** `plan/03-call-tree/08-keys.md` § "Fingerprinting", "Irrelevant
parameters"

---

## The fixture

```python
def note(msg): pass
def log_event(name, payload):
    note(name)              # payload is never read
def forward(cb):
    x = cb
    x()                     # relevant, through x
def keep(cb):
    return cb               # relevant: returned

def main():
    log_event("start", on_ok)
    log_event("stop", on_err)    # same context as the line above
    forward(on_ok)
    forward(on_err)
    keep(on_ok)()
    keep(on_err)()
```

Summary mode: **9** contexts with this step, 10 without.

## Predict

1. Without this step, how many contexts does `log_event` get, and why?
2. `forward` doesn't call `cb` directly. Why is `cb` still relevant?
3. Which costs more: marking a parameter relevant when it isn't, or the
   other way round?

---

## Build

### Decision 1: a parameter the walk can't be affected by is `Unknown` in the key

```rust
pub fn key(&mut self, func: Definition<'db>, bound: &Bound<'db>) -> ContextId {
    let relevant = relevant_params(self.sem.db, func);
    let args = bound.slots.iter().enumerate()
        .map(|(i, v)| if relevant[i] { self.fingerprint(v) } else { self.fps.intern(Fp::Unknown) })
        .collect();
    self.contexts.intern(Context { func, args })
}
```

`log_event` never reads `payload`. Whatever a caller passes there, the walk
does the same thing, so the key shouldn't hold it. Both calls in `main` now
key to `log_event(Data(str), Unknown)`: one walk.

**Why it matters on real code:** loggers, event hooks, metrics wrappers, and
any function taking a `context` or `request` object it only passes along to
stdlib code. Each distinct value of such a parameter would otherwise be its
own context, with its own walk, finding the same calls.

### Decision 2: decided from the code alone, once per function

```rust
#[salsa::tracked(returns(ref))]
pub fn relevant_params<'db>(db: &'db dyn Db, func: Definition<'db>) -> Vec<bool> { … }
```

It reads only the function's own body, so it obeys rule 3: precision depends
only on the key and the code. That also makes it a pure function of the code,
which salsa can cache across requests and recompute only when the file
changes. `src/defs.rs::definitions` already shows a tracked function in pylspt
over ty's `Db`; `Definition<'db>` is a salsa tracked struct, which a tracked
function accepts as an argument.

### Decision 3: when unsure, relevant

The two mistakes are not equal:

- **Over-marking** (relevant when it isn't): two callers that could have
  shared a walk get two. A little slower. The tree is the same.
- **Under-marking** (irrelevant when it isn't): the walk starts with `Unknown`
  where the caller passed something it uses. Edges disappear, and no-reuse
  mode won't notice, because both modes lose them the same way.

So the pass is conservative. A parameter is relevant if it, or a local that
may hold its value, reaches any of these:

| sink | example |
|---|---|
| a call's callee | `cb()` |
| a call's argument (any call) | `note(name)` |
| an attribute's receiver | `cb.x`, `cb.run()` |
| a `return`, `yield` or `await` | `return cb` |
| an attribute or subscript write, as target or value | `self.h = cb`, `d[k] = cb` |
| a nested `def`, `lambda` or comprehension that reads it | `def inner(): cb()` |
| a `with` item | `with cb: …` |

A parameter used only in a condition (`if flag:`) is not relevant: conditions
are never evaluated, so its value can't change the walk.

**Flow through locals:** `x = cb` makes `x` hold `cb`'s value, so a sink on `x`
marks `cb`. Assignments, walrus, `for x in cb`, `with … as x` and tuple targets
all propagate. Branches and loops make "later" flow back, so iterate the pass
over the body until nothing new is marked.

### Yours

- `relevant_params`, as a tracked function in `src/interp/key.rs` or its own
  file.
- `key` uses it.
- The fixture's stats test.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/relevant.rs
use ruff_python_ast::visitor::{Visitor, walk_expr, walk_stmt};
use ruff_python_ast::{self as ast, Expr, Stmt};
use rustc_hash::{FxHashMap, FxHashSet};

/// Which parameters of `func` can affect its walk. One flag per parameter,
/// in the order `bind_parameters` fills `Bound.slots`.
#[salsa::tracked(returns(ref))]
pub fn relevant_params<'db>(db: &'db dyn Db, func: Definition<'db>) -> Vec<bool> {
    let parsed = parsed_module(db, func.python_file(db)).load(db);
    let DefinitionKind::Function(f) = func.kind(db) else { return Vec::new() };
    let def = f.node(&parsed);
    let params: Vec<&ast::Name> = parameter_names(def).collect();

    // holds[name] = the parameters a local may hold. Seeded by the parameters themselves.
    let mut holds: FxHashMap<ast::Name, FxHashSet<usize>> = params.iter().enumerate()
        .map(|(i, n)| ((*n).clone(), FxHashSet::from_iter([i])))
        .collect();
    let mut relevant = vec![false; params.len()];

    loop {
        let mut pass = Taint { holds: &mut holds, relevant: &mut relevant, changed: false };
        pass.visit_body(&def.body);
        if !pass.changed { break }
    }
    relevant
}

struct Taint<'a> {
    holds: &'a mut FxHashMap<ast::Name, FxHashSet<usize>>,
    relevant: &'a mut Vec<bool>,
    changed: bool,
}

impl Taint<'_> {
    /// Every parameter any name in `e` may hold.
    fn params_in(&self, e: &Expr) -> FxHashSet<usize> { /* collect Name loads, union their `holds` */ }

    fn sink(&mut self, e: &Expr) {
        for i in self.params_in(e) {
            if !self.relevant[i] { self.relevant[i] = true; self.changed = true; }
        }
    }

    fn flow(&mut self, target: &Expr, from: &Expr) {
        let ps = self.params_in(from);
        for name in target_names(target) {
            let set = self.holds.entry(name).or_default();
            let before = set.len();
            set.extend(&ps);
            if set.len() != before { self.changed = true; }
        }
    }
}

impl<'a> Visitor<'a> for Taint<'_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::Assign(a) => for t in &a.targets { self.flow(t, &a.value) },
            Stmt::AnnAssign(a) => if let Some(v) = &a.value { self.flow(&a.target, v) },
            Stmt::For(f) => self.flow(&f.target, &f.iter),
            Stmt::Return(r) => if let Some(v) = &r.value { self.sink(v) },
            Stmt::With(w) => for item in &w.items {
                self.sink(&item.context_expr);
                if let Some(t) = &item.optional_vars { self.flow(t, &item.context_expr) }
            },
            Stmt::FunctionDef(d) => self.sink_free_names(d),   // a nested def captures
            Stmt::If(_) | Stmt::While(_) => {}                 // tests are not sinks; bodies below
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, e: &'a Expr) {
        match e {
            Expr::Call(c) => {
                self.sink(&c.func);
                for arg in c.arguments.iter_source_order() { self.sink(arg.value()) }
            }
            Expr::Attribute(a) => self.sink(&a.value),
            Expr::Yield(_) | Expr::YieldFrom(_) | Expr::Await(_) => self.sink(e),
            Expr::Named(n) => self.flow(&n.target, &n.value),
            Expr::Lambda(_) | Expr::ListComp(_) | Expr::SetComp(_) | Expr::DictComp(_) | Expr::Generator(_) => self.sink(e),
            _ => {}
        }
        walk_expr(self, e);
    }
}
```

Attribute and subscript *targets* (`self.h = cb`) are sinks for both sides:
add them in the `Assign` arm. `sink_free_names(d)` marks every parameter whose
name (or a local holding it) is read inside the nested def, including its
decorators and defaults.

This is a sketch: the visitor visits sub-expressions after the match, which is
what makes `f(g(cb))` mark `cb` through the inner call.

</details>

---

## Run

```rust
#[test]
fn p02_04_relevant() {
    check_both("02-function-summaries/python/relevant.py");
    let st = stats_for("02-function-summaries/python/relevant.py", "main", &summary());
    assert_eq!(st.contexts, 9);
}
```

Then every test. Relevance can only make keys share more; if any tree changed,
the pass under-marked something. Find which parameter, and which sink the pass
missed.

---

## Where you are

```
relevant_params(func) → [bool]          salsa-cached, from the code alone
key(func, bound): an irrelevant slot is Fp::Unknown
```

---

## Exercise

1. `def f(a, b): if a: b()`. Which parameters are relevant?
2. `def g(h): return [h]`. Is `h` relevant by the table above? Should it be?
3. Under-marking gives wrong trees that no-reuse mode can't catch. Why can't
   it?

<details><summary>Answers</summary>

1. `b` only. `a` is read only in a condition, which is never evaluated. `b`
   is a callee.
2. Not by the table: a list display is not listed. It should be, if the list
   is returned: `[h]` is part of the `return` expression, so the `return` sink
   covers it, because `params_in` collects every name inside the returned
   expression. Containers are not modelled, so the caller gets `Ty(list)`
   either way, but marking it is the conservative choice.
3. No-reuse mode compares reuse with recomputation, and both build the walk
   from the same key. If the key wrongly says `Unknown`, both walks start from
   `Unknown` and lose the same edges. Only an expected tree, written by hand
   from what Python does, catches it.

</details>

---

→ Next: [`05-recursion-pending-values.md`](05-recursion-pending-values.md)
