# 01.10 — Returns

**You build:** the value of a call to a project function: what the callee
returned on this path. Along the way, a block learns that it may never fall
through (`return`, `raise`), and an empty `Values` starts to mean "no value
reaches here".
**Fixture:** `python/returns.py`
**Plan:** `plan/03-call-tree/05-the-walk.md` § "Statements",
`10-one-call.md` § "A call with no value"

---

## The fixture

```python
def pick_a(): return a
def pick(c):
    if c: return a
    return b
def ident(f): return f
def through(cb): return ident(cb)
def nothing(): pass
def fail(): raise ValueError()
def log(x): pass
def gen(): yield a
async def fetch(): return a

def early(c):
    f = b
    if c:
        f = a
        return
    f()                 # b only

def main():
    pick_a()()          # pick_a, a
    f = pick(1)
    f()                 # pick, a, b
    ident(b)()          # ident, b
    through(a)()        # through > ident, a
    nothing()()         # nothing
    log(fail())         # fail, log
    g = gen()
    g()                 # gen

async def amain():
    (await fetch())()   # fetch, a
```

## Predict

Run it with your step 09 code first.

1. Which blocks fail, and what is the value of `pick_a()` with your step 09
   code? (Hint: what does ty say a call to an unannotated function returns?)
2. `fail()` never returns. What should `log`'s parameter `x` hold?
3. In `early`, why must `f()` not see `a`?

---

## Build

### Decision 1: a call's value is what the callee returned on this path

```rust
// Frame gains
pub returns: Values<'db>,

// eval_call
let mut out = Values::new();
let mut ask_ty = false;
for callee in &callees {
    match *callee {
        AbstractValue::Function(def) if self.sem.is_project(def) =>
            out.extend(self.call_function(def, site, &args)),   // ← the callee's returns
        _ => ask_ty = true,
    }
}
if ask_ty {
    out.extend(self.lift(self.sem.type_of(&self.frame.model, call.into())));
}
dedup(&mut out);
out
```

**Why not keep ty's answer:** ty types a call by the callee's *annotated*
return type, and never infers one from the body (ty's own tests:
`mdtest/function/return_type.md`). None of these functions is annotated, so
ty says `Unknown` for every call in the fixture, and only `nothing()` and
`fail()` match by accident. And even an annotation is one type for all
callers: for `ident`, the return depends on the argument. Your walk has
`f = b` on this path, so `return f` returns `b`.

This is also why steps 01–09 never relied on the value of a call: with ty's
answer, it was `Unknown` for every unannotated project function.

This is the first place where the env-first rule of step 03 pays: a value
from a caller (`b`) goes in as an argument, comes back as a return, and is
called. ty sees none of that.

### Decision 2: a block may never fall through

```rust
pub fn walk_stmts(&mut self, body: &[Stmt], mut env: Env<'db>) -> Option<Env<'db>> {
    for stmt in body {
        env = self.walk_stmt(stmt, env)?;        // None: this path ended here
    }
    Some(env)
}

Stmt::Return(s) => {
    let v = match &s.value {
        Some(e) => self.eval(e, &mut env),
        None => self.none_value(),
    };
    self.frame.returns.extend(v);
    return None;
}
Stmt::Raise(s) => {
    // evaluate `exc` and `cause` for their calls
    return None;
}
```

`Env::join` now takes `Vec<Option<Env>>`, skips the `None` arms, and returns
`None` when every arm is `None`: a branch where every arm returns doesn't fall
through either.

`walk_stmt` keeps its shape from step 09, with three mechanical changes:

```rust
fn walk_stmt(&mut self, stmt: &Stmt, mut env: Env<'db>) -> Option<Env<'db>> {   // 1. Option
    match stmt {
        Stmt::If(s) => {
            // … arms are now Option<Env> …
            env = Env::join(arms)?;                       // 2. `?`: every arm left → so does the `if`
        }
        Stmt::For(s) => {
            // …
            let after = Env::join(vec![Some(before), body])?;   // `before` is always an arm
            env = self.walk_stmts(&s.orelse, after)?;
        }
        Stmt::Return(s) => { /* … */ return None; }
        // … every `env = self.walk_stmts(…)` gets a `?`, the same way …
    }
    Some(env)                                             // 3. the end
}
```

The `?` is the whole rule: if a block never falls through, neither does the
statement that contains it, and the rest of the enclosing block isn't
reached.

**Why:** in `early`, the arm that sets `f = a` returns. If its env reached the
join, `f()` would see `a`, an edge on a path that has already left the
function.

### Decision 3: falling off the end returns `None`

When the body's `walk_stmts` returns `Some`, the end was reachable, and the
function returns `None` there: ty's `None` type, lifted to `Ty(None)`.

**Why it matters:** `nothing()` has a value, `None`. Calling it gives no edge,
because `None` is not callable. That is different from a call that has *no*
value at all (decision 4).

### Decision 4: empty `Values` means no value reaches here

