# 00 — The structures, top to bottom

**Goal:** you can draw every structure the call tree analysis uses, say what
contains what, which ones live for the whole request and which only while one
body is walked, and fill them in by hand for a small program.

This folder is the anatomy. The next one, [`01-summaries/`](../01-summaries/README.md),
is the behaviour: why it is built this way, and what each step does.

---

## The whole thing, in one picture

Read it top to bottom: the outermost structure first, a single value last.

```
Solver                                   one per request. Owns everything below
├── limits, policy, stats
├── fps:      FpId → Fp                   fingerprints, interned
├── contexts: ContextId → Context         keys, interned
│     Context { func, args: [FpId], alias, free: [(Name, FpId)] }
├── slots:    ContextId → Slot            one per context
│     Slot { state, result, current, walk, lowest, joins, cut }
│     ├── state:  New | Running | Parked | Resolving | Ready | Cut
│     ├── result: FunctionResult          the summary, once the walk is done
│     │     ├── calls:   [CallEdge { site: SiteId, targets: [Target] }]
│     │     ├── returns: [SVal]
│     │     ├── effects: [Effect { target: Path, attr, value: [SVal], strong }]
│     │     ├── objects: [NewObject { class, site, attrs }]
│     │     └── status:  Status { gaps, cut }
│     └── walk:   Walk                    only while the body is being walked (or waits on a cycle)
│           ├── env: Env
│           │     ├── locals:    Name → Values
│           │     ├── attrs:     (OriginId, Name) → Values
│           │     ├── written:   (OriginId, Name) → Must | May
│           │     ├── scope
│           │     └── enclosing: ActivationId?
│           ├── origins:     OriginId → OriginKey      Entry(Path) | Beyond | New { site, t, combo, made }
│           ├── values:      ValueId → AbstractValue   interned receivers
│           ├── activations: ActivationId → Env        runs, for closures
│           ├── calls, exits, returns
│           ├── deferred:    [Deferred]                only in a recursion group
│           └── gaps
├── stack:    [ContextId]                 walks in progress
└── heads:    union-find of group heads   recursion groups

Values        = [AbstractValue]
AbstractValue = Function | Closure | Class | Instance { class, origin } | BoundMethod
              | Module | Ty | Pending | Unknown
```

And, built at the end from `contexts` and `slots`:

```
GraphOut { entries, contexts: [ContextOut], edges: [EdgeOut], stats }
FrameJson { target_qname, target_id, call_count, call_site, contexts, marks, children }
```

## Three ways to write one value

The most important thing on this page. A value is written three different ways,
depending on where it is:

| form | lives in | names a specific object? |
|---|---|---|
| `AbstractValue` | a walk | yes, by an `OriginId`, which means something only inside that walk |
| `Fp` (fingerprint) | a key | no: class and attributes only. Two equal-looking objects share a key |
| `SVal` | a summary | yes, relative to the inputs: `Ref(path)` or `Obj(j)` |

Four conversions connect them. That is what a call is
([`09`](09-the-four-conversions.md)).

---

## How this folder is organised

Each file is one level of the picture, top to bottom. Each one ends with the
running example at that level.

| | file | level |
|---|---|---|
| 1 | [`01-the-solver.md`](01-the-solver.md) | the request: what the solver owns |
| 2 | [`02-the-context.md`](02-the-context.md) | what identifies a call: `Context` and `Fp` |
| 3 | [`03-the-slot.md`](03-the-slot.md) | what the solver keeps per context, and its states |
| 4 | [`04-the-summary.md`](04-the-summary.md) | what a call did: `FunctionResult` |
| 5 | [`05-the-walk.md`](05-the-walk.md) | one body under one context: `Walk`, how it starts and ends |
| 6 | [`06-the-env.md`](06-the-env.md) | what one walk knows: `Env` |
| 7 | [`07-origins-and-ids.md`](07-origins-and-ids.md) | which object: `OriginId`, and the other walk-local ids |
| 8 | [`08-the-value.md`](08-the-value.md) | the bottom: `AbstractValue` and `Values` |
| 9 | [`09-the-four-conversions.md`](09-the-four-conversions.md) | how a value crosses a call |
| 10 | [`10-the-output.md`](10-the-output.md) | the graph and the tree built from it all |

Then [`exam.md`](exam.md).

## The running example

`python/example.py`, entry `main`:

```python
class JsonWriter:
    def write(self, data): return None

class CsvWriter:
    def write(self, data): return None

class Pipeline:
    def __init__(self, writer):
        self.writer = writer
    def run(self, data):
        self.writer.write(data)
        return self.writer

def read_json():
    p = Pipeline(JsonWriter())
    w = p.run(1)
    w.write(2)

def read_csv():
    Pipeline(CsvWriter()).run(3)

def main():
    read_json()
    read_csv()
```

Nine contexts, nine walks, one reuse. Keep it open while you read.

---

## Read first

- [`plan/03-call-tree/03-design.md`](../../../plan/03-call-tree/03-design.md): the
  design on one page
- the plan chapter each file names at its top

---

## Done when

- [ ] you can draw the picture above from memory, top to bottom
- [ ] you can say which structures live for the whole request and which only
      during one walk, and why nothing walk-local may reach a summary
- [ ] you can fill in the `fps` and `contexts` tables for `example.py`
- [ ] you can write the summary of `Pipeline.run`
- [ ] you can follow the `JsonWriter` object through all four conversions and
      say which form names it at each step

---

→ Start: [`01-the-solver.md`](01-the-solver.md)
