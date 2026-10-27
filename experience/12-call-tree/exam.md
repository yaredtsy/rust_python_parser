# 12 — The call tree analysis: whole-exercise exam

Closed book, across all four folders. Several questions need a rule from one
folder combined with a rule from another.

---

```python
def emit(writer, data): writer.write(data)
def read_json(): emit(JsonWriter(), 1)
def read_csv():  emit(CsvWriter(), 2)
def main(): read_json(); read_csv()
```

1. Give the contexts, the graph, and the rendered tree. Then do the same if
   `main` called `emit` directly twice, and say what v-noc saves in each case.

```python
def outer():
    w = JsonWriter()
    middle(w)
def middle(x): inner(x)
def inner(y): y.write(1)
```

2. `y` is bound three calls away from where the `JsonWriter` was built. Explain
   why it resolves, using keys, without any lookup back up the call chain.

```python
class Pipeline:
    def __init__(self, handler): self.handler = handler
    def run(self, data): self.handler.process(data)

def build(kind):
    return Pipeline(JsonHandler()) if kind == "json" else Pipeline(CsvHandler())

def use(kind):
    p = build(kind)
    p.run(1)
```

3. Write `build`'s summary. What is `p` after `use("json")`? How many contexts
   of `run`, and which `process` edges?

```python
def on_done(result): logger.info(result)

class Service:
    def __init__(self, cb): self.cb = cb
    def finish(self, result): self.cb(result)

def main():
    Service(on_done).finish(42)
```

4. Trace it: where does `self.cb` come from inside `finish`, and which `member`
   step answers it?

5. `def process(h: Handler): h.handle(1)` is an entry. What does `h` start as,
   and why can that never override a value a caller passed?

```python
class Box:
    def replace(self, h):
        old = self.h
        self.h = h
        return old
```

6. `old = b.replace(New())` then `old.process()`. Which `process`? What breaks
   if effects are applied before returns are resolved?

```python
def f(): return g()
def g(): return f()
def h(): h()
```

7. How many walks of each body? What does `x = f()` give? What does the tree
   show for `h`?

8. A parser's `combine` is first reached during its cycle's fixpoint, and calls
   back into the cycle. What happens to it, and why is the final answer the same
   whichever function was the entry?

9. `def helper(a, b): logger.info(a); validate(b)` vs
   `def helper(a, b): x = a; x.write(b)`. For each, which parameters are in the
   key? Why does the second need assignments tracked to get right?

10. Give one limit that only truncates, one rule that widens, and say why only
    the second kind of result may be reused.

---

→ [`answers.md`](answers.md)
