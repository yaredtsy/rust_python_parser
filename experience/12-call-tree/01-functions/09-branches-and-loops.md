# 01.09 — Branches and loops

**You build:** fork and join. Each arm of an `if`, `match` or `try` starts
from the env before it, and the arms' envs are joined after it. A loop's body
is walked once and joined with "didn't run".
**Fixture:** `python/branches.py`
**Plan:** `plan/03-call-tree/05-the-walk.md` § "Statements",
`01-what-jedi-actually-does.md` § "The line to hold"

---

## The fixture

```python
def never_none(cond):
    f = None
    if cond: f = a
    else:    f()          # no edge: f is None on this path

def both_arms(cond):
    if cond: f = a
    else:    f = b
    f()                   # a, b

def skipped(cond):
    f = a
    if cond: f = b
    f()                   # a, b

def loop(xs):
    f = a
    for x in xs:
        f()               # a
        f = b
    f()                   # a, b

def try_except():
    f = a
    try:
        f = b
        risky()
    except ValueError:
        f()               # a, b
```

(`elif_chain` and `while_loop` are in the file too.)

## Predict

Run the fixture with your step 08 code first.

1. Which blocks fail, and is each failure a missing edge or a wrong one?
2. In `skipped`, why is `a` still possible at `f()`, even though the only
   assignment inside the `if` is `f = b`?
3. In `loop`, Python calls `b` inside the loop on the second iteration. Why
   does the expected tree not show it?

---

## Build

### Decision 1: conditions are never evaluated

`if cond:`, `while n:`, `match x:` and `x == 1` are evaluated for their calls
and then ignored. Every arm is assumed possible.

**Why:** deciding which arm runs means modelling comparisons, which means
modelling integers, strings, `None` checks, `isinstance`. That is a symbolic
executor, and it never ends. The Jedi driver doesn't do it either. The plan's
rule: **union at branches, precision at bindings.** Values flow exactly
through assignments and calls; at a branch, all possibilities are kept.

### Decision 2: fork and join, not a linear pass

Your step 03 code walks an `if`'s arms one after another, passing the env
along. That is wrong in both directions:

- `never_none`: the `else` arm starts with `f = a`, left over from the `if`
  arm. The tree shows `a`, an edge Python can never take on that path.
- `both_arms`: after the `if`, the env says `f = b`, the last arm walked. The
  tree loses `a`.

Instead, every arm starts from the env **before** the branch, and the arms'
results are joined:

```rust
Stmt::If(s) => {
    let mut cond = env;
    self.eval(&s.test, &mut cond);                  // evaluated once, value ignored
    let mut arms = vec![self.walk_stmts(&s.body, cond.clone())];
    for clause in &s.elif_else_clauses {
        if let Some(t) = &clause.test {
            self.eval(t, &mut cond);                // runs only if earlier tests failed
        }
        arms.push(self.walk_stmts(&clause.body, cond.clone()));
    }
    if !s.elif_else_clauses.iter().any(|c| c.test.is_none()) {
        arms.push(cond);                            // no `else`: skipping is an arm
    }
    Env::join(arms)
}
```

`Env::join` unions per name:

- a name bound in every arm gets the union of the arms' values, deduplicated,
  in arm order;
- a name bound in only some arms (first assigned inside one) gets the union
  of those arms. On the other paths Python would raise `UnboundLocalError`,
  so they contribute nothing.

This is why step 03 never *removes* a name from the env: a missing name means
"not assigned on this path", and the join relies on it.

**Order.** Inside `Values`, alternatives keep the order the arms were joined
in. That order is an accident of the walk, and it will differ between modes
later (a summary joins in other places than an inline walk). So the tree must
not depend on it: **within one call site, one node per function, ordered by
where the function is defined** (its file, then its offset). `skipped` shows
`a` before `b` because `a` is defined first, not because of the join.

```rust
// after collecting the nodes of one call site
site_nodes.sort_by_key(|n| s.sem.def_position(n.func));   // (file path, offset)
```

`sem.def_position(def)` is the definition's file path and range start. Nodes
of different call sites keep evaluation order: that one is not an accident.

### Decision 3: a loop body is walked once, joined with "didn't run"

```rust
Stmt::For(s) => {
    self.eval(&s.iter, &mut env);
    let before = env;
    let mut body_env = before.clone();
    self.bind_from_ty(&s.target, &mut body_env);    // the loop variable: ty's element type
    let body = self.walk_stmts(&s.body, body_env);
    let after = Env::join(vec![before, body]);     // ran zero times, or ran
    self.walk_stmts(&s.orelse, after)
}
```

`while` is the same, with its test evaluated before the body.

**Why once:** walking until nothing changes (a fixed point over the loop) is
possible, but every loop in every function would pay for it, and part 02 adds
a fixed point of its own for recursion. The plan keeps this simple and pins
the cost with a fixture: inside `loop`'s body, `f()` shows only `a`, while
Python calls `b` from the second iteration on. That is a known limit, listed
for later (`plan/03-call-tree/15` § "Growth path", v1.2).

`break` and `continue` are not modelled: the walker keeps walking the rest of
the block. That can only add calls that the Jedi driver, which finds calls by
syntax, also shows.

