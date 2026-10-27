# 03.01 — Constructing an instance

The class-side base case. Calling a class doesn't run the class body: it makes a
new object, then calls `__init__` on it as an ordinary method call. What
`__init__` writes on `self` comes back as effects on the new object.

**Fixture:** `python/construct.py`. **Reading:**
[`plan/03-call-tree/10-one-call.md`](../../../plan/03-call-tree/10-one-call.md)
§ "Constructors".

---

## The shape

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler
    def run(self, data):
        self.handler.process(data)

def main():
    Pipeline(JsonHandler()).run(1)
```

This file is the first half: `Pipeline(JsonHandler())`. The second half,
`.run(1)`, is [`02`](02-self-and-method-binding.md) and [`03`](03-attribute-state.md).

## The trace

```
eval  Pipeline(JsonHandler())                     in main's walk
  1. callee: Name "Pipeline" → ty: ClassLiteral → Class(Pipeline)
  2. arguments: JsonHandler() → (its own construction) → Instance{JsonHandler, New{site A}}
  3. before = main's env
  4. a class: construct.
     a. mint the new object: origin New{site B, t: 0, combo: 0, made: Instance}
        fingerprint: Instance{Pipeline, attrs: []}
     b. find __init__ through Pipeline's MRO (vnoc::member on the class)
     c. bind: [self ← the new object, handler ← Instance{JsonHandler}]
        key:  Pipeline.__init__ { self: Instance{Pipeline, []}, handler: Instance{JsonHandler, []} }
     d. get → walk __init__:
        │ self ← Entry(Param(0)), handler ← Entry(Param(1))
        │ self.handler = handler     → write (Entry(Param(0)), handler), Must
        │ end: effects [Param(0).handler = Ref(Param(1)), strong]
     e. phase 1: Param(0) → the new object; Ref(Param(1)) → the JsonHandler
        phase 2: attrs[(New{site B}, handler)] = the JsonHandler   (replaced)
  value: Instance{Pipeline, New{site B}}
  edge:  Construct { class: Pipeline, init: Some(__init__{..}) }
```

Steps 1–3 are the same as any call. A class only differs at step 4: the walk
makes the object, then `__init__` is an ordinary method call on it. There is no
separate "constructor interpreter", and no special store for what constructors
write: `__init__`'s write to `self.handler` is an effect on `Param(0)`, like any
method's write to its receiver.

The rendered node is `ClassSchema/<id>`, with `__init__`'s calls as its
children.

## Sharing, and identity

Every `Pipeline(JsonHandler())` in the project has the same `__init__` key, so
`__init__` is walked once. But each site mints its own origin, so two objects
built at two sites never share attributes. The summary is shared; the objects
aren't.

## No `__init__` body

A dataclass, `NamedTuple` or attrs class has a synthesised `__init__`. Then
`init: None`: the class edge is still there, nothing is walked, the object has
no attributes, and `member` falls through to ty's field types. The same if no
class in the MRO has an `__init__` body.

---

## Exercise

Using `python/construct.py`:

1. When is the new object's origin minted, and could it be minted after
   `__init__` is walked?
2. Is `Pipeline`'s class body (the `class Pipeline:` block) walked when an
   instance is built?
3. After the trace, what does `main` know about the new object's `handler`?
   What does it still not know about `run`?
4. Two `Pipeline(JsonHandler())` calls on two lines: how many `__init__` walks,
   how many origins?

<details><summary>Answers</summary>

1. Before `__init__` is keyed: the new object is `__init__`'s parameter 0, and
   its effects are resolved onto that origin. Minting it afterwards would leave
   the effect `Param(0).handler` nowhere to land.
2. No. The class statement binds a class value when it runs, the way a `def`
   binds a function. Only `__init__`'s body is walked, and only through its
   summary.
3. `attrs[(New{site B}, handler)]` is the `JsonHandler`. Nothing about `run`
   yet: `run` is keyed and walked when `.run(1)` is evaluated, with this state in
   its key.
4. One walk (one key). Two origins (two sites).

</details>

---

→ Next: [`02-self-and-method-binding.md`](02-self-and-method-binding.md)
