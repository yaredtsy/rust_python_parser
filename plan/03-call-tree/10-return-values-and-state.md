# 03.10 — Return values and object state

The three mechanisms that make value flow work. Present in today's behaviour via
Jedi ([`01`](01-what-jedi-actually-does.md#present-via-jedi-not-via-the-resolver-)),
absent from `call_resolver.py`'s explicit code, and **lost by default in a
line-by-line port**.

All three share one machinery: **record bindings into the env as you walk**.

---

## Why they're worth it

Without value flow, this dead-ends:

```python
def build(cfg):
    return JsonWriter() if cfg.json else CsvWriter()

def run(cfg):
    w = build(cfg)        # ← w is Unknown without return flow
    w.write(1)            # ← no children. tree stops here.
```

That is not an exotic pattern — it is factories, builders, `get_handler()`,
service locators, and DI containers, i.e. exactly the code where a call tree is
worth having. A tool that stops at every `x = make_thing()` is not much of a
tool.

The cost is bounded, and the machinery is mostly already specified:
`bind_parameters` from [`05`](05-binding-arguments.md), the
attribute store from [`06`](06-attributes-and-self.md).

---

## 1. Local assignment tracking (cheapest, do first)

Without this, return values have nowhere to land.

Thread the env through the statements by value — the pattern already
introduced in [`06`](06-attributes-and-self.md#attributes-written-outside-__init__)
— with **one statement walker** for every body. The tree walk uses it to
emit frames; the same walk also collects every `return` value it reaches
(§2). Node output can be turned off for the rare case that needs a value
without a tree walk; it is still this walker, never a second one.

```rust
/// One run of one function body. Each run gets its own id, and its slot
/// holds the run's CURRENT env, updated as the walk moves. Closures defined
/// in the run point here (05 § enclosing).
pub struct ActivationId(u32);           // index into Interp.activations: Vec<Env<'db>>

/// What one walk produces besides bindings.
struct Mode<'a, 'db> {
    frame:   Option<&'a mut Frame<'db>>, // Some → every call evaluated adds a node
                                         // None → node output off, same walk
    returns: Values<'db>,                // union of every `return` reached — always collected
}
// Mode::tree(frame) / Mode::value() build the two cases.
// The entry point that mints the run and stores the returns is §2's walk_function.

/// `None` = this block never falls through (it always returns or raises),
/// so it contributes nothing at a join.
fn walk_stmts(&mut self, stmts: &[ast::Stmt], mut env: Env<'db>,
              act: ActivationId, mode: &mut Mode<'_, 'db>) -> Option<Env<'db>> {
    for stmt in stmts {
        self.activations[act] = env.clone();          // the slot — closures read it
        env = match stmt {
            // ★ fork and join. Every arm starts from the env BEFORE the `if`;
            //   arms are never run one after another on the same env.
            ast::Stmt::If(s) => {
                self.eval(&s.test, &env, mode);               // value unused; its calls are walked
                let before = env;
                let mut arms = vec![self.walk_stmts(&s.body, before.clone(), act, mode)];
                for clause in &s.elif_else_clauses {
                    if let Some(t) = &clause.test { self.eval(t, &before, mode); }
                    arms.push(self.walk_stmts(&clause.body, before.clone(), act, mode));
                }
                if !has_else(s) { arms.push(Some(before)); }   // no else: skipping is an arm
                Env::join(arms)?                               // every arm returned → so does this
            }
            ast::Stmt::Match(m) => { /* eval the subject, then as If: one arm per case; `case _` is the else */ }

            // The body runs zero or more times: walk it ONCE, join with "didn't run".
            // No fixed point — a value set on the second iteration isn't seen.
            ast::Stmt::For(_) | ast::Stmt::While(_) => {
                self.eval(loop_head(stmt), &env, mode);    // the `for` iterable / `while` test
                let before = env;
                let body = self.walk_stmts(loop_body(stmt), bind_loop_target(stmt, before.clone()), act, mode);
                let after = Env::join(vec![Some(before), body])?;
                self.walk_stmts(loop_orelse(stmt), after, act, mode)?
            }

            // A handler can start after any statement of the body; approximate
            // its start as before ∪ body-end.
            ast::Stmt::Try(t) => {
                let before = env;
                let body = self.walk_stmts(&t.body, before.clone(), act, mode);
                let start = Env::join(vec![Some(before), body.clone()]).unwrap();
                let mut arms = vec![body.and_then(|e| self.walk_stmts(&t.orelse, e, act, mode))];
                for h in &t.handlers { arms.push(self.walk_stmts(h.body(), start.clone(), act, mode)); }
                let joined = Env::join(arms)?;
                self.walk_stmts(&t.finalbody, joined, act, mode)?
            }

            ast::Stmt::With(w) => {                    // body always runs: sequential
                let env = self.bind_with_targets(w, env, mode);   // evals each context expr, binds `as`
                self.walk_stmts(&w.body, env, act, mode)?
            }

            ast::Stmt::Return(r) => {
                if let Some(e) = &r.value {
                    let v = self.eval(e, &env, mode);    // walks any call in it, returns its value
                    mode.returns.extend(v);
                } else { mode.returns.push(AbstractValue::Unknown); }   // bare `return`
                return None;
            }
            ast::Stmt::Raise(r) => { /* eval the exception expr(s) */ return None; }

            // Everything else is straight-line: evaluate the statement's
            // expressions in the env BEFORE it, then bind for the statements after.
            other => self.exec_simple(other, env, act, mode),
        };
    }
    self.activations[act] = env.clone();
    Some(env)
}
```

### ★ Evaluation drives the walk — there is no call list

There is no separate call finder. Calls are found because evaluation
reaches them: `eval` on an `ast::Expr::Call` *is* resolving the call
(§2's `eval_call`), and it returns the call's value to whoever evaluated
it. Python's own order falls out of that, with nothing to check:

```
eval(emit(make()))
  eval(emit)          → Function(emit)            1. the function
  eval(make())        → walks make, returns its value   2. the arguments, left to right
  call emit with it   → walks emit                3. then the call
```

Each argument, including any call inside it, is fully evaluated before the
outer call happens. Nested blocks are walked by `walk_stmts` with their own
env; everything else is an expression some statement evaluates. With
`frame: Some` each call reached adds a node; with `frame: None` nothing is
emitted, but bodies are still walked for their values.

**Every expression is evaluated exactly once per walk.** A second
evaluation would walk the callee again into the same merged frame and bump
`call_count`, which is in the output. So `x = y = f()` evaluates `f()`
once, and `exec_simple` never evaluates the right-hand side of an
assignment as "the calls" and again as "the value".

**Positions evaluation must reach.** The call finder this replaces
(`ty_ide`'s `OutgoingCallsFinder`, used in 03's first version) also
reported calls in places that aren't plain statement expressions. Each
must be evaluated by the statement that owns it, in the current env:

| statement | evaluate |
|---|---|
| `def` | decorators, default values, annotations — then bind the name |
| `class` | decorators, base classes, keywords (`metaclass=`) — then bind the name |
| `for` / `while` / `if` / `match` | the iterable / test / subject, before the body |
| `with` | each context expression, before binding `as` |
| `assert`, `del`, `raise`, augmented assignment, `x: T = v` | every expression in it |

**[check]** against the golden files: annotations (evaluated at runtime only
without `from __future__ import annotations`), class bodies run inside a
function, comprehensions and lambdas (a lambda body runs only when called).
Match whatever the Jedi driver reports — the contract is the output
([`01`](01-what-jedi-actually-does.md)). The memory note *scope entry must
be walk_node* applies: a walk that starts at `def.body` misses the
decorators and defaults.

**Child order in the JSON is a separate question.** Evaluation order adds
`make` before `emit` for `emit(make())`; a source-order finder may have
added `emit` first. If the golden files show source order, record each
child's first call-site position in `add_child` and sort children by it
when serialising ([`09` § Determinism](09-path-identity.md#determinism-)).
Don't change the evaluation order to get the output order.

`Env::join` unions per name: a name any arm wrote gets the union of every
reaching arm's value (the "before" value counts for an arm that didn't
write it). `attrs` joins the same way. `None` arms are skipped; all `None`
→ `None`. It's cheap because each arm can record which names it wrote.

**Order matters.** Evaluate the statement in the env *before* it, then
bind. `w = make(); w.write()` are separate statements, so `w` is bound by
the time `w.write()` is reached.

**Why fork and join, when conditions are never evaluated.** Not evaluating
a condition means *every arm is possible*, not that the arms run in
sequence. A single linear pass gets both directions wrong:

```python
handler = None
if c:
    handler = JsonHandler()
else:
    handler.process()      # linear pass: sees JsonHandler — an edge Python can't take

if c:   w = JsonWriter()
else:   w = CsvWriter()
w.write()                  # linear pass: CsvWriter only — loses JsonWriter.write
```

With `rpds`, keeping "before" is a pointer copy, so forking costs nothing;
the arm envs live only until the join. (An earlier draft walked a flat
`statements_in_scope(func)` list with last-write-wins. Retired: it picked
the last arm, which is choosing a branch, the thing this design doesn't do.)

```rust
/// A straight-line statement: evaluate its expressions once, in Python's
/// order, in the env BEFORE it; then bind.
fn exec_simple(&mut self, stmt: &ast::Stmt, env: Env<'db>, act: ActivationId,
               mode: &mut Mode<'_, 'db>) -> Env<'db> {
    match stmt {
        ast::Stmt::Assign(a) => {
            let v = self.eval(&a.value, &env, mode);     // ONCE, however many targets
            let mut env = env;
            for target in &a.targets {
                match target {
                    ast::Expr::Name(n)      => env = env.bind_local(&n.id, v.clone()),
                    ast::Expr::Attribute(x) => env = self.bind_attribute(x, v.clone(), env, mode),
                    other => { self.eval_target_parts(other, &env, mode); }  // `a[f()] = v`, tuples: Unknown
                }
            }
            env
        }
        ast::Stmt::AnnAssign(a) => { /* eval annotation [check], then value (if any) once; bind as above */ }
        ast::Stmt::Expr(e)      => { self.eval(&e.value, &env, mode); env }   // `f(x)` on its own line
        // (With / For / If / Try / Match / Return / Raise are structural — walk_stmts handles them.)

        // A nested def: evaluate decorators and defaults HERE (they run when
        // the `def` runs, in this env), then bind. The name points at THIS
        // run, so a later call from any frame reads this run's env as it is
        // at that moment (05 § enclosing). At module level there's no run to
        // point at; the name goes to ty — but decorators/defaults still run.
        ast::Stmt::FunctionDef(d) => {
            for dec in &d.decorator_list { self.eval(&dec.expression, &env, mode); }
            self.eval_defaults_and_annotations(&d.parameters, d.returns.as_deref(), &env, mode);
            if !env.is_function_scope() { return env }
            let func = self.definition_of_def(d);           // 04: decorated → still the def
            env.bind_local(&d.name.id, smallvec![AbstractValue::Closure { func, activation: act }])
        }
        ast::Stmt::ClassDef(c) => {
            for dec in &c.decorator_list { self.eval(&dec.expression, &env, mode); }
            for base in c.bases() { self.eval(base, &env, mode); }
            for kw in c.keywords() { self.eval(&kw.value, &env, mode); }
            env   // [check] class body inside a function — see the positions table
        }
        other => { self.eval_all_exprs(other, &env, mode); env }   // assert, del, aug-assign, …
    }
}
```

**Rebinding works for free**, and is very common:

```python
w = JsonWriter(); w.write(1)      # → JsonWriter.write
w = CsvWriter();  w.write(2)      # → CsvWriter.write   ← different, correctly
```

**Note this beats ty even when ty could answer.** ty is flow-sensitive within a
scope, so it would narrow `w` correctly here — but only because both values are
constructors. The moment `w = build(cfg)`, ty gives the declared return type and
you need the interpreter.

### Scope limits (keep them)

| Not modelled | Result |
|---|---|
| tuple/list destructuring `a, b = f()` | targets → `Unknown` |
| augmented assign `x += f()` | `Unknown` |
| `global` / `nonlocal` | `Unknown` |
| walrus `:=` in a condition | bind it; it's a plain assignment |
| loop-carried values (`x` reassigned in a loop body) | body walked once, joined with "didn't run" |

Sequential statements: a later write replaces an earlier one. Branches:
every arm from the same "before", union at the join. Loops: one pass, then
join. Do **not** build a fixed point over loops.

---

## 2. Return-value flow

**Walking a call returns its value.** `eval` on a call expression walks
the callee (§1, "Evaluation drives the walk") and hands the union of every
`return` it reached straight back to whoever evaluated the call — the
assignment `w = make()`, the argument in `emit(make())`, the receiver in
`make().run()`. There is no separate value traversal, and no lookup on the
normal path.

```rust
/// `eval` on a call expression. This IS resolving the call — 03's
/// `resolve_call` steps (a)–(e) live in `call_one`.
fn eval_call(&mut self, call: &ast::ExprCall, env: &Env<'db>, mode: &mut Mode<'_, 'db>) -> Values<'db> {
    let callees = self.eval(&call.func, env, mode);                  // 1. the function
    let args    = self.eval_arguments(&call.arguments, env, mode);   // 2. arguments, left to right
    let mut out = Values::new();
    for callee in callees {                                          // 3. then the call
        out.extend(self.call_one(call, callee, &args, env, mode));
    }
    self.cap_fanout(out)
}

fn call_one(&mut self, call: &ast::ExprCall, callee: AbstractValue<'db>, args: &EvaluatedArgs<'db>,
            env: &Env<'db>, mode: &mut Mode<'_, 'db>) -> Values<'db> {
    match callee.as_callable(self.db) {
        // (a) constructor → construct_instance (below) walks `__init__`
        Some(Callable::Class(c)) => {
            let child = self.enter(call, c, mode);          // 03 steps: maybe a ClassSchema node
            smallvec![self.construct_instance(c, args, call, env, child)]
        }

        // (b) ★ project function → walk it; its returns ARE the value
        Some(Callable::Function(f) | Callable::Closure { func: f, .. } | Callable::Bound { func: f, .. })
            if self.is_project_code(f) =>
        {
            let callee_env = self.bind_parameters(f, args, callee.receiver(), callee.enclosing(), env);
            match self.enter(call, f, mode) {               // 03 steps (b)–(e), depth, budget
                Enter::Walk(child_mode) => {
                    // Reuse: 08 layer 5's graft, 09's "identical walk" skip.
                    if let Some(v) = self.reuse(f, &callee_env, &child_mode) { return v }
                    self.walk_function(f, &callee_env, child_mode).1
                }
                // Not walked: recursion cut, depth/budget/deadline, no qname/ID.
                Enter::Cut => self.returns.get(&ReturnKey::of(f, &callee_env))
                                  .cloned().unwrap_or(smallvec![AbstractValue::Unknown]),
            }
        }

        // (c) anything else → ty's declared return type
        _ => self.lift(call.inferred_type(mode.model).unwrap_or(Type::unknown())),
    }
}

/// The one way a function body gets walked.
fn walk_function(&mut self, func: Definition<'db>, callee_env: &Env<'db>,
                 mut mode: Mode<'_, 'db>) -> (Option<Env<'db>>, Values<'db>) {
    let act = self.new_activation(callee_env);
    let end = self.walk_stmts(body(func), callee_env.clone(), act, &mut mode);
    let returns = if is_generator(func) { smallvec![AbstractValue::Unknown] }   // point 5
                  else { self.cap_fanout(mode.returns) };
    // Kept only for reuse and for the recursion cut — the caller already has it.
    self.returns.insert(ReturnKey::of(func, callee_env), returns.clone());
    (end, returns)
}
```

`enter` is 03's per-target checks: project code, qname, the per-call
`seen` dedup, docstring ID, `is_ancestor`, then depth and budget. When they
pass, it adds the child node if `mode` has a frame and hands back the
child's mode (node output off if the caller's was off). Otherwise `Cut`.

**The builtin fast path skips the callee, never the arguments.** 03's
step 1 skips a call like `print(...)` by name. Here it means `eval_call`
returns ty's answer for `print` without resolving it — but steps 1 and 2
(evaluate the function, evaluate the arguments) have already run, so
`print(make())` still walks `make`. That matches today: the Jedi driver
finds `make()` as its own call and resolves it regardless of `print`.

### The store: only for reuse and the cut

The value of a normally walked call goes straight back to the caller. The
keyed store (`self.returns`) is read in only two places:

- **Reuse** — a memoised subtree graft (`08` layer 5) or `09`'s skip of an
  identical walk. Nothing is walked, so the value has to come from a
  previous walk with the same inputs.
- **The cut** — `enter` refused to walk (recursion, depth, budget, no
  qname/ID). An earlier *finished* run with the same key can still answer;
  otherwise it's `Unknown`.

So a key that's too narrow can only hurt those two paths, never a normal
call. It still has to hold everything a return depends on:

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
struct ReturnKey<'db> {
    func:      Definition<'db>,
    locals:    rpds::HashTrieMap<Name, Values<'db>>,   // the BOUND params: self, args, defaults
    enclosing: Option<ActivationId>,                    // which run a closure reads
    attrs:     AttrsVersion,                            // see below
}
```

| field | without it, reuse or the cut would return a wrong value for |
|---|---|
| `locals` (bound params, incl. `self`) | `a.get()` vs `b.get()` with different instances |
| `enclosing` | `outer(JsonWriter())()` vs `outer(CsvWriter())()` (fixture 20) |
| `attrs` | `def get(self): return self.h` — `p.h = A(); p.get()` vs `p.h = B(); p.get()` |

`AttrsVersion` is a `u32` the env carries and bumps on every attribute
write (and on a join that changes `attrs`). Two envs with the same version
have the same attrs, so it stands in for the whole map without hashing it.

### Nothing stored means `Unknown`

A cut call finds nothing when no run with its key has finished:

- **Recursion.** Recursion is walked once: `is_ancestor` cuts the inner
  call (03 step (e)), and the inner call reads whatever is stored for its
  key. The outer run hasn't finished, so that's nothing: `Unknown`. This
  is also what guarantees termination — there is no second traversal that
  could loop.
- **A limit.** The depth limit, the node budget or the deadline
  ([`08`](08-termination-and-cycles.md)) stopped the walk before this call.
- **A skip.** `enter` didn't walk the target: no qname, no docstring ID
  (03 steps (b), (d)).

Known gap, accepted for v1 — recursion that only needs the *other* arm's
value:

```python
def walk(node):
    r = walk(node.first())    # inner walk is cut → r = Unknown
    r.close()                 # → no edge, though walk plainly returns Handle
    return Handle()
```

If a fixture shows one of these losing an edge that matters, the fix is to
walk that function with the **same walker** and node output off
(`walk_function(.., Mode::value())`) at the read. Don't add a second walker.

(Two earlier drafts are retired. The first had a second walker as the
main path: `eval_returns`, a value query with its own in-flight memo,
`MAX_RETURN_DEPTH` budget and "two independent traversals" argument. The
second had one walker but still found calls with a separate call finder,
stored every return, and looked values up afterwards — which needed the
finder to visit calls innermost-first. Evaluation reaching the calls
replaces both.)

### ★ `construct_instance()` — the one place a class gets constructed

Called from one place: `call_one`'s class arm (a). Every `Foo()` reaches
it because evaluation reaches it, so there is one origin and one set of
attrs per construction ([`04`](04-value-domain.md#-originkey--exactly-what-goes-in-and-nothing-else)).
`__init__` is walked with the same `walk_function` as any other body.

```rust
fn construct_instance(
    &mut self,
    class: Definition<'db>,
    args: &EvaluatedArgs<'db>,
    call: &ast::ExprCall,
    env: &Env<'db>,
    child: Enter<'_, 'db>,                   // from `enter`: Walk(mode) or Cut
) -> AbstractValue<'db> {
    // The ONLY place an OriginKey is built. Every input `__init__`'s result
    // depends on is in it — site, class, argument values — and nothing else
    // (04 § OriginKey). `is_new`: this is the first time this exact
    // construction is seen, so its attrs aren't known yet.
    let key = OriginKey { file: self.file, range: call.range(), class, args: args.clone() };
    let (origin, is_new) = self.origin_arena.intern(key);
    let instance = AbstractValue::Instance { class, origin };

    if let (Some(init), Enter::Walk(mode)) = (self.lookup_init(class), child) {
        // Walk when this frame needs the nodes, or when the attrs aren't
        // known yet. A node-output-off walk of a known origin adds nothing.
        if mode.frame.is_some() || is_new {
            let init_env = self.bind_parameters(init, args, Some(instance), None, env);
            if let (Some(end), _) = self.walk_function(init, &init_env, mode) {
                if is_new { self.origin_attrs.extend(end.attrs_of(origin)); }
            }
        }
    }
    // No `__init__` in the class body — a dataclass / NamedTuple / attrs
    // synthesised one, or none at all: the class still gets its node
    // (`enter` added it), there's just no body to walk.
    // Attrs stay empty; `member()` falls through to ty's field types.
    instance
}
```

Two frames reaching the same `OriginKey` (same site, same arguments, two
paths) both walk `__init__` in tree mode — each frame needs its nodes — but
only the first fills `origin_attrs`. `__init__` constructing another
instance of its own class at the same site would be unbounded recursion in
the *source program*; `is_ancestor` and the depth limit cut it like any
other recursive call.

### The things to get right

**1. One evaluation, so nothing is double-counted.** `w = make_writer()`
evaluates the call once: that one evaluation adds the `make_writer` node
and returns its value. (The old risk — a value query walking `make_writer`
a second time and adding nodes — is gone because there is no second walk.
The remaining risk is evaluating the same expression twice; §1's "exactly
once" rule.)

**2. Multiple returns union.** `if x: return A() else: return B()` → both.
Consistent with the no-branch-analysis rule. The fan-out cap applies through
`cap_fanout` ([`04`](04-value-domain.md#-the-union-cap-cap_fanout)), which
drops non-project values before deciding the union is too wide.

**3. `async def` / `await`.** Treat `await f()` as transparent: unwrap
`ast::Expr::Await` and evaluate the inner call directly. Modelling coroutine
objects buys nothing here.

**4. A frame walked twice stores twice.** Two calls merged into one frame
(`emit(JsonWriter()); emit(CsvWriter())`, [`09`](09-path-identity.md#the-merge-rule))
are two runs with different `ReturnKey`s, so each stores its own value.
`09`'s "skip an identical walk" shortcut is safe for the same reason: an
identical walk would store an identical value under the identical key —
provided that shortcut's fingerprint is the same `ReturnKey` (attrs
included), not just the arguments.

**5. Generators.** A function containing `yield` returns a generator, not the
yielded value. Store `Unknown` rather than unioning the yield expressions —
otherwise `for x in gen(): x.run()` resolves `x` to the generator.

**6. Stored values can hold closures.** A stored value may be a
`Closure { func, activation }` pointing at the run that produced it, read
long after that run ended. So **activations are never freed before the
request ends**; the slot then holds the run's final env.

**7. Memoised subtrees must still store returns.** `08` layer 5 grafts a
cached subtree instead of walking a context-independent function. A graft
walks nothing, so on its own it would store nothing, and `w = make()` after
a grafted `make()` would be `Unknown`. So the memo entry keeps the return
value from the first walk next to the subtree, and the graft writes it into
`returns` under the current `ReturnKey`. That's only sound if the return
value doesn't depend on the inputs, so the taint rule also counts an input
reaching a `return` as context-dependent (below).

---

## 3. Object state beyond `__init__`

[`06`](06-attributes-and-self.md) specifies the attribute store keyed on
`OriginId`. Two generalisations, both cheap once `exec_simple` exists:

### (a) Any method, not just `__init__`

Promote option **(b)** from
[`06`](06-attributes-and-self.md#attributes-written-outside-__init__) to the
default. Since `exec_simple` already handles `Attribute` targets, walking
any method body with a bound `self` records `self.X = ...` automatically. No
extra code — just don't special-case `__init__`.

### (b) Any receiver, not just `self`

```python
p = Pipeline()
p.handler = JsonHandler()      # mutation from outside the class
p.run()                        # → JsonHandler.process
```

This is [`06`'s `bind_attribute`](06-attributes-and-self.md#write-bind_attribute-one-function-for-every-depth),
unchanged — it already evaluates any receiver expression, not just `self`.
(An earlier draft had a second, simpler copy here that wrote only to the
*first* Instance receiver and had no weak update. It is retired; `06`'s is
the only one.)

`self.x = v` is then just the case where the receiver happens to be the first
parameter. One code path, both behaviours.

**Dropping writes to unknown receivers is deliberate.** There is no sound way to
model "some object of unknown identity was mutated" without invalidating
everything, and unsound over-approximation here would corrupt unrelated paths.
Opaque instances (`OriginId::OPAQUE`, from ty rather than a construction we
saw) count as unknown identity here: they all share one origin, so a write to
one would appear on all of them ([`04`](04-value-domain.md#-opaque-instances-have-no-attribute-slots)).

---

## Interaction with memoisation

⚠ This is the one place the additions genuinely complicate something else.

[`08`](08-termination-and-cycles.md#layer-5--memoise-context-independent-subtrees--the-big-win)
memoises subtrees of *context-independent* functions. With value flow,
`is_context_independent` must get stricter — a function is context-dependent if
a parameter reaches a callee position **transitively through a local binding**:

```python
def run(w):
    x = w              # ← parameter flows into a local
    x.write(1)         # ← which becomes a callee. CONTEXT-DEPENDENT.
```

The syntactic check needs a small local dataflow pass: mark parameters tainted,
propagate through assignments, and flag if a tainted name reaches a callee or
receiver position. Still one pass over the body, still `#[salsa::tracked]`, but
no longer a simple name scan.

Two more things count as reaching an output, now that returns come from the
walk (§2, point 7):

- a tainted name in a **`return`** expression — `def ident(x): return x`
  has no calls but its return depends on `x`, so it can't share one cached
  return across arguments;
- **free variables** are inputs too, like parameters — a closure's
  `return w`, where `w` is from the enclosing run, is context-dependent.

**Being conservative is free.** Marking a function context-dependent when it
isn't costs some memoisation; the reverse produces wrong trees.

---

## Fixtures

```python
# return flow
def make(): return JsonWriter()
w = make(); w.write(1)                       # 1. simple factory

def build(c): return JsonWriter() if c else CsvWriter()
build(True).write(1)                         # 2. two returns → union of 2

def outer(k): return build(k)
outer(1).write(1)                            # 3. chained returns, depth 2

def rec(n): return rec(n - 1)
rec(3)                                       # 4. recursive → Unknown, terminates

def gen(): yield JsonWriter()
for w in gen(): w.write(1)                   # 5. generator → Unknown, NOT JsonWriter

async def afetch(): return JsonWriter()
(await afetch()).write(1)                    # 6. await transparent

# local assignment
w = JsonWriter(); w.write(1)
w = CsvWriter();  w.write(2)                 # 7. rebinding → two DIFFERENT leaves

def f(h):
    x = h
    x.run()                                  # 8. param → local → callee

# object state
p = Pipeline(); p.handler = JsonHandler(); p.run()      # 9. external mutation
class C:
    def setup(self): self.h = JsonHandler()             # 10. write outside __init__
    def go(self):    self.h.process()

a = Pipeline(); b = Pipeline()
a.handler = JsonHandler(); b.handler = CsvHandler()
a.run(); b.run()                             # 11. ★ two instances, no cross-talk

def mk(h): return Pipeline(h)
mk(JsonHandler()).run(); mk(CsvHandler()).run()   # 13. one site, two args → two origins
x = get_logger(); y = get_db()
x.handler = FileHandler(); y.handler.write()      # 14. opaque write dropped, no cross-talk

# branches: fork and join
handler = None
if c: handler = JsonHandler()
else: handler.process()                           # 15. NO JsonHandler.process edge
if c: w = JsonWriter()
else: w = CsvWriter()
w.write()                                         # 16. BOTH JsonWriter.write and CsvWriter.write

def f(h):
    x = h
    return x
f(JsonWriter()).write()                           # 17. return sees the local (not the entry env)

# one walker, return store
emit(make())                                      # 18. make walked BEFORE emit reads its arg
x = y = make()                                    # 22. ONE make node, call_count 0 (evaluated once)
def setup():
    @register(make())                             # 23. decorator and default calls are setup's
    def handler(w=make_default()): ...            #     children — evaluated when the def runs
a = Box(JsonWriter()); b = Box(CsvWriter())
a.get().write(); b.get().write()                  # 19. ReturnKey has `self`: two different edges
def outer(w):
    def get(): return w
    return get
outer(JsonWriter())().write()                     # 20. ReturnKey has `enclosing`:
outer(CsvWriter())().write()                      #     JsonWriter.write, then CsvWriter.write
def walk(node):
    r = walk(node.first()); r.close()
    return Handle()                               # 21. KNOWN GAP: no Handle.close edge (cut → Unknown)

# the tree-shape invariant
w = make(); w.write(1)
# 12. `make` appears ONCE as a child, and its value read adds no node
```

Fixture 11 proves `OriginId` isolation under external mutation; fixtures 12,
18 and 22 prove the evaluation-driven walk (no double count, right order,
each expression once); 23 proves the non-statement positions are reached. Those are
the ones that catch architectural mistakes rather than logic slips — write
them first. Fixture 21 is a known gap: it pins today's answer so a change
is noticed.

### Check against the Jedi driver before committing

Run these through `call_resolver.py` and write down what it produces. Each
one decides a rule:

| fixture | if Jedi resolves it |
|---|---|
| `log = getLogger(); log.handler = FileHandler(); log.handler.write()` | give each opaque value its own origin ([`04`](04-value-domain.md#-opaque-instances-have-no-attribute-slots)) instead of dropping the write |
| `make()` with `handler` reassigned after the `def` ([`05`](05-binding-arguments.md#test-fixtures-for-this-chapter) fixture 12) | confirms the per-run slot; if Jedi gives `None`'s answer, match it |
| `if c: w = Json() else: w = Csv(); w.save()` (fixture 16) | both edges expected; if Jedi shows one, note it as a parity difference |

---

## Build order

Slot into M6 after attributes:

| | | Gate |
|---|---|---|
| M6.6 | local assignment tracking | fixtures 7, 8, 15, 16 |
| M6.7 | return-value flow | fixtures 1–6, 12, 17–23 |
| M6.8 | generalised object state | fixtures 9, 10, 11, 13, 14 |

Do **not** build return flow before local assignment tracking — without
somewhere to land, returns are unobservable and you'll be debugging two things
at once.

---

→ Next: [`04-build/00-dev-cli.md`](../04-build/00-dev-cli.md)
