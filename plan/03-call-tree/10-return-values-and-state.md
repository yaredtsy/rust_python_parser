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
`bind_parameters` and `walk_body` from [`05`](05-binding-arguments.md), the
attribute store from [`06`](06-attributes-and-self.md).

---

## 1. Local assignment tracking (cheapest, do first)

Without this, return values have nowhere to land.

Thread the env through the statement loop by value — the pattern already
introduced in [`06`](06-attributes-and-self.md#attributes-written-outside-__init__):

```rust
fn walk_body(&mut self, func, env: &Env<'db>, frame: &mut Frame<'db>) {
    let mut env = env.clone();                 // cheap: rpds structural sharing
    for stmt in statements_in_scope(func) {
        // 1. emit call-tree nodes for calls in this statement
        for call in calls_in(stmt) { self.resolve_call(call, &env, frame); }

        // 2. THEN record what this statement binds, for subsequent statements
        env = self.record_bindings(stmt, env);
    }
}
```

**Order matters.** Resolve calls in the statement using the env *before* it,
then update. `w = make(); w.write()` are separate statements, so `w` is bound by
the time `w.write()` is reached.

```rust
fn record_bindings(&mut self, stmt: &ast::Stmt, env: Env<'db>) -> Env<'db> {
    match stmt {
        ast::Stmt::Assign(a) => {
            let v = self.eval(&a.value, &env, model);
            let mut env = env;
            for target in &a.targets {
                match target {
                    ast::Expr::Name(n)      => env = env.bind_local(&n.id, v.clone()),
                    ast::Expr::Attribute(x) => env = self.bind_attribute(x, v.clone(), env),
                    _ => {}   // tuple/starred destructuring → not modelled
                }
            }
            env
        }
        ast::Stmt::AnnAssign(a) if a.value.is_some() => { /* same */ }
        ast::Stmt::With(w)  => { /* bind `as` targets from the CM's __enter__ */ }
        ast::Stmt::For(f)   => { /* bind the loop var → Unknown, or element type */ }
        _ => env,
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
| loop-carried values (`x` reassigned in a loop body) | last-write-wins, single pass |

Last-write-wins with a single linear pass, consistent with the "no branch
analysis" rule. Do **not** build a fixed point over loops.

---

## 2. Return-value flow

```rust
fn eval_call_result(&mut self, call: &ast::ExprCall, env: &Env<'db>, model)
    -> Values<'db>
{
    let callees = self.eval_callee(&call.func, env, model);
    let args    = self.eval_arguments(&call.arguments, env, model);
    let mut out = Values::new();

    for callee in callees {
        match callee.as_callable(self.db) {
            // (a) constructor → a NEW instance with a fresh origin
            Some(Callable::Class(c)) => {
                let origin = self.fresh_origin(call, env.path_key());
                out.push(AbstractValue::Instance { class: c, origin });
            }

            // (b) ★ project function → interpret its returns
            Some(Callable::Function(f)) | Some(Callable::Bound { func: f, .. })
                if self.is_project_code(f) =>
            {
                let callee_env = self.bind_parameters(f, &args, callee.receiver(), env);
                out.extend(self.eval_returns(f, &callee_env));
            }

            // (c) anything else → ty's declared return type
            _ => out.extend(lift(self.db, self.env(),
                    call.inferred_type(model).unwrap_or(Type::unknown()))),
        }
    }
    self.cap_fanout(out)
}
```

```rust
enum Memo<'db> { InFlight, Done(Values<'db>) }

fn eval_returns(&mut self, func: FunctionDef<'db>, env: &Env<'db>) -> Values<'db> {
    let key = (func, fingerprint(env.locals()));

    // ★ The memo table IS the recursion guard. An in-flight entry means we
    //   re-entered this query while computing it — a cycle. Yield Unknown and
    //   let the outer frame finish.
    match self.return_memo.get(&key) {
        Some(Memo::InFlight) => return smallvec![AbstractValue::Unknown],
        Some(Memo::Done(v))  => return v.clone(),
        None => {}
    }
    if self.return_depth >= MAX_RETURN_DEPTH { return smallvec![AbstractValue::Unknown] }

    self.return_memo.insert(key, Memo::InFlight);
    self.return_depth += 1;

    let mut out = Values::new();
    for ret in return_statements_in_scope(func) {      // NOT into nested defs
        match &ret.value {
            Some(e) => out.extend(self.eval(e, env, model)),
            None    => out.push(AbstractValue::Unknown),   // bare `return`
        }
    }

    self.return_depth -= 1;
    let out = self.cap_fanout(out);
    self.return_memo.insert(key, Memo::Done(out.clone()));
    out
}
```

### The five things to get right

**1. It emits no tree nodes.** Return evaluation is a *value query*, not a tree
walk. The call `make_writer()` still appears as a child of the current frame —
that comes from the ordinary `resolve_call` on the same expression. These are
two independent traversals of the same function, and conflating them
double-counts every factory in your tree.

That separation is also what makes `return_memo` safe: a pure query with no
side effects on the frame can be cached freely.

**2. The cycle guard here is *not* the tree's `is_ancestor`.** These are
different kinds of thing and it's worth being explicit about why, because
"can't I just reuse the ancestor check?" is the obvious question.

| | `is_ancestor` (tree walk) | in-flight memo (value query) |
|---|---|---|
| Purpose | **output semantics** — defines the tree's shape | **termination** — implementation detail |
| Observable? | yes, in the JSON | no |
| Free to change? | no, it's the contract | yes, any correct mechanism |

`is_ancestor` is in the output spec: a function may appear many times in the
tree, just not inside itself (`call_resolver.py:157`). It is not there to make
the walk terminate — it is what "unique path per function" *means*.

Reusing it for value queries fails in both directions:

```python
def f(): return g()
def g(): return f()
def main():
    x = f()          # value query. tree ancestors = [root, main] — f and g
    x.run()          # are NOT on it, because value queries push no tree frames.
