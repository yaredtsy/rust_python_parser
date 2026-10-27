# 01.04 — One call: resolve, then write

What the walk does when it reaches a call to a project function. This is where
the summary meets the caller's state.

**Reading:** [`plan/03-call-tree/10-one-call.md`](../../../plan/03-call-tree/10-one-call.md).
**Fixtures:** `python/replace.py`, `python/split.py`.

---

## The steps

```
1. evaluate the callee                    (eval)
2. evaluate the arguments, left to right  (eval_arguments)
3. snapshot the env: `before`             (an O(1) clone)
4. bind the arguments to the parameters, split by argument type
5. for each combination: key → get the summary
6.   phase 1: resolve the summary's values against `before`
7.   phase 2: write: new objects' attributes, then effects
8. the call's value is the resolved returns
```

## Phase 1: resolve against the snapshot

Every `SVal` in the summary is turned back into caller values, reading
`before`:

- `Ref(path)`: start at the argument for `path.root`, follow each step through
  `before`'s attributes. "The object at this path when the call started."
- `Obj(j)`: a new origin for this call site, `New { site, t, combo, made: Obj(j) }`.
- `Plain(..)`: the matching value.

## Phase 2: write

Write the new objects' attributes, then apply each effect to every object its
target resolved to. Replace the old value only when the effect is strong, the
target is exactly one object, and this site had one callee and one combination.
Otherwise join. Every write is recorded in the caller's own `written`, so it
reaches the caller's summary too.

## Why resolve before writing

```python
class Box:
    def replace(self, h):
        old = self.h
        self.h = h
        return old
```

Summary: `returns: [Ref(Param(0).h)]`, `effects: [Param(0).h = Ref(Param(1)), strong]`.

```python
b = Box(JsonHandler())
old = b.replace(CsvHandler())
old.process()            # must be JsonHandler.process
b.h.process()            # must be CsvHandler.process
```

Phase 1 resolves `Ref(Param(0).h)` against `before`, where `b.h` is still the
`JsonHandler`. Phase 2 then writes the `CsvHandler`. Both answers come out right.

Do it the other way round and `Ref(Param(0).h)` reads the `CsvHandler` that was
just written: `old.process()` becomes `CsvHandler.process`, and
`JsonHandler.process` disappears. A swap breaks the same way:

```python
def swap(self):
    t = self.x
    self.x = self.y
    self.y = t
# effects: Param(0).x = Ref(Param(0).y), Param(0).y = Ref(Param(0).x)
```

Both effects must read the old `x` and `y`. Applying the first before resolving
the second makes both attributes the same handler.

Simple fixtures pass with this bug present. Only one that returns or writes a
value it also replaces catches it.

## Split by argument type

```python
def build(h):
    return Job(h, h.key())

def main(c):
    h = JsonHandler() if c else CsvHandler()
    build(h).go()
```

`h` has two values. `build(h)` becomes two combinations, two contexts:
`build {h: JsonHandler}` and `build {h: CsvHandler}`. Each returns its own `Job`,
pairing a `JsonHandler` with a `JsonKey`, or a `CsvHandler` with a `CsvKey`.

In `main`, each returned `Job` gets an origin that includes the combination
index `k`: `New { site, t: 0, combo: k, made: Obj(0) }`. Without `k`, both
`Job`s would get one origin, its attributes would be joined, and `go()` would see
a `JsonHandler` paired with a `CsvKey`: pairs that never existed together.

Over `split_cap` combinations, the arguments collapse into one context with a
union fingerprint, and the counter `cpa_collapses` says so.

## A call with no value

If an argument has no value at all (every path to it raised), Python never makes
the call. The Jedi driver still shows it, because it finds calls by syntax. So
pylspt records the edge, keyed with `Unknown` for the missing argument, and walks
the callee so its subtree renders. But it doesn't use the returns or effects: the
call's value is empty too.

```python
def fail(): raise ValueError()
def main(): log(fail())          # main > fail, main > log
```

---

## Exercise

1. `python/replace.py`, `main`: write each step for `b.replace(CsvHandler())`,
   then say which `process` each of the two final calls finds.
2. Same file, `swap_main`: after `p.swap()`, which handler is `p.x`? Then
   answer again as if phase 2 ran before phase 1.
3. `python/split.py`: list the contexts of `build`, of `Job.__init__` and of
   `Job.go`. Which `encode` appears under which `process`?

<details><summary>Answers</summary>

1. Callee: `BoundMethod { replace, b }`. Arguments: `[CsvHandler]`. Snapshot:
   `b.h = JsonHandler`. Key: `replace {self: Box{h: JsonHandler}, h: CsvHandler}`.
   Phase 1: returns → `b.h` in `before` → the `JsonHandler`. Phase 2: `b.h` ←
   the `CsvHandler`, replaced (strong, one target, one combo). So
   `old.process()` → `JsonHandler.process`, `b.h.process()` → `CsvHandler.process`.
2. `p.x` is the `CsvHandler` and `p.y` the `JsonHandler`. With the phases
   reversed: applying `x = Ref(y)` first makes `x` the `CsvHandler`, then
   resolving `y = Ref(x)` reads the new `x`, so both become the `CsvHandler`.
3. `build`: two contexts, one per handler class. `Job.__init__`: two, one per
   `(h, k)` pair. `Job.go`: two, one per `Job` fingerprint. `JsonHandler.process`
   has only `JsonKey.encode` under it; `CsvHandler.process` only `CsvKey.encode`.

</details>

---

→ Next: [`05-the-solver.md`](05-the-solver.md)
