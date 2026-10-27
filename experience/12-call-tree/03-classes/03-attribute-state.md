# 03.03 — Attribute state

A write in one method, a read in another. The machinery is all in place; this
file puts it together.

**Fixture:** `python/pipeline.py`. **Reading:**
[`plan/03-call-tree/07-attributes-and-self.md`](../../../plan/03-call-tree/07-attributes-and-self.md)
§ "Where attribute state lives" and § "Reading: `member`".

---

## The shape

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler        # written here
        self.name = "pipe"
    def run(self, data):
        self.handler.process(data)    # read here, in a different method

def main():
    Pipeline(JsonHandler()).run(1)
```

Three steps, each already covered:

1. `Pipeline(JsonHandler())`: `__init__`'s effects land on the new object in
   `main`'s env ([`01`](01-constructing-an-instance.md)).
2. `.run(1)`: `member` finds `run` on the class and binds **our** object as the
   receiver ([`02`](02-self-and-method-binding.md)). The key fingerprints the
   receiver with its attributes:
   `run { self: Instance{Pipeline, [handler: Instance{JsonHandler, []}, name: Data(str)]} }`.
3. `run`'s walk starts with `self.handler` in its env, written by materialize
   from the key. `self.handler.process(data)` reads it.

## Where state lives

Only in `Env.attrs`, keyed `(origin, name)`:

- attributes the key carried in, for input objects;
- writes made in this walk;
- effects and new objects applied from callees.

There is no store shared across walks. Each walk knows exactly what its key told
it, plus what it did.

## Reading: `member()`

```
member(Instance{class, origin}, name):
  1. Env.attrs[(origin, name)]           — this path's state. The precise answer.
  2. the class side, via vnoc::member    — methods (bound to OUR receiver),
                                            class variables, properties,
                                            declared attributes
  3. an input object's attribute the key didn't hold: an object named by its path
```

The order is the design: path-specific state beats the class, and the class
beats ty's general answer.

Tracing `self.handler.process(data)` in `run`: `self` is `Entry(Param(0))`.
`.handler` hits step 1: the key put it there. `.process` is a second `member`
call, on the handler, and hits step 2: `JsonHandler`'s class has `process`.

Step 3 is for objects deeper than the key; file 5 is about it.

## Branches

```python
def __init__(self, json):
    if json: self.h = JsonHandler()
    else:    self.h = CsvHandler()
```

Each arm starts from the state before the `if`, and the arms are joined: `self.h`
is both. The write happened on every path (both arms write), so the effect is
strong — but its value is the union. Conditions are never evaluated.

---

## Exercise

Using `python/pipeline.py`:

1. Write `__init__`'s summary. Does `self.name = "pipe"` produce an effect?
2. Write `run`'s key. Which `member` step answers `.handler`, and which
   `.process`?
3. If `JsonHandler.process` were a `@property`, which step finds it, and what
   changes?

<details><summary>Answers</summary>

1. `effects: [Param(0).handler = Ref(Param(1)), strong; Param(0).name = Plain(Ty(str)), strong]`.
   Yes: every attribute write on an input becomes an effect, whatever its value.
2. `run { self: Instance{Pipeline, [handler: Instance{JsonHandler, []}, name: Data(str)]}, data: .. }`.
   (`name` is in the fingerprint because fingerprints carry every attribute to
   `fp_depth`.) `.handler`: step 1. `.process`: step 2.
3. Still step 2: `vnoc::member` finds it on the class. What changes is that
   `member` calls the getter at once ([`06`](06-classmethod-staticmethod-property.md)),
   so `self.handler.process` would be the getter's returns, not a bound method.

</details>

---

→ Next: [`04-two-instances-and-origin-isolation.md`](04-two-instances-and-origin-isolation.md)
