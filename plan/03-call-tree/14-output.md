# 03.14 — Output: the graph and the tree

pylspt's answer is a graph of contexts and call edges. The per-path call tree
that v-noc stores today is rendered from it. The response carries both.

---

## Graph and tree

**Call graph** (what `ty_ide`, pyan, pycallgraph give): nodes are functions,
edges are "may call". Each function appears once. Answers "what exists?"

**Call tree** (what v-noc shows): nodes are calls on a path. A function appears
once per path that reaches it. Answers "what happens on this path?"

```
        call graph                     call tree
        ──────────                     ─────────
      main                            main
      ├──► read_json ─► emit ─► write ├── read_json
      └──► read_csv  ─┘               │   └── emit
                                      │       └── JsonWriter.write
      write appears once              └── read_csv
                                          └── emit
                                              └── CsvWriter.write
```

pylspt's graph is neither of the left-hand kind: its nodes are **contexts**, so
`emit` with a `JsonWriter` and `emit` with a `CsvWriter` are two nodes. It is
linear in the number of contexts, and every path of the tree can be read off it.

---

## The graph

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
    pub target_id: Option<String>,
    pub status:    Status,
}

pub struct EdgeOut {
    pub from: StableContextId,
    pub site: SiteId,                         // stable: range within the function
    pub span: Span,                           // file range, for display
    pub to:   EdgeTo,
}

pub enum EdgeTo {
    Context(StableContextId),
    Construct { class: StableDef, init: Option<StableContextId> },
    External(StableDef),
    Unresolved,
}
```

Only contexts reachable from the entries through the final edges are output. A
context that only an early fixpoint round called ([`12`](12-recursion.md#the-fixpoint))
or that only a cut path reached is left out. A recursive call is an edge back to
a context already on the path.

---

## Stable identity

`ContextId` is discovery order inside one request. `Definition<'db>` is valid for
one salsa revision. Anything that outlives a request needs IDs that depend on
neither: the v1.1 cache, comparing two runs, any export, and the UI keeping a
selection across runs.

```rust
pub enum StableDef {
    Node(String),                                         // docstring ID from the ids stage
    Path { module: String, qname: String, nth: u16 },     // no ID: external code, or not assigned yet
}

/// A type, by structure. Never a display string.
pub enum StableType {
    Instance { class: StableDef, args: Vec<StableType> },
    Callable { params: Option<Vec<StableType>>, ret: Box<StableType> },  // None: `...`
}

pub struct StableContextId {
    pub func:   StableDef,
    pub digest: [u8; 16],                                 // 128-bit hash of the canonical key
}
```

`nth` separates two definitions with the same qname in one module
(`if X: def f()` and `else: def f()`), in source order.

The canonical key encodes `args`, `alias` and `free`, with:

- every `Definition` replaced by its `StableDef`
- every `FpId` replaced by the fingerprint it points to, recursively, never the
  number. `Back(k)` stays `Back(k)`: it is already relative
- names as strings, every list in its sorted order
- `Data(Type)` as its `StableType`

