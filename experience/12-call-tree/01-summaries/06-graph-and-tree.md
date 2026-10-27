# 01.06 — The graph, and the tree rendered from it

**Reading:** [`plan/03-call-tree/14-output.md`](../../../plan/03-call-tree/14-output.md).

---

## The graph is the answer

When the solver is done, every context reachable from the entries has a summary,
and every summary lists its call edges. That is the graph:

```
contexts                                edges
  main                                   main      ─► read_json, read_csv
  read_json                              read_json ─► emit{JsonWriter}
  read_csv                               read_csv  ─► emit{CsvWriter}
  emit{writer: JsonWriter}               emit{JsonWriter} ─► JsonWriter.write
  emit{writer: CsvWriter}                emit{CsvWriter}  ─► CsvWriter.write
  JsonWriter.write, CsvWriter.write
```

It is linear in the number of contexts. Its nodes are contexts, not functions,
so `emit` appears twice, once per kind of writer. A recursive call is an edge
back to a context already on the path. Each context gets a **stable ID**
(its function's docstring ID plus a hash of its key), so two runs can be
compared and the cache can outlive one request.

## The tree is a view

v-noc stores and shows a tree: one node per call, per path. pylspt renders it
from the graph, starting at an entry:

```
main
├── read_json
│   └── emit
│       └── JsonWriter.write
└── read_csv
    └── emit
        └── CsvWriter.write
```

The rules:

- **One node per `(call site, function)` on each path.** A call site whose
  argument may be a `JsonWriter` or a `CsvWriter` has two contexts but renders
  as one `emit` node, with both writes under it.
- **Recursion is a marked leaf.** If the callee's qname is already on the path,
  the node is shown with `Mark::Recursive` and not entered. (The Jedi driver
  dropped it.)
- **Hidden nodes.** A callee that isn't project code, or has no docstring ID, is
  not shown, and neither is anything under it, as in the Jedi driver. Values
  still flow through it in the analysis.
- **Marks.** `Incomplete` on a node whose summary has a gap or a cut. `Cut` on a
  node whose children were cut short by `render_nodes`.
- **No depth limit.** `render_nodes` (100,000) is the only guard, and every cut
  is marked.

## `call_count` and v-noc's merge

Every node pylspt sends has `call_count = 0`, because each one is one call site
on one path. v-noc merges when it saves the tree: siblings with the same qname
become one node with `call_count = Σ counts + k − 1`, and their children are
merged the same way.

```python
def main():
    emit(JsonWriter(), 1)
    emit(CsvWriter(), 2)
```

```
sent by pylspt                   saved by v-noc
main                             main
├── emit                         └── emit  (call_count = 1)
│   └── JsonWriter.write             ├── JsonWriter.write
└── emit                             └── CsvWriter.write
    └── CsvWriter.write
```

The saved tree is what the Jedi driver produces. It also looks like what a
path-free tool would print, which is why every fixture that tests path
sensitivity calls `emit` from **different** functions: those stay apart after
the merge.

## Determinism

Same input, same output, ten times in a row. Output is sorted by stable IDs, so
the order contexts were discovered in never leaks out. The deadline is off for
the check, because a slow machine can cut where a fast one doesn't.

---

## Exercise

```python
def a(): pass
def b(): pass
def c(): pass

def main():
    a()
    b()
    c()
```

1. Render `main`'s tree with `render_nodes = 3`, counting `main` itself.
2. In `../02-functions/python/recursion.py`, `use_direct()` calls
   `f()`, which calls `f()`. Draw the graph and the rendered tree.
3. A function with no docstring ID is called from `main`, and it calls
   `JsonWriter.write`. Is `JsonWriter.write` in the tree under `main`?

<details><summary>Answers</summary>

1. `main` with children `a` and `b`; `main` is marked `Cut`. (`main`, `a`, `b`
   use up the 3 nodes.)
2. Graph: `use_direct ─► f`, and `f ─► f`, a self-edge to the same context. Tree:
   `use_direct > f > f [Recursive]`. `f`'s body is walked once.
3. No. The hidden function's subtree is hidden with it, as in the Jedi driver.
   But if the hidden function *returned* something, that value still flows to
   `main`.

</details>

---

With this folder done, the two case-by-case folders read as applications of it.
Do the exam first.

→ [`exam.md`](exam.md), then [`../02-functions/README.md`](../02-functions/README.md)
