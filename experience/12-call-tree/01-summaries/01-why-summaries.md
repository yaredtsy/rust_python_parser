# 01.01 — Why summaries

**Reading:** [`plan/03-call-tree/03-design.md`](../../../plan/03-call-tree/03-design.md)
§ "Why summaries" and § "The rules".

---

## The problem: the tree is exponential

The call tree v-noc shows has one node per call **per path**. If the average
function makes 3 project calls:

| depth | nodes |
|---:|---:|
| 5 | 243 |
| 10 | 59,049 |
| 15 | 14,348,907 |
| 20 | 3.5 × 10⁹ |

The Jedi driver survives because it is slow, stops early
(`per_function_execution_limit` is 6), and users learn to ask about shallow
functions. A Rust port is fast enough to reach the deep parts. Walking the tree
path by path would make the port slower than the thing it replaces, on exactly
the inputs that matter.

## The fix: one walk per distinct context

Most of those paths repeat. `log_event(name, payload)` is called from 500
places, and its body does the same thing every time. `emit(writer)` is called
from many places, but with only two kinds of writer.

So: walk each function body **once per distinct context**, and record what the
call did in a **summary**. A second call with the same context reuses the
summary. Work becomes:

> number of distinct contexts × body size

The tree is still there: it is rendered from the summaries at the end
([`06-graph-and-tree.md`](06-graph-and-tree.md)), and rendering is bounded and
cheap.

This is the standard answer in program analysis (Sharir and Pnueli, 1981).
Jarvis, Infer, Pysa and Pyright all do a version of it.

```
read_json ─► emit {writer: JsonWriter} ─► JsonWriter.write
read_csv  ─► emit {writer: CsvWriter}  ─► CsvWriter.write
main      ─► read_json, read_csv
other     ─► emit {writer: JsonWriter}       ← reused: same context as read_json's
```

## The three rules

Reuse is only safe if a summary means the same thing for every caller that gets
it. Three rules make sure of that.

**1. A summary is a function of its key.** The walk for a context starts from
what the key holds, and nothing else. If it needs something the key doesn't
hold, it gets `Unknown`, the annotation, or ty's answer — never "whatever the
first caller happened to have".

> Without it: the first caller's extra state leaks into the summary, and the
> second caller, with the same key but different state, gets the first caller's
> answer.

**2. Nothing walk-local leaves a summary.** No origin, activation, value or
deferred-operation id appears in a summary. Values are written relative to the
inputs: "the caller's parameter 0", "the object I built at this site".

> Without it: a summary mentions "origin #17", which means something only in the
> walk that made it. The next caller has a different #17.

The plan makes this a type rule: the summary's value type, `PlainVal`, has no
variant that can hold such an id, so a leak doesn't compile.

**3. Precision depends only on the key and the code. Cost limits only
truncate.** Rules that widen a value (a depth limit on fingerprints, a cap on
union size) depend only on the key and the code. Limits that depend on *how* a
context was reached (stack depth, number of contexts, the deadline) never widen.
They produce a marked, incomplete result that is never reused.

> Without it: a summary computed while the stack was nearly full is truncated,
> stored, and reused by a caller that had plenty of room. The answer depends on
> which caller came first.

---

## Exercise

```python
def log_event(name, payload):
    logger.info(name)

def emit(writer, data):
    writer.write(data)

def a(): log_event("x", 1); emit(JsonWriter(), 1)
def b(): log_event("y", 2); emit(JsonWriter(), 2)
def c(): log_event("z", 3); emit(CsvWriter(), 3)
def main(): a(); b(); c()
```

1. How many contexts of `emit` are there? Of `log_event`?
2. How many times is `emit`'s body walked? How many `emit` nodes does the
   rendered tree have?
3. Suppose `emit`'s walk could read a global "current request" object that the
   key doesn't hold. Which rule does that break, and what would go wrong?

<details><summary>Answers</summary>

1. Two of `emit`: one with a `JsonWriter`, one with a `CsvWriter` (`a` and `b`
   share one). One of `log_event`: neither parameter reaches a call, a receiver
   or a return, so neither is in the key at all
   ([`02-the-key.md`](02-the-key.md)).
2. Walked twice, once per context. Rendered three times: under `a`, `b` and
   `c`, one node per path. Rendering reads the summaries; it doesn't walk.
3. Rule 1. `emit`'s summary would depend on the request that was current when
   it was first walked, and every later caller with the same key would reuse
   that answer even if its request was different.

</details>

---

→ Next: [`02-the-key.md`](02-the-key.md)