```
→ an ancestor check never fires; infinite recursion.

```python
def walk(node):
    r = walk(node.first())    # value query on walk…
    r.close()                 # …while `walk` IS a tree ancestor
    return Handle()
```
→ an ancestor check fires and returns `Unknown`, so `r.close()` dead-ends —
even though `walk`'s return is plainly `Handle`.

The in-flight memo gets both right, and it's one mechanism doing two jobs
(caching + termination) instead of a hand-rolled stack doing one.

> This mirrors how ty handles the same problem internally: `Type::Divergent` is
> documented as *"A cycle marker used during recursive type inference"*
> **[verified, types.rs:1632]**, alongside `types/cyclic.rs`. If you build this
> inside the ruff workspace (Option A), consider making `eval_returns` an actual
> `#[salsa::tracked]` query with salsa cycle recovery rather than a hand-rolled
> table — same semantics, and you inherit the incrementality. **[check]** that
> the env fingerprint can be expressed as a salsa ingredient first; if not, the
> plain table above is fine.

**3. Multiple returns union.** `if x: return A() else: return B()` → both.
Consistent with the no-branch-analysis rule. Fan-out cap applies —
`MAX_UNION_FANOUT` from [`04`](04-value-domain.md).

**4. `async def` / `await`.** Treat `await f()` as transparent: unwrap
`ast::Expr::Await` and evaluate the inner call directly. Modelling coroutine
objects buys nothing here.

**5. Generators.** A function containing `yield` returns a generator, not the
yielded value. Detect `yield` in the body and return `Unknown` rather than
unioning the yield expressions — otherwise `for x in gen(): x.run()` resolves
`x` to the generator.

### Budget

```rust
const MAX_RETURN_DEPTH: usize = 6;     // vs MAX_DEPTH = 24 for the call tree
```

Shallower on purpose. `handler = registry.get(k).build()` is depth 2; chains
past ~4 are rare and rarely informative. Every `eval_returns` also ticks the
global budget from [`08`](08-termination-and-cycles.md).

---

## 3. Object state beyond `__init__`

[`06`](06-attributes-and-self.md) specifies the attribute store keyed on
`OriginId`. Two generalisations, both cheap once `record_bindings` exists:

### (a) Any method, not just `__init__`

Promote option **(b)** from
[`06`](06-attributes-and-self.md#attributes-written-outside-__init__) to the
default. Since `record_bindings` already handles `Attribute` targets, walking
any method body with a bound `self` records `self.X = ...` automatically. No
extra code — just don't special-case `__init__`.

### (b) Any receiver, not just `self`

```python
p = Pipeline()
p.handler = JsonHandler()      # mutation from outside the class
p.run()                        # → JsonHandler.process
```

```rust
fn bind_attribute(&mut self, attr: &ast::ExprAttribute, v: Values<'db>, env: Env<'db>)
    -> Env<'db>
{
    // Evaluate the receiver; if it's an instance we know, write to its slot.
    for recv in self.eval(&attr.value, &env, model) {
        if let AbstractValue::Instance { origin, .. } = recv {
            return env.bind_attr(origin, &attr.attr.id, v);
        }
    }
    env    // unknown receiver → drop the write
}
```

`self.x = v` is then just the case where the receiver happens to be the first
parameter. One code path, both behaviours.

**Dropping writes to unknown receivers is deliberate.** There is no sound way to
model "some object of unknown identity was mutated" without invalidating
everything, and unsound over-approximation here would corrupt unrelated paths.

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

# the tree-shape invariant
w = make(); w.write(1)
# 12. `make` appears ONCE as a child (from the call-site walk),
#     and is NOT double-counted by the return-value query
```

Fixture 11 proves `OriginId` isolation under external mutation; fixture 12
proves the value-query / tree-walk separation. Those two are the ones that catch
architectural mistakes rather than logic slips — write them first.

---

## Build order

Slot into M6 after attributes:

| | | Gate |
|---|---|---|
| M6.6 | local assignment tracking | fixtures 7, 8 |
| M6.7 | return-value flow | fixtures 1–6, 12 |
| M6.8 | generalised object state | fixtures 9, 10, 11 |

Do **not** build return flow before local assignment tracking — without
somewhere to land, returns are unobservable and you'll be debugging two things
at once.

---

→ Next: [`04-build/00-dev-cli.md`](../04-build/00-dev-cli.md)
