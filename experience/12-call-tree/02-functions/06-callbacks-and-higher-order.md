# 02.06 — Callbacks and higher-order functions

A callback is a parameter whose value happens to be callable. If files 1 and 5
are right, this one needs no new code.

**Fixture:** `python/callbacks.py`. **Reading:**
[`plan/03-call-tree/11-closures-and-callbacks.md`](../../../plan/03-call-tree/11-closures-and-callbacks.md)
§ "Callbacks".

---

## The trace

```python
def on_success(result): audit.record(result)
def on_failure(err):    alert.page(err)
def run(task, cb):      cb(task)

def handle_ok():  run("ok-task", on_success)
def handle_err(): run("err-task", on_failure)
```

1. `on_success` is a module-level name: ty's `FunctionLiteral`, lifted to
   `Function(on_success)`.
2. `run("ok-task", on_success)` keys `run { task: Data(str), cb: Function(on_success) }`.
   `run("err-task", on_failure)` keys `cb: Function(on_failure)`: **a different
   context**.
3. In each `run` walk, `cb` is in the starting env. `cb(task)` is a call on a
   value, like any other.

```
main
├── handle_ok
│   └── run
│       └── on_success
│           └── Audit.record
└── handle_err
    └── run
        └── on_failure
            └── Alert.page
```

Two contexts of `run`, each walked once, each with one leaf.

## Dispatch on the value, never the syntax

The call never asks "is the callee a name, an attribute, or a parameter?" — only
"what value is here, and can it be called?". Higher-order code is first-order
code where the value arrived through a parameter. Guard this in review: a quick
`if let Expr::Name(..) = call.func` special case is how the generality gets
lost.

## Through two levels

```python
def through_two_levels(cb): middle(cb)
def middle(cb):             cb("value")
def call_through_levels():  through_two_levels(on_success)
```

`through_two_levels { cb: Function(on_success) }` calls
`middle { cb: Function(on_success) }`, whose walk calls `on_success`. Each
level's key carries the callback. No lookup ever goes back up a call chain.

## Bound methods as callbacks

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
`Param(0) + [Recv]`. `store`'s write to its `self` becomes `run`'s effect
`Param(0).Recv.h = Obj(0)`. `main` follows `Recv` from its bound-method value to
`s` and writes `h`. Then `s.flush()` keys `flush` with `s.h` in it.

## Not modelled, on purpose

- **Functions in containers**: `HANDLERS[kind](data)` is `Unknown`.
- **Stdlib higher-order functions**: `map(f, xs)`, `sorted(key=f)`,
  `functools.partial(f, x)`. The callee is stdlib, so `f` is never called in
  the tree. That's the Jedi driver's behaviour too.

---

## Exercise

Using `python/callbacks.py`:

1. Draw `main()`'s tree. How many contexts of `run`?
2. If `handle_err` also passed `on_success`, how many contexts of `run`?
   How many rendered `run` nodes?
3. In `call_through_levels`, where is `cb` looked up when `cb("value")` runs?
4. In the `Sink` example, write `run`'s summary.

<details><summary>Answers</summary>

1. As above. Two contexts.
2. One context (both keys equal), walked once, reused. Still two rendered `run`
   nodes, one under each handler.
3. In `middle`'s own env: the key put it there when `middle`'s walk started. It
   is the same lookup as any local.
4. `returns: [Plain(Ty(None))]`, `effects: [Param(0).Recv.h = [Obj(0)], strong]`,
   `objects: [JsonHandler]`.

</details>

---

→ Next: [`07-curry-partial-and-decorators.md`](07-curry-partial-and-decorators.md)
