# 12 — Call tree resolver: whole-exercise exam

Closed book, covering all three folders. Do this after finishing
`00-values/`, `01-functions/` and `02-classes/` individually — this exam is
deliberately cross-cutting and several questions require combining a rule
from one folder with a rule from another.

---

```python
def emit(writer, data): writer.write(data)
def read_json(): emit(JsonWriter(), data)
def read_csv():  emit(CsvWriter(), data)
def main(): read_json(); read_csv()
```

1. Draw the full tree. Then redraw it as if `main()` called `emit` directly
   twice (once per writer) instead of through `read_json`/`read_csv`. Name
   the one rule responsible for the difference between the two drawings.

```python
def outer():
    w = JsonWriter()
    middle(w)
def middle(x): inner(x)
def inner(y): y.write(1)
```

2. `y` is bound three parameter-hops away from where `JsonWriter()` was
   actually constructed. Explain, citing the specific mechanism, why this
   resolves correctly despite the interpreter being eager (evaluating
   arguments once, at each call site) rather than lazy like Jedi.

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

3. This combines a `01-functions/05` mechanism with a `02-classes/01` and
   `/03` mechanism. Name both, and say what `p` resolves to after
   `use("json")` is called — one value or a union? Justify from the
   "no branch analysis" rule, noting that `kind == "json"` is a condition
   that's never evaluated.

```python
def on_done(result): logger.info(result)

class Service:
    def __init__(self, cb): self.cb = cb
    def finish(self, result): self.cb(result)

def main():
    Service(on_done).finish(42)
```

4. A callback stored as instance state, then invoked later. Which file in
   `01-functions/` names this exact shape as "the fixture that ties
   callbacks to attribute state"? Trace it: what does `self.cb` resolve to
   inside `finish`, and through which tier of `member()`?

```python
def process(h: Handler):
    h.handle(1)

def run_unresolved(opaque):
    process(opaque)      # `opaque`'s own value can't be determined
```

5. What does `h` resolve to inside `process`, on this call path? Name the
   resolution tier responsible, and the one thing that must be true about
   the ordering of tiers for this to be safe.

```python
a = Pipeline(JsonHandler())
b = Pipeline(CsvHandler())
a.run(1); b.run(2)
```

6. Which `00-values/` concept is this fixture a test of? What specific
   allocation bug would make `a.run(1)` and `b.run(2)` produce the *same*
   leaf?

```python
def f(): return g()
def g(): return f()
def h(): h()
```

7. `f`/`g` and `h` both "don't terminate" in ordinary Python semantics (one
   recurses through return values, one recurses through direct calls). Name
   the two *different* guard mechanisms involved, and say which one would
   give the wrong answer if applied to the other's case.

```python
def helper(a, b):
    logger.info(a)
    validate(b)

def path_one(): helper(X(), Y())
def path_two(): helper(Z(), W())
```

8. Is `helper` a candidate for memoisation? If `helper` were instead
   `def helper(a, b): a.write(b)`, would your answer change, and why does
   that change specifically require local-assignment tracking to detect
   correctly (not just a parameter-name scan)?

9. Four decorators or binding forms appear across this exercise:
   `@staticmethod`, `@classmethod`, `@property`, and a plain decorator like
   `@retry` wrapping a regular function. For each, state in one phrase
   what happens to receiver binding or callee resolution.

10. Name the one file in this actual repository (not `plan/`, not an
    exercise fixture) that already implements MRO-aware member lookup, and
    the one test in this repository that already proves its answer depends
    on which receiver you pass it.

---

→ [`answers.md`](answers.md)
