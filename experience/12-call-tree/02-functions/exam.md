# 02 — Functions: exam

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

1. How many contexts of `emit`? How many walks of its body? How many rendered
   `emit` nodes? Write one `emit` key.

```python
DEFAULT = Handler()
def process(h=DEFAULT): h.go()
def a(): process()
def b(): process(Handler())
```

2. Where does `h`'s value come from for `a()`? Does binding ever evaluate a call
   in a default? Why not?

```python
def f(a, *rest, **opts): a.go()
def call_a(items): f(*items)
```

3. Which parameters are `Unknown` in `f`'s key? Does `a.go()` produce a node?

```python
def make_handler(writer):
    def handle(data): writer.write(data)
    return handle
def read_json(): make_handler(JsonWriter())(1)
```

4. Write `make_handler`'s summary. Where does `handle`'s `writer` come from when
   it is called in `read_json`?

```python
def make(): return JsonWriter()
def run():
    w = make()
    w.write(1)
```

5. Where does `w`'s value come from, and why isn't `make` walked twice if ten
   functions call it?

```python
def on_success(r): audit.record(r)
def run(task, cb): cb(task)
def a(): run("x", on_success)
def b(): run("y", on_success)
```

6. How many contexts of `run`? How many rendered `run` nodes?

```python
@retry
def fetch(): return Handler()
```

7. State the decorator rule, and the risk it accepts.

```python
def f(): f()
def g(): h()
def h(): g()
```

8. Draw the rendered trees for `main → f` and `main → g`. How many walks of
   each body? How does this differ from the Jedi driver's output?

9. In the parser (`parse_expr`, `parse_atom`, `combine`), `combine` is first
   walked during the fixpoint. Why does it join the group instead of being
   walked again in every round?

10. Why does the a/b/c chain need repeated rounds, and why does that not mean
    walking any body twice?

---

→ [`answers.md`](answers.md)