### Decision 4: a `try` handler can start anywhere in the body

An exception can be raised by any statement of the `try` body, so a handler
can start with the env from before the body, after its first statement, after
its second, and so on. Walking each start is too costly. The approximation is
the join of "before the body" and "end of the body":

```rust
Stmt::Try(s) => {
    let before = env;
    let body = self.walk_stmts(&s.body, before.clone());
    let start = Env::join(vec![before, body.clone()]);
    let mut arms = vec![self.walk_stmts(&s.orelse, body)];
    for h in &s.handlers {
        let h = h.as_except_handler().unwrap();
        arms.push(self.walk_stmts(&h.body, start.clone()));
    }
    let joined = Env::join(arms);
    self.walk_stmts(&s.finalbody, joined)
}
```

`try_except` shows `a, b` in the handler: `f` is `a` if `f = b` hadn't run
yet, `b` if it had.

### The rest

- `with`: each context expression is evaluated, `as` targets get ty's type
  (`bind_from_ty`), and the body always runs.
- `match`: the subject is evaluated, then each case body is an arm from the
  env before it, plus "no case matched". Names bound by patterns get
  `Unknown`: patterns are not expressions, so there is no simple way to ask ty
  for them. **[check]** if that costs edges on real code.
- `return` and `raise` inside an arm still fall through for now. Step 10 fixes
  that.

### Yours

- `Env::join(Vec<Env>) -> Env`.
- The `If`, `For`, `While`, `Try`, `With` and `Match` arms.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/env.rs (added)
impl<'db> Env<'db> {
    /// Union per name. A name missing from an arm was not assigned on that path.
    pub fn join(arms: Vec<Env<'db>>) -> Env<'db> {
        let mut arms = arms.into_iter();
        let mut out = arms.next().unwrap_or_default();
        for arm in arms {
            for (name, vals) in arm.locals.iter() {
                let merged = match out.locals.get(name) {
                    Some(old) if old == vals => continue,
                    Some(old) => {
                        let mut m = old.clone();
                        m.extend(vals.iter().copied());
                        dedup(&mut m);
                        m
                    }
                    None => vals.clone(),
                };
                out.locals.insert_mut(name.clone(), merged);
            }
        }
        out
    }
}
```

```rust
// src/interp/walk.rs: the remaining arms
Stmt::While(s) => {
    self.eval(&s.test, &mut env);
    let before = env;
    let body = self.walk_stmts(&s.body, before.clone());
    let after = Env::join(vec![before, body]);
    self.walk_stmts(&s.orelse, after)
}
Stmt::With(s) => {
    for item in &s.items {
        self.eval(&item.context_expr, &mut env);
        if let Some(target) = &item.optional_vars {
            self.bind_from_ty(target, &mut env);
        }
    }
    self.walk_stmts(&s.body, env)
}
Stmt::Match(s) => {
    self.eval(&s.subject, &mut env);
    let before = env;
    let mut arms = Vec::new();
    for case in &s.cases {
        let mut arm = before.clone();
        for name in pattern_names(&case.pattern) {
            arm.bind(name, smallvec![AbstractValue::Unknown]);
        }
        if let Some(guard) = &case.guard {
            self.eval(guard, &mut arm);
        }
        arms.push(self.walk_stmts(&case.body, arm));
    }
    arms.push(before);   // no case matched
    Env::join(arms)
}
```

`walk_stmt` now returns the env each arm produces, instead of updating one
`env` in place. `pattern_names` collects the `name` of every `MatchAs`,
`MatchStar` and the `rest` of every `MatchMapping` in a pattern.

</details>

---

## Run

```rust
#[test]
fn p01_09_branches() {
    check("01-functions/python/branches.py");
}
```

Then every earlier test. `one_call.py` has an `if True:`; its call must still
be found.

---

## Where you are

```
walk_stmts(body, env) -> Env      each arm gets its own copy; Env::join merges them
```

The walker follows Python's control flow without evaluating a single
condition. Every arm is possible, and no arm sees another arm's assignments.

---

## Exercise

1. `if c: f = a` followed by `f()`, where `f` was never assigned before the
   `if`. What does `f` hold at the call, and what does Python do on the other
   path?
2. Write a two-statement loop body where walking it once misses an edge *after*
   the loop, not just inside it.
3. Why does `If` evaluate later `elif` tests in the same `cond` env, rather
   than each in a fresh copy of the env before the `if`?

<details><summary>Answers</summary>

1. `[a]`, from the one arm that bound it. On the path where `c` is false,
   Python raises `UnboundLocalError` at `f()`, so that path has no value to
   contribute.
2. With `g = a` before the loop, the body `f = g` then `g = b`. After one
   iteration `f` is `a`; after two, `f` is `b`. The walker joins "didn't run"
   with "ran once", so after the loop `f` holds its old value or `a`, never
   `b`, and a call `f()` after the loop misses `b`. A value that takes two
   iterations to form is the same known limit as the one inside the body.
3. Python evaluates an `elif` test only after every earlier test was false, so
   it runs after them. A walrus or a call in an earlier test is visible to the
   later ones. Evaluating each in a fresh copy would lose that order.

</details>

---

→ Next: [`10-returns.md`](10-returns.md)
