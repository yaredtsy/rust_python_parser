# 02 — Classes: exam

Closed book. Write your answers, then check [`answers.md`](answers.md).

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler
    def run(self, data):
        self.handler.process(data)
```

1. `Pipeline(JsonHandler()).run(1)` — at what exact step is the fresh
   `OriginId` minted, and what would break if it were minted after
   `__init__`'s body finished walking instead of before?

```python
Greeter().greet(Payload())
Greeter.greet(Greeter(), Payload())
```

2. Write `bind_parameters`'s resulting locals map for each call. Why are
   they identical despite one call supplying a receiver and the other not?

3. State `member()`'s three-tier lookup order from memory. Which tier is
   "path-specific," and what does that phrase mean concretely?

```python
a = Pipeline(JsonHandler())
b = Pipeline(CsvHandler())
a.run(1)
b.run(2)
```

4. This fixture is explicitly the one to write first. What specific bug
   would a single-instance fixture (like `Pipeline(JsonHandler()).run(1)`
   alone) fail to catch that this one does?

```python
class Pipeline:
    def __init__(self): pass
    def configure(self): self.handler = OtherHandler()
```

5. Is a separate code path needed to record `self.handler = OtherHandler()`
   here, given it's not in `__init__`? Why or why not?

```python
p = Pipeline()
p.handler = JsonHandler()
```

6. This write happens at module scope, not inside any method. Does
   `bind_attribute` treat it differently from a `self.x = ...` write inside
   a method? What determines whether the write succeeds at all?

```python
class Service:
    @staticmethod
    def static(x): return x
    @classmethod
    def create(cls, handler): return cls(handler)
```

7. For `Service.static(1)`, what's bound to `x`? For `Service.create(Handler())`,
   what's bound to `cls`, and what does `cls(handler)` construct?

```python
class Base:
    def __init__(self): self.log = Logger()
class Child(Base):
    def __init__(self):
        super().__init__()
    def go(self):
        self.log.write("hi")
```

8. What makes `self.log` resolve correctly inside `Child.go`, given that
   `Child.__init__` never itself writes `self.log`? Name the one field that
   has to be shared for this to work.

9. Name the one file, in this actual repository (not `plan/`), that
   implements "walk the MRO for a member lookup." What does it answer, and
   what does it explicitly *not* answer?

10. `self.handler.handle` as a callback (passed into another function and
    invoked later) requires one specific design choice to survive the trip.
    Name it, and say what would go wrong without it.

---

→ [`answers.md`](answers.md)