**`StableType` must be one-to-one.** Two different in-memory keys must never
share a `StableContextId`. So a type is encodable only if every part of it is:
`Callable[[Foo], Bar]` encodes as `Callable` with one parameter, but a callable
with keyword-only parameters, a `ParamSpec`, or any part that can't be encoded
is not encodable at all. `KeyPolicy` keeps `Fp::Data` only for an encodable
type; anything else is `Fp::Unknown` in the key and counted in
`Stats.types_dropped` ([`08`](08-keys.md#fingerprinting)). That is a key-only
rule, so it obeys rule 3, and no context identity ever depends on how ty prints
a type.

`Callable` is there because a callable value from ty is common and its return
type carries edges:

```python
def run(make):                         # unannotated: the key carries the value's type
    make().process()

def main():
    run(registry.factory_for("json"))  # ty: Callable[[], JsonHandler] → JsonHandler.process
```

Dropping it to `Unknown` would lose `JsonHandler.process`. (An annotated
`make: Callable[[], JsonHandler]` doesn't need the encoding: the annotation
fallback covers it, [`08`](08-keys.md#the-annotation-fallback).) `StableType`
gains more variants only with a fixture that shows a lost edge, and S5 reports
`types_dropped` to show where to look.

The digest is only for identity outside the process. In memory, keys stay full
values and are compared exactly.

---

## The tree is a view

```rust
pub struct FrameJson {
    pub target_qname: String,
    pub target_id:    String,                 // FunctionSchema/<id> or ClassSchema/<id>
    pub call_count:   u32,                    // always 0 from pylspt (below)
    pub call_site:    Option<Span>,
    pub contexts:     Vec<StableContextId>,   // which graph contexts this node shows
    #[serde(skip_serializing_if = "SmallVec::is_empty")]
    pub marks:        SmallVec<[Mark; 2]>,
    pub children:     Vec<FrameJson>,
}

pub enum Mark {
    Recursive,     // already on this path: shown as a leaf, not entered
    Cut,           // render_nodes or render_depth ran out here: children are partial
    Incomplete,    // a summary this node shows has a gap or a cut
}
```

`call_site`, `contexts` and `marks` are additive to the current wire contract.

```rust
/// The children of a node that stands for `ctxs`: every context of ONE function
/// reached at one site on this path. One child per (site, callee function).
fn render(g: &Graph, ctxs: &[ContextId], path: &mut Vec<QName>, b: &mut RenderBudget) -> Rendered {
    let mut out = Rendered::default();
    if b.depth_reached(path.len()) { out.cut = true; return out }
    for (site, targets) in g.sites_union(ctxs) {            // same body, so sites line up
        for (func, callee_ctxs) in g.group_by_function(&targets) {   // a Construct groups by its class
            if !g.shown(func) { continue }                  // not project code, or no ID
            if !b.tick() { out.cut = true; return out }     // the parent gets Mark::Cut
            let mut node = g.frame(func, site, &callee_ctxs);
            let q = g.qname(func);
            if path.contains(&q) {
                node.marks.push(Mark::Recursive);           // shown, not entered
            } else {
                path.push(q);
                let kids = render(g, &callee_ctxs, path, b);   // for a class: its __init__ contexts
                path.pop();
                if kids.cut { node.marks.push(Mark::Cut) }
                node.children = kids.nodes;
            }
            if g.any_incomplete(&callee_ctxs) { node.marks.push(Mark::Incomplete) }
            out.nodes.push(node);
        }
    }
    out
}
```

- **Grouping by `(site, function)`.** One call site whose argument is
  `JsonWriter | CsvWriter` gives two contexts but one node, with both writes
  inside. Without the grouping, v-noc's merge would count it as two calls.
- **Classes.** A `Construct` target is a `ClassSchema/<id>` node whose children
  come from its `__init__` contexts. With `init: None` it has no children.
- **Hidden nodes.** A function that is not project code, or has no docstring
  ID, is not shown, and neither is anything under it. That matches the Jedi
  driver, which skipped those calls. Values still flow through them in the
  analysis. **[check]** parity: after the ids stage, functions with no ID
  should be rare.
- **Recursion is visible.** The ancestor rule is by qname, as
  `call_resolver.py:157`. A recursive call is a leaf marked `Recursive`, where
  the Jedi driver dropped it. The rule is output only. The analysis handles
  recursion in [`12`](12-recursion.md).
- **No depth limit by default.** `render_nodes` is the guard, and every cut is
  marked.

The invariant the tree keeps: a path in the rendered tree is a sequence of
contexts, and each callee was resolved under the context above it. Put this
sentence in a doc comment on `render`.

---

## `call_count` and v-noc's merge

**Decided (2026-10-27): pylspt sends the tree unmerged, and v-noc merges it when
saving.** Every node pylspt sends has `call_count = 0`. That is correct: each
rendered node is one call site, with one function, on one path.

v-noc's backend graph DB (not salsa) stores a node per `(parent, target_qname)`:

- siblings with the same `target_qname` are grouped, in first-seen order;
- a group of `k` nodes becomes one node with
  `call_count = (sum of their call_counts) + k − 1`;
- its children are the concatenation of the group's children, merged the same
  way, recursively.

The sum makes one rule work for both drivers during rollout: pylspt's nodes all
arrive with 0, so the result is `k − 1`; the Jedi driver's tree arrives already
merged, so every group has `k = 1` and keeps its count. Two calls to `emit` from
one function become one saved node with `call_count = 1`, as Jedi saves today.

pylspt keeps `merge_for_output` only as the spec, for the parity differ and
`pylspt-dev calls --merged`. It never runs on the response path.

```rust
fn merge_for_output(nodes: &[FrameJson]) -> Vec<FrameJson> {
    // group by target_qname in first-seen order
    // each group → FrameJson { call_count: Σ call_count + group.len() - 1,
    //                          children: merge_for_output(concat of their children), .. }
}
```

**[check]** whoever writes the v-noc side: run both copies over the parity corpus
and diff. Two implementations of one rule drift.

### The second merge

`service.py:57-70` merges the per-call-site trees into one root, matching on
`target_id`, not `target_qname`, and drops `call_count` at that boundary. That
merge moves to v-noc with the first. **[check]** whether v-noc keeps the quirk
or uses one rule everywhere; that is a UI behaviour change, so it is v-noc's
call.

### Why fixtures use two callers

Two calls to `emit` from the same function render as two nodes, which v-noc
merges into one. The merged tree looks like what a path-free tool would print:
it proves nothing. Calls from different functions stay apart after the merge.
Use the cross-function form in every fixture that tests path sensitivity.

---

## Determinism

Same input, same output, ten runs in a row. At risk from:

- **hash map iteration order.** Never iterate an `FxHashMap` to produce output.
  The graph is sorted by stable IDs, so `ContextId` order never reaches it.
- **the deadline.** A slow machine can cut where a fast one doesn't. The
  determinism check runs with the deadline off, or asserts that no
  `Cut::Deadline` happened.
- **child order.** Children are in the evaluation order of the first context's
  sites. All contexts of a function walk the same body, so their sites line up.
  **[check]** against the golden files: if they use source order, sort by
  `SiteId` here. Never change the walk order to get it.

---

→ Next: [`15-checks-and-build-order.md`](15-checks-and-build-order.md)
