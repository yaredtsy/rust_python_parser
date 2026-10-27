# 03.11 — Closures and callbacks

Functions as values. If binding ([`06`](06-binding-arguments.md)), keys
([`08`](08-keys.md)) and summaries ([`09`](09-summaries.md)) are right, most of
this chapter needs no new code. A callback is a parameter whose value happens to
be callable, and a closure is a function value that carries the values it
captured.

---

## Closures

### Where a nested call belongs

```python
def outer():
    h = Handler()
    def inner():
        h.go()
    inner()
```

A `def` statement binds a function value. It does not run the body. `h.go()`
belongs to `inner`'s context, reached through the call `inner()` in `outer`'s
body. The `def` itself produces no edge.

### What a closure captures

`inner` reads `h`, a local of `outer`, not a parameter of its own. The `def`
binds `inner` to `Closure { func: inner, activation }`, where `activation` is
the run of `outer` the `def` ran in ([`05`](05-the-walk.md#activations)).

When `inner` is called, its key's `free` list is read from that activation,
never from the caller's env ([`08`](08-keys.md#fingerprinting)). `inner`'s walk
starts with those values in a new activation that becomes its `enclosing`.

### Escaping closures

The case that decides whether this works is a closure called from a different
function than the one that defined it:

```python
def make_handler(writer):
    def handle(data):
        writer.write(data)      # writer: a free variable of handle
    return handle

def read_json():
    h = make_handler(JsonWriter())
    h(1)                        # called here, where there is no `writer` at all

def read_csv():
    h = make_handler(CsvWriter())
    h(2)
```

The Jedi driver resolves `read_json`'s `handle` to `JsonWriter.write` and
`read_csv`'s to `CsvWriter.write` **[verified]**, with `dynamic_params` off.

In pylspt:

1. `make_handler`'s walk binds `handle` to a closure over its own run, and
   returns it. The summary returns
   `Closure { func: handle, free: [(writer, [Ref(Param(0))])] }`.
2. `read_json` resolves that against its snapshot: `Param(0)` is its
   `JsonWriter` object. The result is a closure value over a new activation
   holding `writer → that object`.
3. `h(1)` keys `handle` with `free = [(writer, Instance{JsonWriter})]`.
   `handle`'s walk finds `JsonWriter.write`.

`read_csv` does the same with its own `CsvWriter`, which gives a different key.
The answer does not depend on where `h` is called from, only on where it was
made.

A same-frame fixture (`inner` defined and called inside `outer`) cannot tell
this apart from reading the caller's env, because there the two are the same
env. Use the cross-function form.

### Late binding

Inside one walk, a closure reads its activation's slot at call time, as Python
reads a cell:

```python
def make():
    handler = None
    def run(): handler.go()
    handler = JsonHandler()
    return run              # the summary carries the free values at this exit
make()()                    # JsonHandler.go
```

### A closure that captures itself

```python
def outer(root):
    def walk(n):
        walk(n.child)       # walk is a free variable whose value is walk itself
    walk(root)
```

The fingerprint of `walk`'s free variable `walk` is `Back(0)`
([`08`](08-keys.md#fingerprinting)), so the inner call keys to the same context
as the outer one, and the recursion is recorded, not entered
([`12`](12-recursion.md)). The same rule covers two nested defs that call each
other.

### Known limit: rebinding through `nonlocal`

Each closure in a summary carries its own copy of its free values. Two closures
that share one binding, where one rebinds it with `nonlocal`, don't see each
other's rebinding ([`15`](15-checks-and-build-order.md#known-limits)). Mutating
a captured object is shared, because both copies point at the same object.

---

## Callbacks

```python
def on_success(result): audit.record(result)
def on_failure(err):    alert.page(err)

def run(task, cb):
    cb(task())

def handle_ok():  run(fetch, on_success)    # → on_success → audit.record
def handle_err(): run(fetch, on_failure)    # → on_failure → alert.page
```

1. `on_success` is a module-level function: ty's `FunctionLiteral`, lifted to
   `Function(on_success)`.
2. `run(fetch, on_success)` keys `run` with `cb = Fp::Function(on_success)`.
   `run(fetch, on_failure)` gives a different key, so two contexts of `run`.
3. In `run`'s walk, `cb` is `Function(on_success)`. `cb(...)` is a call on a
   value like any other.

No new code. If this fails while binding and keys work, the bug is in one of
those.

**The call dispatches on the callee's value, not its syntax.** It never asks
"is this a name or an attribute or a parameter", only "what value is here, and
is it callable". Higher-order code is first-order code where the value arrived
through a parameter. Guard this in review: a quick `if let Expr::Name(..) =
call.func` special case is how the generality gets lost.

### Bound methods as callbacks

```python
class Sink:
    def store(self, h): self.h = h
    def flush(self):    self.h.process()

def run(cb): cb(JsonHandler())

def main():
    s = Sink()
    run(s.store)
    s.flush()             # JsonHandler.process
```

`s.store` is `BoundMethod { store, receiver: s }`. It crosses into `run` as
`Fp::Bound { recv }`, and inside `run` the receiver sits at path
`Param(0) + [Recv]`. `store`'s effect on its `self` lands there, so `run`'s
summary has the effect `Param(0).Recv.h = Obj(0)`. `main` follows `Recv` from
its own bound-method value to `s`, and writes `h`.

### Lambdas

parso's `Lambda` is a `Function` subclass, and the Jedi driver's call collector
skips nested functions, so lambda bodies are probably not traversed there.
**[check]** with a fixture against the Jedi driver, then match it. If lambdas
are wanted later: a lambda is a one-expression closure, the same machinery as a
nested `def`.

### Decorators

```python
@retry
def fetch(): ...
```

`fetch` is really `retry(fetch)`, and ty types it as the decorator's return
value, which loses the function. **Decorators are transparent**: a name bound by
a `def` resolves to that `def`'s function ([`04`](04-values.md#the-operations)).
This is a deliberate imprecision, pinned by a fixture. `@app.route`,
`@pytest.fixture` and `@lru_cache` are everywhere, and treating them as opaque
would blank out large parts of real projects. The binding side has its own
safety rule ([`06`](06-binding-arguments.md#five-things-that-are-easy-to-get-wrong)).

### Out of scope, on purpose

- **Functions in containers**: `HANDLERS[kind](data)` is `Unknown`. Containers
  are not modelled. If the Jedi driver resolves constant-key dict literals,
  measure how common the pattern is before adding support.
- **Stdlib higher-order functions**: `map(transform, xs)`,
  `functools.partial(f, x)`, `sorted(key=...)`. The callee is stdlib, so the
  callback is never called in the tree. That is current behaviour. A special
  case list would be a behaviour change.

### Recursive callbacks

```python
def walk(node, visit):
    visit(node)
    for c in node.children: walk(c, visit)
```

`walk` calling itself with the same `visit` keys to the same context, so it is
recorded, not entered. `visit` still resolves in `walk`'s one walk. The callback
is not the recursion; `walk` is.

---

## Fixtures

```python
# 1. escaping closure: the defining run, not the caller
#    read_json → handle → JsonWriter.write; read_csv → handle → CsvWriter.write

# 2. no caller lookup: a name the caller has must not leak in
def g(): x.write()          # x is not local and not free → ty, NOT f's x
def f(x): g()

# 3. late binding: JsonHandler.go, not None's
def make():
    handler = None
    def run(): handler.go()
    handler = JsonHandler()
    return run
make()()

# 4. same name in the caller, on purpose
def make(h):
    def run():
        h.process()
    return run
def main():
    h = CsvHandler()
    r = make(JsonHandler())
    r()                     # JsonHandler.process only

# 5. one function, two runs of make_handler
def main():
    a = make_handler(JsonWriter())
    b = make_handler(CsvWriter())
    a(1)                    # JsonWriter.write
    b(2)                    # CsvWriter.write

# 6. a closure that captures itself: one context of walk, a Recursive leaf

# callbacks
# 7.  plain function as an argument            run(f, on_success)
# 8.  bound method as an argument, with an effect through Recv: the Sink example
# 9.  two callbacks from two different callers: two run contexts, one leaf each
# 10. callback passed through two levels       outer(cb) → middle(cb) → cb()
# 11. callback stored on self, called later    self.cb = cb ... self.cb()
# 12. decorated function called                @retry def fetch(); fetch()
# 13. lambda as callback                       pin the Jedi driver's behaviour
# 14. class passed as a factory                run(f, JsonWriter) → constructor
```

Fixtures 1, 4 and 5 are the ones a same-frame shortcut passes by accident.
Fixture 11 ties callbacks to attribute state.

---

→ Next: [`12-recursion.md`](12-recursion.md)
