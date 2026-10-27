# 03.05 — Writes anywhere, and writes deep in the caller's objects

A write is a write: in `__init__`, in another method, or from outside the class.
This file is about how each one reaches the code that reads it, including a
write several levels inside an object the caller built.

**Fixtures:** `python/external_mutation.py`, `python/deep_chain.py`,
`python/deep_write.py`. **Reading:**
[`plan/03-call-tree/07-attributes-and-self.md`](../../../plan/03-call-tree/07-attributes-and-self.md)
§ "Write: `bind_attribute`" and § "Objects the key does not reach".

---

## One write function, every receiver

```python
def bind_attribute(attr, value, env):
    receivers = eval(attr.value)        # `a.b.c = v`: the receiver is `a.b`
    targets   = the receivers that can hold state (Entry, New origins)
    if exactly one target: replace, record Must
    else:                  join with the old value, record May
    an OPAQUE or Unknown receiver: the write is dropped
```

- **A write outside `__init__`**: `def configure(self): self.handler = X()`.
  `self` is `Entry(Param(0))`, so the write is recorded, and `configure`'s
  summary has the effect `Param(0).handler = Obj(0)`. The caller applies it to
  its object. Then `p.run(1)` keys `run` with the new handler.
- **A write from outside the class**: `p.handler = JsonHandler()` in a plain
  function. `p` is the caller's own object, so the write goes straight into its
  env. Same function, same rules.
- **A write at depth**: `self.service.handler = LoudHandler()`. The receiver is
  `self.service`, evaluated by the ordinary chain rule; the write is to that
  object. No depth-specific code.

Only the last name is written; everything before it is a read. Intermediate
objects are never created: if `a.b` is `Unknown`, `a.b.c = v` is dropped (Python
raises `AttributeError` there too).

**Writes to unknown receivers are dropped on purpose.** There is no sound way to
say "some object of unknown identity changed" without invalidating everything,
and guessing would corrupt unrelated paths. `OPAQUE` counts as unknown identity,
because every ty-made instance shares it.

## Deep writes: past the key

```python
class Sink:
    def __init__(self, h):
        self.h = h

def rewire(svc):
    svc.app.config.sink.h = JsonHandler()

def main():
    svc = Service(App(Config(Sink(CsvHandler()))))
    rewire(svc)
    svc.app.config.sink.h.process()     # Python: JsonHandler.process
```

`main` built every object, so it knows the whole chain, down to `h = CsvHandler`.
`rewire`'s key stops at `fp_depth = 2`: `svc`, `svc.app`, and `svc.app.config`
as `Opaque(Config)`.

When `rewire` reads `.sink` on `svc.app.config`, the key has nothing for it. If
the walk took ty's answer as is, `.sink` would be an `OPAQUE` `Sink`, the write
would be dropped, `main` would keep `CsvHandler`, and the tree would show
`CsvHandler.process` — an edge Python never takes — with no mark.

So: reading a missing attribute on an **input object** gives an object named by
its path, `Entry(Param(0).app.config.sink)`, class `Sink` from ty. It holds
nothing the key didn't give; it only says where it lives. The write to it is
tracked, and `rewire`'s summary has:

```
effects: [Param(0).app.config.sink.h = Obj(0), strong]
```

`main` resolves the path in its own state, finds its real `Sink`, and replaces
`h`. `main` shows `JsonHandler.process`, and only that.

This keeps rule 1: `rewire` never reads `main`'s deep values. It names a
location; the caller fills in the object.

### Past `max_path`

Paths are capped (`max_path`, default 6). Past the cap, the object gets a
`Beyond` origin: the deepest path that fits, plus the next step. It holds no
state. A write to it becomes the fallback effect: a **weak `Unknown`** at that
deepest path. The summary gets the `DEEP_WRITE` gap, the node is marked
`Incomplete`, and `Stats.deep_write_fallbacks` counts it.

With `max_path = 2` in the example, `rewire`'s effect is
`Param(0).app.config.sink = [Unknown], weak`. `main`'s `sink` becomes "the old
`Sink` or `Unknown`": it still shows `CsvHandler.process`, misses
`JsonHandler.process`, but the node is marked. Incomplete, never silently wrong.

---

## Exercise

Using the fixtures:

1. `external_mutation.py`, `write_outside_init()`: write `configure`'s summary.
   What does `p.run(1)` find?
2. Same file, `write_from_outside_the_class()`: is this write an effect?
3. `deep_chain.py`, `Session.__init__` writes `self.service.handler = LoudHandler()`.
   Write `Session.__init__`'s effects. What does `Session().go()` find?
4. `deep_write.py`, `main()`: write `rewire`'s summary with the defaults. What
   does the last line find?
5. Same, with `max_path = 2`.

<details><summary>Answers</summary>

1. `effects: [Param(0).handler = [Obj(0)], strong]`, `objects: [OtherHandler]`.
   `p.run(1)` keys `run` with an `OtherHandler` and finds
   `OtherHandler.process`.
2. No: `p` is `write_from_outside_the_class`'s own object, so the write goes
   into its env directly. It would become an effect only if `p` had been passed
   in.
3. `Param(0).service = Obj(0)` and the `Service` object's attributes. In
   `__init__`'s walk, `self.service` is the `Service` it just built (a `New`
   origin), so `self.service.handler = LoudHandler()` writes to that object's
   state: `objects[0]` (the `Service`) has `handler: Obj(1)` (the
   `LoudHandler`), replacing the `Handler` that `Service.__init__` set. `go()`
   finds `LoudHandler.handle`.
4. `effects: [Param(0).app.config.sink.h = [Obj(0)], strong]`,
   `objects: [JsonHandler]`. The last line finds `JsonHandler.process` only.
5. `effects: [Param(0).app.config.sink = [Plain(Unknown)], weak]`, gap
   `DEEP_WRITE`. The last line finds `CsvHandler.process` (the old `Sink` is
   still possible), misses `JsonHandler.process`, and `rewire` is marked.

</details>

---

→ Next: [`06-classmethod-staticmethod-property.md`](06-classmethod-staticmethod-property.md)
