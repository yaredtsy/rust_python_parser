# 01 — Functions: exam

Closed book. Write your answers, then check [`answers.md`](answers.md).

```python
def emit(writer, payload):
    writer.write(payload)

def read_json(): emit(JsonWriter(), 1)
def read_csv():  emit(CsvWriter(), 2)
def entry():
    read_json()
    read_csv()
```

1. Draw `entry()`'s call tree. How many `emit` nodes, and why exactly that
   many (cite the rule, not just the answer)?

```python
DEFAULT = Handler()
def process(h=DEFAULT): h.go()
def a(): process()
def b(): process(Handler())
```

2. For `a()`, which environment is `DEFAULT` resolved against? For `b()`,
   does the default expression get evaluated at all?

```python
def f(a, *rest, **opts): a.go()
def call_a(items): f(*items)
```

3. `call_a`'s call to `f` passes a call-site splat. Which parameters bind to
   `Unknown`? Is `a.go()` still a node in the tree?

```python
def outer():
    h = Handler()
    def inner(): h.go()
    inner()
```

4. Does `h` get looked up through `Env.parent`, or does it arrive as an
   eagerly-bound argument? What would change if `inner` instead took `h` as
   a parameter, called as `inner(h)`?

```python
def make(): return JsonWriter()
def run():
    w = make()
    w.write(1)
```

5. Name the two independent traversals involved in resolving `w.write(1)`'s
   receiver, and say what would go wrong if you merged them into one pass.

```python
def on_success(r): audit.record(r)
def run(task, cb): cb(task)
def a(): run("x", on_success)
def b(): run("y", on_success)
```

6. One frame calling `run` twice with the same callback vs. two frames each
   calling it once — which of `a()`/`b()`'s shape actually demonstrates
   context sensitivity, and why does the other shape prove nothing?

```python
@retry
def fetch(): return Handler()
```

7. State the decorator rule in one sentence, and the specific risk it
   accepts in exchange for not blanking out every decorated function in a
   real project.

```python
def f(): f()
def g(): h()
def h(): g()
```

8. For each of `f` and `g`/`h`, say how many nodes appear in the tree and at
   what depth the ancestor guard fires.

9. Give one fixture — from anywhere in this folder — where `is_ancestor`
   and the return-value in-flight memo would, if swapped for each other,
   produce a *wrong* answer in each direction (one fixture per direction is
   fine).

10. `functools.partial(f, x)` and manual curry (`def make(): def inner():
    ...; return inner`) both "return a function that remembers an earlier
    argument." One resolves at full precision in this design; the other
    stays opaque. Which is which, and what's the one-sentence reason for the
    difference?

---

→ [`answers.md`](answers.md)
