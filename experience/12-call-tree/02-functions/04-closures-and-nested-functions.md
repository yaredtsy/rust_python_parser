# 02.04 — Closures and nested functions

Which context a nested call belongs to, and where a closure's captured values
come from.

**Fixture:** `python/closures.py`. **Reading:**
[`plan/03-call-tree/11-closures-and-callbacks.md`](../../../plan/03-call-tree/11-closures-and-callbacks.md)
§ "Closures", and [`08-keys.md`](../../../plan/03-call-tree/08-keys.md) § "Fingerprinting".

---

## A `def` binds a value; it doesn't run the body

```python
def outer():
    h = Handler()
    def inner():
        h.go()
    inner()
```

When `outer`'s walk passes `def inner`, it binds `inner` to
`Closure { func: inner, activation }`, where `activation` is this run of
`outer`. The body of `inner` is not walked here. `h.go()` belongs to `inner`'s
own context, reached through the call `inner()`. The `def` produces no edge;
the call does.

## Where captured values come from

`inner` reads `h`, which is `outer`'s local, not `inner`'s parameter. When
`inner()` is called:

1. the key's `free` list is read **from the closure's own activation**:
   `free = [(h, Instance{Handler, []})]`;
2. `inner`'s walk starts with `h` in a new activation that becomes its
   `enclosing`;
3. `h.go()` looks up `h`: not a local, but ty's semantic index says it's a free
   variable of `inner`, so the lookup walks `enclosing` and finds it.

The free list is never read from the caller's env. In this fixture the caller
and the defining run are the same, so you can't tell the difference. The next
one can.

## The escaping closure

```python
def make_handler(writer):
    def handle(data):
        writer.write(data)
    return handle

def read_json():
    h = make_handler(JsonWriter())
    h(1)                         # called here, where there is no `writer`

def read_csv():
    h = make_handler(CsvWriter())
    h(2)
```

The Jedi driver gets this right: `read_json`'s `handle` reaches
`JsonWriter.write`, `read_csv`'s reaches `CsvWriter.write`. In pylspt:

1. `make_handler {writer: JsonWriter}` is walked. `def handle` binds a closure
   over that run. `return handle` exports it in the summary as
   `Closure { func: handle, free: [(writer, [Ref(Param(0))])] }`. The closure
   carries its captured values with it; the activation stays behind.
2. `read_json` resolves that against its snapshot. `Param(0)` is its
   `JsonWriter`, so `h` is a closure over a new activation holding
   `writer → that JsonWriter`.
3. `h(1)` keys `handle { data: .., free: [(writer, Instance{JsonWriter})] }`.
   Its walk finds `JsonWriter.write`.

`read_csv` does the same with its `CsvWriter`: a different key. The answer
depends only on where the closure was made, never on where it is called.

## Late binding inside one walk

The activation's slot holds the run's *current* env, updated after every
statement. A closure called inside the same walk reads it at call time:

```python
def make():
    handler = None
    def run(): handler.go()
    handler = JsonHandler()
    return run           # exported with the free values at this exit
make()()                 # JsonHandler.go
```

## A closure that captures itself

```python
def outer(root):
    def walk(n):
        walk(n.child)
    walk(root)
```

`walk`'s free variable `walk` is the closure itself. A naive fingerprint never
ends: `Closure{walk, free: [walk: Closure{walk, free: [walk: ...]}]}`. So while
fingerprinting a closure, the key builder keeps a stack of closures in progress,
and a free value equal to one on the stack becomes `Back(k)`. Then the inner
`walk(...)` keys to the same context as the outer one, which is recursion,
handled in [`08-recursion.md`](08-recursion.md).

## Lambdas

parso's `Lambda` is a `Function` subclass, and the Jedi driver's call collector
skips nested functions, so lambda bodies are probably not traversed there.
**Check `with_lambda()` against the Jedi driver, then match it.** If lambdas
are added later, a lambda is a one-expression closure, the same machinery.

## Known limit: `nonlocal`

Each exported closure carries its own copy of its free values. Two closures
that share a binding, where one rebinds it with `nonlocal`, don't see each
other's rebinding. Mutating a shared object works, because both copies point at
the same object.

---

## Exercise

1. In `outer()`, is `inner`'s node a child of `outer`'s node? Why does the
   `def` produce no edge?
2. If `inner` took `h` as a parameter (`inner(h)`), would its `free` list be
   used?
3. For `read_json()`'s `h(1)`, write `handle`'s key. Where did the `JsonWriter`
   in it come from?
4. `main` does `h = CsvHandler(); r = make(JsonHandler()); r()` where
   `make(h)` returns a closure calling `h.process()`. Which `process`?
5. Two calls in one function: `a = make_handler(JsonWriter())`,
   `b = make_handler(CsvWriter())`, then `a(1)`, `b(2)`. How many contexts of
   `handle`?

<details><summary>Answers</summary>

1. Yes: the call `inner()` is in `outer`'s body. The `def` only binds a value;
   the body is walked when it's called.
2. No. `h` would be a parameter, in the key's `args`, and found in `inner`'s
   locals before any free-variable lookup.
3. `handle { data: Data(int), free: [(writer, Instance{JsonWriter, []})] }`. The
   `JsonWriter` came from `make_handler`'s summary
   (`free: [(writer, Ref(Param(0)))]`), resolved against `read_json`'s snapshot,
   where `Param(0)` was `read_json`'s own `JsonWriter`.
4. `JsonHandler.process` only. `r`'s captured `h` came from `make`'s run, not
   from `main`'s env, even though `main` has an `h` too.
5. Two: one per captured writer. `a(1)` finds `JsonWriter.write`, `b(2)` finds
   `CsvWriter.write`.

</details>

---

→ Next: [`05-returns-and-value-flow.md`](05-returns-and-value-flow.md)
