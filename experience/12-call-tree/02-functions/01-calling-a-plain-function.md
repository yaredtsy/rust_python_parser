# 02.01 — Calling a plain function

The base case. Everything else in this folder is a variation on this trace.
Read it slowly once.

**Fixture:** `python/basic.py`. **Reading:**
[`plan/03-call-tree/10-one-call.md`](../../../plan/03-call-tree/10-one-call.md).

---

## The shape

```python
def emit(writer, payload):
    writer.write(payload)

def main():
    emit(JsonWriter(), 1)
```

## The trace

`main` is the entry, so the solver walks it: `get(main {})`.

```
walk main
  eval  emit(JsonWriter(), 1)
  1. callee: Name "emit"
       env miss → ty: FunctionLiteral(emit) → lift → Function(emit)
  2. arguments, left to right, in main's env:
       JsonWriter()  → a class: construct (03-classes/01)
                       → Instance{JsonWriter, New{site}}
       1             → ty → Ty(Literal[1])
  3. before = main's env                         (the snapshot)
  4. bind: [writer ← Instance{JsonWriter}, payload ← Ty(Literal[1])]
     one value each, so one combination
  5. key:  emit { writer: Instance{JsonWriter, []}, payload: Data(int) }
             payload is passed on to another call, so it is relevant;
             the literal widens to its class
  6. get(emit{..}) → New → walk it:
     │ walk emit
     │   materialize: writer ← Instance{JsonWriter, Entry(Param(0))}
     │                payload ← Ty(int)
     │   eval  writer.write(payload)
     │     callee: writer → env HIT → member(.., "write")
     │             → vnoc::member → BoundMethod{JsonWriter.write, recv: Entry(Param(0))}
     │     key:  JsonWriter.write { self: Instance{JsonWriter, []}, payload: Unknown }
     │           (inside write, payload goes nowhere, so it is not in write's key)
     │     get → walk it → returns [Plain(Ty(None))], no effects
     │   edges: (site of writer.write, Call(JsonWriter.write{..}))
     │ end: returns [Plain(Ty(None))], effects [], Finished → stored Ready
  7. phase 1: returns → Ty(None).  phase 2: no effects to write.
  edges: (site of JsonWriter(), Construct{JsonWriter}), (site of emit(..), Call(emit{..}))
end main
```

Then render, from `main`'s context:

```
main
├── JsonWriter          (ClassSchema, if JsonWriter has an ID)
└── emit
    └── JsonWriter.write
```

Three things to notice:

- **`writer.write` hit the env.** `writer` was in `emit`'s starting env because
  the key carried it. That's the whole point: `emit` never asks ty what
  `writer` is.
- **`payload` is in `emit`'s key, but not in `write`'s.** `emit` passes it on
  to a call that might use it, so it counts; `write` does nothing with it, so it
  doesn't. `emit(JsonWriter(), 1)` and `emit(JsonWriter(), 2)` share a summary
  (both `int`); `emit(JsonWriter(), "x")` is a second context of `emit` that
  reuses the same `write` summary.
- **Nothing walked `emit` "under" `main`.** The walk of `emit` is just the walk
  of `emit {writer: JsonWriter}`. The tree's nesting comes from rendering.

## The cross-function form

`basic.py` also has `entry()`, which calls `read_json()` and `read_csv()`, each
calling `emit` with a different writer. Two contexts of `emit`:

```
entry
├── read_json
│   └── emit
│       └── JsonWriter.write
└── read_csv
    └── emit
        └── CsvWriter.write
```

That is the shape that only a context-sensitive analysis gets right. `main()`
calling `emit` twice with two writers would render two `emit` nodes too, but
v-noc's merge joins them into one, which looks like what a path-free tool
prints ([`01-summaries/06`](../01-summaries/06-graph-and-tree.md)).

---

## Exercise

1. Draw `main()`'s rendered tree from memory.
2. Draw `entry()`'s. How many contexts of `emit`? How many walks of `emit`'s
   body?
3. Add a third function `read_json_again()` that does what `read_json` does.
   How many contexts of `emit` now? How many walks?
4. In step 2 of the trace, which argument touches `lift`, and which never does?

<details><summary>Answers</summary>

1. `main > [JsonWriter], emit > JsonWriter.write`.
2. Two contexts, two walks, two rendered `emit` nodes.
3. Still two contexts and two walks: `read_json_again`'s call has the same key
   as `read_json`'s and reuses its summary. Three rendered `emit` nodes.
4. `1` goes through ty and `lift`. `JsonWriter()` never does: calling a class
   is the walk's own constructor, which mints a `New` origin.

</details>

---

→ Next: [`02-defaults-and-keywords.md`](02-defaults-and-keywords.md)
