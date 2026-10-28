# 00.10 — The output: the graph and the tree

When the request's walks are done, the solver's `contexts` and `slots` hold
everything. The answer is built from them: a graph of contexts, and the per-path
tree v-noc stores today, rendered from the graph.

**Reading:** [`plan/03-call-tree/14-output.md`](../../../plan/03-call-tree/14-output.md).

---

## The structures

```rust
pub struct GraphOut {
    pub entries:  Vec<StableContextId>,
    pub contexts: Vec<ContextOut>,            // only those reachable from the entries
    pub edges:    Vec<EdgeOut>,
    pub stats:    Stats,
}
pub struct ContextOut { pub id: StableContextId, pub func: StableDef, pub qname: String,
                        pub target_id: Option<String>, pub status: Status }
pub struct EdgeOut    { pub from: StableContextId, pub site: SiteId, pub span: Span, pub to: EdgeTo }
pub enum EdgeTo       { Context(StableContextId), Construct { class: StableDef, init: Option<StableContextId> },
                        External(StableDef), Unresolved }

pub struct StableContextId { pub func: StableDef, pub digest: [u8; 16] }   // the function + a hash of its key
pub enum StableDef { Node(String) /* docstring ID */, Path { module, qname, nth } }

pub struct FrameJson {
    pub target_qname: String,
    pub target_id:    String,                 // FunctionSchema/<id> or ClassSchema/<id>
    pub call_count:   u32,                    // always 0 from pylspt; v-noc fills it when merging
    pub call_site:    Option<Span>,
    pub contexts:     Vec<StableContextId>,   // which graph contexts this node shows
    pub marks:        SmallVec<[Mark; 2]>,    // Recursive | Cut | Incomplete
    pub children:     Vec<FrameJson>,
}
```

**Why stable IDs.** `ContextId` and `Definition` are only valid inside one
request. A `StableContextId` is the function's docstring ID plus a 128-bit hash
of its key, written with stable names instead of ids, so two runs, a cache, or
the UI can refer to the same context.

**From slots to graph.** Each `Ready` slot gives one `ContextOut`. Each
`CallEdge` in its summary gives one `EdgeOut` per target.

**From graph to tree.** Start at an entry. For each call site of its context, in
evaluation order, group the targets by function: one child node per
`(call site, function)`. Recurse into the callee's contexts. A function already
on the path is shown as a leaf marked `Recursive`. A callee that isn't project
code, or has no docstring ID, is not shown, nor anything under it.

---

## The running example, at this level

**Graph** (9 contexts, 11 edges):

```
main ─────────► read_json, read_csv
read_json ────► Construct JsonWriter (init: none)
              ► Construct Pipeline   (init: __init__{JsonWriter})
              ► Pipeline.run{writer: JsonWriter}
              ► JsonWriter.write
Pipeline.run{writer: JsonWriter} ► JsonWriter.write
read_csv ─────► Construct CsvWriter (init: none)
              ► Construct Pipeline  (init: __init__{CsvWriter})
              ► Pipeline.run{writer: CsvWriter}
Pipeline.run{writer: CsvWriter}  ► CsvWriter.write
```

`JsonWriter.write` is one context with two edges into it: from `run` and from
`read_json`'s `w.write(2)`.

**Tree** (from `main`):

```
main
├── read_json
│   ├── JsonWriter              ClassSchema, no children
│   ├── Pipeline                ClassSchema, __init__ calls nothing
│   ├── Pipeline.run
│   │   └── JsonWriter.write
│   └── JsonWriter.write        the reused summary, shown again on this path
└── read_csv
    ├── CsvWriter
    ├── Pipeline
    └── Pipeline.run
        └── CsvWriter.write
```

Every `call_count` is 0. v-noc merges siblings with the same qname when it saves
(none here), with `call_count = Σ + k − 1`.

---

## Exercise

1. How many `FrameJson` nodes does the tree have? How many walks did the
   analysis do?
2. If `main` called `read_json()` twice, what changes in the graph, the tree,
   and the tree v-noc saves?
3. Which `StableContextId`s would change if you edited the body of `run`? Which
   if you renamed `data` to `payload`?

<details><summary>Answers</summary>

1. 12 nodes (including `main`), from 9 walks. The tree repeats what the graph
   shares: `JsonWriter.write` appears twice, once per path.
2. Graph: one more edge from `main` to `read_json` (a second site), no new
   contexts. Tree: two `read_json` subtrees under `main`. Saved: v-noc merges
   them into one `read_json` node with `call_count = 1`.
3. Editing the body: usually none, because the digest hashes the key (the
   function and its argument fingerprints), not the code. The exception is an
   edit that changes which parameters matter (`relevant_params`): if `run`
   stopped passing `data` on, `data` would become `Unknown` in the key and the
   digest would change. The summaries change either way. Renaming a parameter:
   none, because arguments are keyed by position.

</details>

---

You've seen every structure, top to bottom, and how a value crosses a call.
Do the exam, then start building: the steps explain *why* each of these is
built this way, at the moment you write it.

→ [`exam.md`](exam.md), then [`../01-functions/README.md`](../01-functions/README.md)
