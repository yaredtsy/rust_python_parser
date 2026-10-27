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

4. Does `h` get looked up through `Env.enclosing`, or does it arrive as an
   eagerly-bound argument? What would change if `inner` instead took `h` as
   a parameter, called as `inner(h)`?

```python
def make(): return JsonWriter()
def run():
    w = make()
    w.write(1)
```

5. Resolving `w.write(1)`'s receiver needs `make`'s return value. Where
   does that value come from, why is it there by the time `w` is bound, and
   why isn't `make` counted twice?

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

9. Recursion is walked once. Give the fixture from this folder where that
   costs precision, say which call loses its edge, and why.

10. `functools.partial(f, x)` and manual curry (`def make(n): def inner():
    ...; return inner`) both "return a function that remembers an earlier
    argument." Under the **base** design (before `04`'s `Closure`/`ActivationId`
    fix), does either resolve precisely when called from a different frame
    than where it was created? Which one's imprecision is permanent by
    design, and which one's is a fixable gap — and why that distinction
    matters?

---

→ [`answers.md`](answers.md)