`fail()` always raises, so its `returns` stay empty, and the call's value is
an empty `Values`. Not `Unknown`: `Unknown` means "some value we know nothing
about"; empty means "no path gets here with a value".

What a call does with an empty argument:

- Python never makes the call `log(<nothing>)`, because `fail()` raised first.
- The Jedi driver finds calls by syntax, so it shows `log` anyway.
- So the call is shown and walked, with the empty parameter bound as
  `Unknown`, and its value is empty.

```rust
let runs = !args.has_empty();     // every argument has at least one value
// … bind (empty slots as Unknown), walk, push the node …
if runs { callee.returns } else { Values::new() }
```

This rule matters again in part 02, where an empty value is how a recursive
call that hasn't produced anything yet looks.

### Decision 5: generators, `async`, and the recursion guard

- A function whose body contains `yield` returns `[Unknown]`: calling it gives
  a generator object, not the yielded values. Its body is still walked.
- `await x` evaluates to `x`'s value. Calling an `async def` returns its
  returns directly. The coroutine object is not modelled.
- A call stopped by the recursion guard (step 06) returns `[Unknown]`. Part 02
  finds its real value.

### Yours

- `returns` in `Frame`, `none_value()`, `sem.none_type()`.
- `walk_stmts` returns `Option<Env>`. `Return`, `Raise`, and `Env::join` over
  `Option`.
- `call_function` returns `Values`: the callee frame's returns, plus `None` if
  the end was reachable. `[Unknown]` for a generator or a recursive call.
- `eval_call` builds the call's value as in decision 1.
- `Expr::Await` in `eval`: the inner value.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/env.rs: join over Option
pub fn join(arms: Vec<Option<Env<'db>>>) -> Option<Env<'db>> {
    let mut arms = arms.into_iter().flatten();
    let mut out = arms.next()?;
    for arm in arms {
        // … the per-name union from step 09 …
    }
    Some(out)
}
```

```rust
// src/interp/call.rs: call_function, the inline walk part
let parsed = self.sem.parsed(def);
let Some(node) = self.sem.function_node(def, &parsed) else { return smallvec![AbstractValue::Unknown] };
let runs = !args.has_empty();

self.path.push(qname.clone());
let caller = std::mem::replace(&mut self.frame, Frame::new(self.sem, def));
let env = self.bind_parameters(node, args);          // an empty slot binds [Unknown]
if self.walk_stmts(&node.body, env).is_some() {
    let none = self.none_value();
    self.frame.returns.extend(none);                 // fell off the end
}
let callee = std::mem::replace(&mut self.frame, caller);
self.path.pop();

self.frame.children.push(Node { qname, site: Some(site), marks: Vec::new(), children: callee.children });

if !runs {
    Values::new()                                    // Python never makes this call
} else if contains_yield(&node.body) {
    smallvec![AbstractValue::Unknown]                // a generator object
} else {
    callee.returns
}
```

```rust
// src/interp/bind.rs
impl<'db> EvaluatedArgs<'db> {
    pub fn has_empty(&self) -> bool {
        self.positional.iter().any(|v| v.is_empty())
            || self.keywords.iter().any(|(_, v)| v.is_empty())
    }
}
// in bind_parameters, wherever a slot is bound:
//     let v = if v.is_empty() { smallvec![AbstractValue::Unknown] } else { v };
```

`contains_yield(body)` is a `Visitor` that looks for `Expr::Yield` and
`Expr::YieldFrom`, and does not enter nested `def`, `class` or `lambda`.

</details>

---

## Run

```rust
#[test]
fn p01_10_returns() {
    check("01-functions/python/returns.py");
}
```

Then every earlier test. `recursion.py` must still terminate, with the
recursive calls' values now `[Unknown]`.

---

## Where you are

```
Frame { func, file, model, children, returns }
walk_stmts(body, env) -> Option<Env>      None: the path ended (return / raise)
call_function(…) -> Values                the callee's returns on this path
```

Values now flow both ways through a call: arguments in, returns out.

---

## Exercise

1. `def f(): return g` where `g` is defined later in the file at module level.
   Does `f()()` resolve? Who answers `g`?
2. `x = fail(); x()`. What nodes appear, and what does `x` hold?
3. Why does `call_function` still walk `log` when its argument is empty,
   instead of skipping it?

<details><summary>Answers</summary>

1. Yes. `g` is a module global, so inside `f` the env misses and ty answers
   with `g`'s function literal. The return is `Function(g)`, and the outer call
   gets an edge to `g`.
2. `fail` only. `x` is empty, so `x()` has no callee value at all and produces
   no node. (Python raises on the first line, so that is exact.)
3. To match the Jedi driver, which shows `log` because it finds calls by
   syntax, and to show `log`'s own subtree under it. Only the call's *value* is
   dropped, because Python never gets one.

</details>

---

→ Next: [`11-closures.md`](11-closures.md)
