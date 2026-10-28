# 05.01 — The graph and stable IDs

**You build:** the graph output, and identities that mean the same thing in
another request, another process, or after an edit above the function:
`StableDef` for a definition, `StableType` for a type in a key, and
`StableContextId` for a context.
**Fixture:** `../02-function-summaries/python/graph.py`
**Plan:** `plan/03-call-tree/14-output.md` § "The graph", § "Stable identity"

---

## The output

```rust
pub struct GraphOut {
    pub entries:  Vec<StableContextId>,
    pub contexts: Vec<ContextOut>,            // reachable from the entries, sorted by id
    pub edges:    Vec<EdgeOut>,               // sorted by (from, site, to)
    pub stats:    Stats,
}

pub struct ContextOut {
    pub id:        StableContextId,
    pub func:      StableDef,
    pub qname:     String,
    pub target_id: Option<String>,            // FunctionSchema/<id> or ClassSchema/<id>
    pub status:    Status,
}

pub struct EdgeOut {
    pub from: StableContextId,
    pub site: SiteId,                         // stable: a range inside the function
    pub span: Span,                           // a file range, for display
    pub to:   EdgeTo,
}

pub enum EdgeTo {
    Context(StableContextId),
    Construct { class: StableDef, init: Option<StableContextId> },
    External(StableDef),
    Unresolved,
}
```

For `graph.py` (step 02.03): 8 contexts, 9 edges (`twice` has two sites to
the same `run(a)` context). The tree has 12 nodes. On a 10 × 6 wall, 61
contexts against a million tree nodes: the graph is what scales.

## Predict

1. `ContextId` is an index in discovery order. Name two things that change it
   between two runs on the same code.
2. `Definition<'db>` is a salsa id. Why can't it be written into the response?
3. Two contexts of `emit`: one with a `JsonWriter`, one with a `CsvWriter`.
   What must their stable ids have in common, and what must differ?

---

## Build

### Decision 1: only contexts reachable through the final edges

`contexts` holds what the entries reach through the edges in the summaries,
not every context the solver ever created. Left out:

- a context that only an early fixpoint round called (step 02.06: the first
  `combine` in the parser);
- a context that was only reached on a cut path and then walked again
  elsewhere.

A recursive call is an edge back to a context already in the graph.

