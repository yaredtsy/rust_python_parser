# 03.07 — Callbacks and higher-order functions

The other case you named. The good news: if [`05`](05-binding-arguments.md) and
[`06`](06-attributes-and-self.md) are right, this is almost free — a callback is
just a parameter whose bound value happens to be callable.

---

## The base case

```python
def on_success(result): audit.record(result)
def on_failure(err):    alert.page(err)

def run(task, cb):
    cb(task())                        # ← callback invoked

def handle_ok():
    run(fetch, on_success)            # → on_success → audit.record

def handle_err():
    run(fetch, on_failure)            # → on_failure → alert.page

def main():
    handle_ok()
    handle_err()
```

> The two `run` calls are in **different functions** on purpose. From one frame
> they would merge into a single `run` node holding both callbacks
> ([`09`](09-path-identity.md#the-merge-rule)) — still correct, but it would not
> demonstrate anything.

Trace it through the machinery you already have:

1. `eval(on_success)` → `Name` not in env → ty → `Type::FunctionLiteral`
   → `lift` → `AbstractValue::Function(on_success)`.
2. `bind_parameters` binds `cb` ← that value in `run`'s env.
3. `walk_body(run)` finds `cb(...)`.
4. `eval_callee(cb)` → **env hit** → `Function(on_success)`.
5. `resolve_call` descends into `on_success`.

**No new code.** Two `run` nodes with correct leaves — provided the two calls
are in *different* functions; from one frame they would merge
([`09`](09-path-identity.md#the-merge-rule)). If your
implementation of 05 and 06 is right, this works on the first try — and if it
doesn't, the bug is in 05, not here.

---

## The variants that need attention

### 1. Bound methods as callbacks

```python
run(fetch, self.handler.on_done)
```

`eval` of an `Attribute` → `member()` → `BoundMethod { func, receiver }`.
Binding `cb` to a `BoundMethod` means `resolve_call` must handle
`Callable::Bound` — bind the receiver to the first parameter. That's the
`receiver: Option<AbstractValue>` argument in `bind_parameters`, already in
the signature from [`05`](05-binding-arguments.md).

**The receiver must survive the trip through `run`'s parameter.** That's why
`BoundMethod` carries an interned `ValueId` rather than being re-derived at the
call site — by the time `cb()` is called, the expression `self.handler.on_done`
is long gone.

### 2. Lambdas

```python
run(fetch, lambda r: audit.record(r))
```

Your parser drops lambda *nodes* (`parser.py:121`) and, because `_scan_children`
returns after `_visit_node`, drops their bodies from the node tree too. But the
**call resolver** is a separate traversal. Decide explicitly:

- **Match current behaviour:** check whether `_analyze_function`'s
  `collect_call_node` descends into `lambdef`. It skips `Class`/`Function`
  instances, and parso's `Lambda` **is** a `Function` subclass — so lambda
  bodies are **not** traversed by the call resolver either. **Verify this with a
  fixture on the Python driver, then match it.** **[check]**
- If you later want them: `AbstractValue::Lambda(ExprLambda)` and treat the body
  as a one-expression function. ~30 lines.

Start by matching. Note it in the code.

### 3. Decorators

```python
@retry
def fetch(): ...
```

`fetch` now denotes `retry(fetch)`. ty gives you the decorator's return type,
which is usually `Unknown` or a generic `Callable` — you lose the underlying
function.

Pragmatic rule, and probably what your current driver effectively does:

```rust
// If a name resolves to a function definition, use the UNDECORATED function.
// Decorators are treated as transparent for call-tree purposes.
```

This is a **deliberate imprecision** that keeps the tree useful. Decorated
functions are extremely common (`@app.route`, `@pytest.fixture`, `@lru_cache`)
and treating them as opaque would blank out large parts of real projects.
Write it down as a documented choice, and add a fixture pinning the behaviour.

### 4. Functions stored in containers

```python
HANDLERS = {"json": handle_json, "csv": handle_csv}
HANDLERS[kind](data)
```

Out of scope ([`04-value-domain.md`](04-value-domain.md) — containers aren't
modelled). `Unknown`, no children. Jedi may occasionally resolve these via
literal dict inference; if a fixture shows your current driver does, decide
whether to add constant-key dict support. **Measure before building** — this is
a common pattern in dispatch-table code, but if it's rare in *your* corpus it's
not worth the machinery.

### 5. `functools.partial`, `map`, `filter`, `sorted(key=...)`

Calls into stdlib. `_is_project_code` returns `False`, so the tree stops there —
and importantly, **the callback passed in is never invoked in your tree**,
because you never descend into `map` to find the `f(x)` call site.

That's current behaviour. Preserve it. If you ever want `map(transform, xs)` to
show `transform` as a child, that's a special case list, not a general
mechanism — and it's a behaviour change.

### 6. Self-referencing / recursive callbacks

```python
def walk(node, visit):
    visit(node)
    for c in node.children: walk(c, visit)     # ← recursion with same callback
```

The `is_ancestor` guard stops the `walk`→`walk` edge.
[`08`](08-termination-and-cycles.md) covers this. Note that the *callback* is
not the recursion — `walk` is — so `visit` still resolves correctly at the depth
where it's first seen.

---

## Why this falls out for free

The key structural property: **`resolve_call` dispatches on the callee's
*value*, not its syntax.** It never asks "is this a name or an attribute or a
parameter" — it asks "what value is here, and is it callable". Higher-order
code is then just first-order code where the value arrived via a parameter.

Guard that property in review. The temptation to special-case
`if let Expr::Name(..) = call.func` for a quick fix is how this generality gets
lost.

---

## Fixtures

```python
# 1. plain function as arg                    run(f, on_success)
# 2. bound method as arg                      run(f, obj.method)
# 3. two callbacks, from two DIFFERENT callers  ← two run nodes, one leaf each
#    (same-frame variant → one run node, two leaves — test both)
# 4. callback passed through 2 levels         outer(cb) → middle(cb) → cb()
# 5. callback stored on self, called later    self.cb = cb ... self.cb()
# 6. decorated function called                @retry def fetch(); fetch()
# 7. lambda as callback                       ← pin current behaviour
# 8. class passed as a factory                run(f, JsonWriter)  → constructor
```

Fixture 5 is the one that ties this chapter to [`06`](06-attributes-and-self.md)
— a callback that becomes instance state. If 3, 4, and 5 pass, higher-order
support is done.

---

→ Next: [`08-termination-and-cycles.md`](08-termination-and-cycles.md)
