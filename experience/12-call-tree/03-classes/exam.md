# 03 — Classes: exam

Closed book. Write your answers, then check [`answers.md`](answers.md).

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler
    def run(self, data):
        self.handler.process(data)
```

1. `Pipeline(JsonHandler()).run(1)`: write `__init__`'s summary, then `run`'s
   key. Where does `run` get `self.handler` from?

```python
Greeter().greet(Payload())
Greeter.greet(Greeter(), Payload())
```

2. Write the bound parameter values for each. Why are they the same?

3. Write `member()`'s order. Which step is path-specific, and what does step 3
   do?

```python
a = Pipeline(JsonHandler())
b = Pipeline(CsvHandler())
a.run(1)
b.run(2)
```

4. What bug does this catch that a one-instance fixture can't? How many
   `__init__` contexts are there, and would one be wrong?

```python
class Pipeline:
    def __init__(self): pass
    def configure(self): self.handler = OtherHandler()
```

5. How does `configure`'s write reach `p.run()` in
   `p = Pipeline(); p.configure(); p.run()`?

```python
def rewire(svc):
    svc.app.config.sink.h = JsonHandler()
```

6. With `fp_depth = 2`, why isn't `svc.app.config.sink` an `OPAQUE` object
   inside `rewire`? What effect does `rewire` report? What changes past
   `max_path`?

```python
class Service:
    @staticmethod
    def static(x): return x
    @classmethod
    def create(cls, handler): return cls(handler)
```

7. `Service.static(1)`: what's bound to `x`? `Service.create(Handler())`: what's
   `cls`, and what does `cls(handler)` build?

8. `Child.__init__` calls `super().__init__()`, and `Base.__init__` writes
   `self.log`. What makes `self.log` visible in `Child.go`?

9. Name the file in this repo that answers "what is `.name` on this type". What
   does it not answer?

10. A write to an `Unknown` receiver is dropped. A write past `max_path` is
    not. Why the difference?

---

→ [`answers.md`](answers.md)