`External` and `Unresolved` targets are new here: the walker records a call
whose callee is not project code (`External(def)`, with ty's definition) or
has no value at all (`Unresolved`). The tree never shows them; the graph does,
so a missing edge can be told from an unresolved one.

### Decision 2: identities that outlive the request

`ContextId` is discovery order: it changes when a fixture adds a call before
another. `Definition<'db>` is valid for one salsa revision. Anything that
outlives a request needs ids that depend on neither: the v1.1 cache, comparing
two runs, any export, the UI keeping a selection.

```rust
pub enum StableDef {
    Node(String),                                         // the docstring ID
    Path { module: String, qname: String, nth: u16 },     // no ID: external code, or not assigned yet
}

pub struct StableContextId {
    pub func:   StableDef,
    pub digest: [u8; 16],                                 // 128-bit hash of the canonical key
}
```

`nth` separates two definitions with the same qualified name in one module
(`if X: def f()` / `else: def f()`), in source order.

The **canonical key** is the context's key written out with nothing
request-local in it:

- every `Definition` replaced by its `StableDef`;
- every `FpId` replaced by the fingerprint it points to, recursively, never
  the number (`Back(k)` stays `Back(k)`: it is already relative);
- names as strings, every list in its sorted order;
- `Data(Type)` as its `StableType`.

`emit(JsonWriter)` and `emit(CsvWriter)` share `func` and differ in `digest`.

**Why a 128-bit digest only here:** in memory, keys are full values compared
exactly (step 02.01). The digest is for identity outside the process, where
carrying the whole key is impractical and 128 bits make a collision
negligible.

### Decision 3: a type in a key must encode one-to-one

```rust
/// A type, by structure. Never a display string.
pub enum StableType {
    Instance { class: StableDef, args: Vec<StableType> },
    Callable { params: Option<Vec<StableType>>, ret: Box<StableType> },  // None: `...`
}
```

**`StableType` must be one-to-one.** Two different in-memory keys must never
share a `StableContextId`. So a type is encodable only if every part of it
is: `Callable[[Foo], Bar]` encodes as `Callable` with one parameter, but a
callable with keyword-only parameters, a `ParamSpec`, or any part that can't be
encoded is not encodable at all.

The rule lives in the key, not here: `fp_one` keeps `Fp::Data(t)` only when
`t` is encodable, and makes it `Fp::Unknown` otherwise, counting
`Stats.types_dropped`. That is a key-only rule, so it obeys rule 3, and no
context identity ever depends on how ty prints a type.

**Why `Callable` is in the list:** a callable value from ty is common, and its
return type carries edges:

```python
def run(make):
    make().process()
def main():
    run(registry.factory_for("json"))   # ty: Callable[[], JsonHandler] → JsonHandler.process
```

Dropping it to `Unknown` loses `JsonHandler.process`. Add more variants only
with a fixture that shows a lost edge; the benchmark's `types_dropped` says
where to look. (The plan's fixture 17 needs `factory_for` to be non-project
code, so ty answers it instead of your walk. **[check]** how to set that up
in a fixture folder: a package under an excluded directory, or a stub-only
third-party module on the search path.)

### Decision 4: sorted, never hash-map order

`contexts` sorted by id, `edges` by `(from, site, to)`. Never iterate an
`FxHashMap` to produce output: its order changes between runs, and the output
must not (step 03).

### Yours

- `src/stable.rs`: `StableDef`, `StableType`, the canonical encoding, the
  digest (any 128-bit hash with a fixed seed: SipHash-1-3 with fixed keys, or
  `xxh3_128`).
- `fp_one`: `Data` only for an encodable type; `Stats.types_dropped`.
- `src/graph.rs`: `GraphOut` from the solver, by reachability from the entries.
- `External` and `Unresolved` targets in the walker.
- `pylspt-dev calls <file> --graph` printing it.

<details><summary>Reference: this step's code</summary>

```rust
// src/graph.rs
pub fn graph(s: &Solver<'_>, entries: &[ContextId]) -> GraphOut {
    let mut seen = FxHashSet::default();
    let mut stack: Vec<ContextId> = entries.to_vec();
    let mut contexts = Vec::new();
    let mut edges = Vec::new();
    while let Some(c) = stack.pop() {
        if !seen.insert(c) { continue }
        let id = s.stable_id(c);
        contexts.push(s.context_out(c, id.clone()));
        let Some(res) = &s.slots[c].result else { continue };
        for e in &res.calls {
            for t in &e.targets {
                edges.push(EdgeOut { from: id.clone(), site: e.site, span: s.span(c, e.site), to: s.edge_to(t) });
                for next in t.contexts() { stack.push(next) }
            }
        }
    }
    contexts.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| (&a.from, a.site, &a.to).cmp(&(&b.from, b.site, &b.to)));
    GraphOut { entries: entries.iter().map(|&e| s.stable_id(e)).collect(), contexts, edges, stats: s.stats.clone() }
}
```

</details>

---

## Run

```rust
#[test]
fn p05_01_graph() {
    let g = graph_for("02-function-summaries/python/graph.py", "main");
    assert_eq!((g.contexts.len(), g.edges.len()), (8, 9));
}

#[test]
fn p05_01_stable_ids_ignore_discovery_order() {
    // main calls read_a then read_b; main2 calls them in the other order.
    // Every context's StableContextId must be the same in both runs.
}
```

---

## Where you are

The response's graph exists, with identities that survive the request. The
tree is still `to_text`'s; step 02 makes it the wire format.

---

## Exercise

1. Add a function above `run` in `graph.py`. Which parts of the output
   change: `StableDef`s, digests, `SiteId`s, `span`s?
2. A key holds `Data(dict[str, int])`. Is it encodable with the two
   `StableType` variants? What happens to it?
3. Why does `EdgeOut` carry both `site` and `span`?

<details><summary>Answers</summary>

1. Only `span`s, which are file ranges. `StableDef`s are docstring IDs or
   qualified names, digests come from canonical keys, and a `SiteId` is
   relative to its function's start.
2. Yes, as `Instance { class: dict, args: [Instance { str }, Instance { int }] }`,
   if `dict`, `str` and `int` encode as `StableDef::Path` (they have no
   docstring ID: they are external code).
3. `site` is stable across edits above the function, so it identifies the call
   site. `span` is where it is in the file right now, so an editor can jump to
   it.

</details>

---

→ Next: [`02-the-wire-tree.md`](02-the-wire-tree.md)
