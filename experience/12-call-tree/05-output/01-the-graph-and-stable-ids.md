# 05.01 — The graph and stable IDs

**You build:** the graph output, and identities that mean the same thing in
another request, another process, or after an edit above the function:
`StableDef` for a definition, `StableType` for a type in a key, and
`StableContextId` for a context.
**Fixtures:** `../02-function-summaries/python/graph.py`, `python/stable_ids.py`
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
    pub qname:     String,                    // module-qualified, as `sem.qname` gives it
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
    Construct { class: StableDef, init: Option<StableContextId> },   // with 04.01
    External(StableDef),
    Unresolved,
}

/// Where a call is in its file now: one-based line and column, start and end.
pub struct Span { pub start: (u32, u32), pub end: (u32, u32) }
```

For `graph.py` (step 02.03): 8 contexts, 9 edges (`twice` has two sites to
the same `run(a)` context). The tree has 12 nodes. On a 10 × 6 wall, 61
contexts against a million tree nodes: the graph is what scales.

`EdgeTo::Construct` arrives with step 04.01's `Target::Construct`. If you
build this part before part 04, leave the arm out and add it then.

## Predict

1. `ContextId` is an index in discovery order. Name two things that change it
   between two runs on the same code.
2. `Definition<'db>` is a salsa id. Why can't it be written into the response?
3. Two contexts of `emit`: one with a `JsonWriter`, one with a `CsvWriter`.
   What must their stable ids have in common, and what must differ?

---

## Where it goes

| piece | file | why there |
|---|---|---|
| `StableDef`, `StableType` | `src/stable.rs` (new, `pub mod stable;` in `lib.rs`) | plain data with no `'db`. `sem.rs` returns them and `interp` uses them, so they sit below both |
| `sem.stable_def`, `sem.stable_type`, `sem.callee_definition`, `sem.line_col` | `src/sem.rs` | they read ty's definitions and types. Matching on ty belongs in `sem.rs` |
| `StableContextId`, the canonical key, the digest, `Solver::stable_id` | `src/interp/canon.rs` (new, `mod canon;`) | it encodes `Fp` and `Context`, which are private to `interp` |
| `GraphOut`, `ContextOut`, `EdgeOut`, `EdgeTo`, `Span`, `graph()` | `src/interp/graph.rs` (new, `mod graph;`) | it reads `Solver.slots` and `contexts`. Re-export the output types from `interp/mod.rs`, as `tree::Node` is |
| `call_graph(db, file, entry, opts)` | `src/interp/mod.rs`, next to `call_tree` and `solve` | the public entry: `solve`, then `graph`. The CLI and the server both call it |
| `Target::External`, `Target::Unresolved`, `Target::contexts` | `src/interp/summary.rs` | where `Target` lives |
| setting them | `call_values` in `src/interp/call.rs` | where every callee value is sorted |
| skipping them in the tree | `sites_union` in `src/interp/render.rs` | the tree never shows them |
| `Fp::Data` only when encodable, `Stats.types_dropped` | `fp_one` in `src/interp/key.rs`, `Stats` in `src/interp/mod.rs` | a key rule |
| `graph_for` | `src/interp/fixture.rs` | test helper |
| `calls` | `src/bin/pylspt-dev.rs` | the dev CLI |

**One dependency for you to add** (a fixed 128-bit hash):

```toml
xxhash-rust = { version = "0.8", features = ["xxh3"] }
```

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

### Decision 2: every call site gets an edge, even when nothing is walked

`External` and `Unresolved` targets are new. `call_values` sorts each callee
value:

| callee value | target |
|---|---|
| project `Function`, `Closure`, and (04.01) `BoundMethod`, `Class` | as now |
| non-project `Function`, `BoundMethod`, `Class` | `External(def)` |
| `Ty(t)` with a definition (`sem.callee_definition(t)`) | `External(def)` |
| `Ty(t)` without one, `Unknown`, `Instance`, or no callee value at all | `Unresolved` |

Push them into `edge_targets` in the same loop that sets `ask_ty`, deduplicated.
When `callees` is empty, push one `Unresolved`. A site with any target gets a
`CallEdge`, so sites that only reach ty now have edges too.

The tree never shows them. `sites_union` takes `Target::Call(ctx)` only and
skips the rest (`let Target::Call(ctx) = *t else { continue };`). Otherwise
`render.rs:60` stops compiling. The graph does show them, so a missing edge
can be told from an unresolved one.

`Target` gains a lifetime for `External(Definition<'db>)`, and so do
`CallEdge` and `Frame.calls` (04.01 needs the same for `Construct`).

`sem.callee_definition(t)` is the definition behind a callable type: through
`shape` (`Function`, `Class`, `BoundMethod`), else
`vnoc::callable_definition`. That is the same fallback `property_parts`
uses. Factor it out of `property_parts` rather than writing it twice.

### Decision 3: identities that outlive the request

`ContextId` is discovery order: it changes when a fixture adds a call before
another. `Definition<'db>` is valid for one salsa revision. Anything that
outlives a request needs ids that depend on neither: the v1.1 cache, comparing
two runs, any export, the UI keeping a selection.

```rust
// src/stable.rs
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, serde::Serialize)]
pub enum StableDef {
    Node(String),                                         // the docstring ID
    Path { module: String, qname: String, nth: u16 },     // no ID: external code, or not assigned yet
}

// src/interp/canon.rs
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, serde::Serialize)]
pub struct StableContextId {
    pub func:   StableDef,
    pub digest: [u8; 16],                                 // 128-bit hash of the canonical key
}
```

`sem.stable_def(def)`:

- **`Node(id)`** when the def is project code and its body's docstring has an
  `ID:`. Read it with `scan::id_from_body(body)`, the one reader the ids
  stage also uses, so the two never disagree on which defs have an ID. The
  body is the `def`'s or the `class`'s node body.
- **`Path`** otherwise. `module` comes from `file_to_module` (as
  `qname::qualified_name` does). `qname` is `sem.qname(def)` with the
  `module.` prefix removed. `nth` separates two definitions with the same
  `qname` in one module (`if X: def f()` / `else: def f()`), in source order.
  It is 0 for nearly every def. Compute it in one pass over the module's defs,
  grouped by qname, and cache it per file.

### Decision 4: the canonical key, and the bytes that are hashed

The **canonical key** is the context's key written out with nothing
request-local in it:

```rust
// src/interp/canon.rs
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
enum Canon {
    Unknown,
    Data(StableType),
    Function(StableDef),
    Class(StableDef),
    Module(String),                                       // the module's name
    Union(Vec<Canon>),                                    // sorted by Canon, deduplicated
    Closure { func: StableDef, free: Vec<(String, Canon)>, defaults: Vec<(String, Canon)> },
    Back(u8),                                             // already relative
}

#[derive(serde::Serialize)]
struct CanonKey { func: StableDef, args: Vec<Canon>, free: Vec<(String, Canon)>, defaults: Vec<(String, Canon)> }
```

- Every `Definition` becomes its `StableDef`, every `Fp::Data(t)` its
  `StableType`, and names become strings.
- **A `Union` is re-sorted by `Canon`.** `fingerprint` sorts union members by
  `FpId`, which is the order they were interned in this request. Keeping that
  order would give one key two digests in two runs.
- `free` is already sorted by name, and `defaults` is in parameter order.
  Both orders come from the code, so they are kept.
- Part 04 adds arms: `Instance { class, attrs }`, `Opaque(class)`,
  `Bound { func, recv }` (04.01), and `alias` in the key (04.07).

The digest is `xxh3_128` of `serde_json::to_vec(&canon_key)`, as 16
little-endian bytes. **Hash bytes you wrote, never `#[derive(Hash)]`:**
`std::hash::Hash` makes no promise to stay the same across Rust versions or
platforms, and these ids are meant to outlive the process. JSON of a struct
with no maps is fixed by the field order and the sorted lists, so it's
deterministic.

`emit(JsonWriter)` and `emit(CsvWriter)` share `func` and differ in `digest`.

**Why a 128-bit digest only here:** in memory, keys are full values compared
exactly (step 02.01). The digest is for identity outside the process, where
carrying the whole key is impractical and 128 bits make a collision
negligible.

### Decision 5: a type in a key must encode one-to-one

```rust
// src/stable.rs
/// A type, by structure. Never a display string.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, serde::Serialize)]
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

`sem.stable_type(t, file) -> Option<StableType>` returns `None` for anything
not encodable. The rule lives in the key: `fp_one`'s `Ty` arm keeps
`Fp::Data(widened)` only when `stable_type` is `Some`, and makes it
`Fp::Unknown` otherwise, counting `Stats.types_dropped`. That is a key-only
rule, so it obeys rule 3, and no context identity ever depends on how ty
prints a type. The canonical key then calls `stable_type(..).expect(..)`: a
`Data` that reached a key is encodable by construction.

**[check]** what ty gives you for the parts: a nominal instance's type
arguments (its specialization), and a callable's signature (positional
parameter types, return type, and whether it is `...`). Each needs a thin
wrapper in `types/vnoc.rs`, and the matching stays in `sem.rs`.

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

### Decision 6: sorted, never hash-map order

`contexts` sorted by id, `edges` by `(from, site, to)`. Never iterate an
`FxHashMap` to produce output: its order changes between runs, and the output
must not (step 03). A hash map used only for lookups (the id cache below) is
fine.

Sorting needs `Ord` on everything in the sort keys: `StableDef`,
`StableContextId`, `EdgeTo`, and `SiteId`, which today derives `Hash` but not
`Ord`: add `PartialOrd, Ord`. `GraphOut` is printed as JSON, so `Stats`,
`Cuts`, `Status` and `Gaps` derive `serde::Serialize` too.

### Decision 7: the dev CLI calls what the server calls

`pylspt-dev` today takes `<dir> <file> [--pretty]` and runs the scan. Make the
first argument a command:

```
pylspt-dev parse <dir> <file> [--pretty]             # what it does today
pylspt-dev calls <dir> <file> --entry <name> [--graph] [--pretty]
```

`calls` prints `to_text` of `call_tree`, or with `--graph` the JSON of
`call_graph`. It calls the same functions the server will
(`plan/04-build/00-dev-cli.md`: "the CLI must call the exact same functions
the RPC layer calls"). `--entry` is required for now: the module body is not
run (a known gap), so the plan's default of "module level" has nothing to
start from. `--project` with walk-up comes with the rest of that plan.

### Yours

- `src/stable.rs`, with `StableDef` and `StableType`, and `sem.stable_def`,
  `sem.stable_type`, `sem.callee_definition` and `sem.line_col`.
- `src/interp/canon.rs`: `Canon`, `CanonKey`, `StableContextId`,
  `Solver::stable_id`.
- `fp_one`: `Data` only for an encodable type; `Stats.types_dropped`.
- `Target::External`, `Target::Unresolved`, `Target::contexts`; setting them
  in `call_values`; skipping them in `sites_union`.
- `src/interp/graph.rs`: the output types and `graph`; `call_graph` in
  `interp/mod.rs`.
- The derives of decision 6.
- `pylspt-dev calls`.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/summary.rs
impl<'db> Target<'db> {
    /// The contexts this target leads to: what the graph walks next.
    pub fn contexts(&self) -> SmallVec<[ContextId; 1]> {
        match *self {
            Target::Call(ctx) => smallvec![ctx],
            Target::External(_) | Target::Unresolved => SmallVec::new(),
            // 04.01: Target::Construct { init, .. } => init.into_iter().collect(),
        }
    }
}
```

```rust
// src/interp/canon.rs
impl<'db> Solver<'db> {
    pub fn stable_id(&self, ctx: ContextId) -> StableContextId {
        let k = &self.contexts[ctx];
        let file = k.func.program_file(self.sem.db); // types are read in the key's file, as `fp_one` did
        let key = CanonKey {
            func: self.sem.stable_def(k.func),
            args: k.args.iter().map(|&f| self.canon(f, file)).collect(),
            free: k.free.iter().map(|(n, f)| (n.to_string(), self.canon(*f, file))).collect(),
            defaults: k.defaults.iter().map(|(n, f)| (n.to_string(), self.canon(*f, file))).collect(),
        };
        let bytes = serde_json::to_vec(&key).expect("a canonical key always serializes");
        StableContextId {
            func: key.func,
            digest: xxhash_rust::xxh3::xxh3_128(&bytes).to_le_bytes(),
        }
    }

    fn canon(&self, fp: FpId, file: ProgramFile<'db>) -> Canon {
        match &self.fps[fp] {
            Fp::Unknown => Canon::Unknown,
            Fp::Data(t) => Canon::Data(
                self.sem.stable_type(*t, file).expect("only encodable types reach a key"),
            ),
            Fp::Function(d) => Canon::Function(self.sem.stable_def(*d)),
            Fp::Class(d) => Canon::Class(self.sem.stable_def(*d)),
            Fp::Module(t) => Canon::Module(self.sem.module_name(*t)),
            Fp::Union(ids) => {
                let mut v: Vec<Canon> = ids.iter().map(|&i| self.canon(i, file)).collect();
                v.sort();
                v.dedup();
                Canon::Union(v)
            }
            Fp::Closure { func, free, defaults } => Canon::Closure {
                func: self.sem.stable_def(*func),
                free: free.iter().map(|(n, f)| (n.to_string(), self.canon(*f, file))).collect(),
                defaults: defaults.iter().map(|(n, f)| (n.to_string(), self.canon(*f, file))).collect(),
            },
            Fp::Back(k) => Canon::Back(*k),
        }
    }
}
```

```rust
// src/interp/graph.rs
pub fn graph(s: &Solver<'_>, entries: &[ContextId]) -> GraphOut {
    // 1. what the entries reach through the final edges, in no particular order
    let mut seen: FxHashSet<ContextId> = FxHashSet::default();
    let mut stack: Vec<ContextId> = entries.to_vec();
    let mut reached = Vec::new();
    while let Some(c) = stack.pop() {
        if !seen.insert(c) {
            continue;
        }
        reached.push(c);
        let Some(res) = &s.slots[c].result else { continue };
        for e in &res.calls {
            for t in &e.targets {
                stack.extend(t.contexts());
            }
        }
    }
    // 2. each reached context's stable id, once (a lookup table: never iterated for output)
    let ids: FxHashMap<ContextId, StableContextId> =
        reached.iter().map(|&c| (c, s.stable_id(c))).collect();
    // 3. the output, then sorted
    let mut contexts = Vec::new();
    let mut edges = Vec::new();
    for &c in &reached {
        let func = s.contexts[c].func;
        let res = s.slots[c].result.as_ref();
        contexts.push(ContextOut {
            id: ids[&c].clone(),
            func: s.sem.stable_def(func),
            qname: s.sem.qname(func),
            target_id: match s.sem.stable_def(func) {
                StableDef::Node(id) => Some(format!("FunctionSchema/{id}")),
                StableDef::Path { .. } => None,
            },
            status: res.map(|r| r.status).unwrap_or_default(),
        });
        for e in res.iter().flat_map(|r| &r.calls) {
            for t in &e.targets {
                let to = match t {
                    Target::Call(ctx) => EdgeTo::Context(ids[ctx].clone()),
                    Target::External(d) => EdgeTo::External(s.sem.stable_def(*d)),
                    Target::Unresolved => EdgeTo::Unresolved,
                };
                let range = s.site_range(func, e.site);
                edges.push(EdgeOut {
                    from: ids[&c].clone(),
                    site: e.site,
                    span: Span {
                        start: s.sem.line_col(func, range.start()),
                        end: s.sem.line_col(func, range.end()),
                    },
                    to,
                });
            }
        }
    }
    contexts.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| (&a.from, a.site, &a.to).cmp(&(&b.from, b.site, &b.to)));
    GraphOut {
        entries: entries.iter().map(|e| ids[e].clone()).collect(),
        contexts,
        edges,
        stats: s.stats.clone(),
    }
}
```

`sem.line_col(def, offset)` reads the def's file through `source_text` and
`line_index` (`ruff_db::source`), and returns `LineIndex::line_column` as
one-based `(line, column)`. `sem.module_name(t)` is the name of a module
literal type.

```rust
// src/interp/mod.rs
/// The context graph of `entry`: what the server's `resolve_calls` returns as `graph`.
pub fn call_graph(db: &ProjectDatabase, file: File, entry: &str, opts: &Options) -> Option<GraphOut> {
    let (s, root) = solve(db, file, entry, opts)?;
    Some(graph(&s, &[root]))
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
    // main calls read_a then read_b; main2 calls them in the other order,
    // so their ContextIds differ. Their stable ids must not.
    check_both("05-output/python/stable_ids.py");
    let ids = |entry: &str| -> BTreeMap<String, StableContextId> {
        graph_for("05-output/python/stable_ids.py", entry).contexts.into_iter()
            .filter(|c| c.qname != format!("stable_ids.{entry}"))   // the entries themselves differ
            .map(|c| (c.qname, c.id))
            .collect()
    };
    let (one, two) = (ids("main"), ids("main2"));
    assert_eq!(one.len(), 3);          // read_a, read_b, done
    assert_eq!(one, two);
}
```

`graph_for` opens the fixture the way `stats_for` does, and calls `call_graph`:

```rust
// src/interp/fixture.rs
pub fn graph_for(fixture: &str, entry: &str) -> GraphOut {
    let path = std::fs::canonicalize(format!("experience/12-call-tree/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    call_graph(&db, file, entry, &summary()).unwrap()
}
```

`GraphOut` holds no `'db` (that is the point of this step), so it can outlive
the database the helper opened. `ContextOut.qname` is module-qualified
(`stable_ids.main`), as `sem.qname` gives it. `to_text` is what drops the
module, which is why the expected trees don't show it.

`graph.py` and `stable_ids.py` call nothing outside the project, so
`External` and `Unresolved` don't change the counts.

The fixture, `python/stable_ids.py`:

```python
def done(): pass
def read_a(cb): cb()
def read_b(cb): cb()

def main():
    read_a(done)
    read_b(done)

def main2():
    read_b(done)
    read_a(done)
```

`read_a` and `read_b` take a function, so their keys hold a `Function`
fingerprint, which the canonical key writes as a `StableDef`. Without an
argument the test would only check `StableDef`, never the key encoding.

Then the rest of the suite: every tree must be unchanged. `External` and
`Unresolved` add edges to summaries, and `render` skips them.

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
4. A key holds `Union(Function(a), Function(b))`, and in another run `b` was
   fingerprinted first. What do the two `Fp::Union`s look like, and why must
   `canon` sort?

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
4. The members are sorted by `FpId`, the interning order, so one run has
   `[a, b]` and the other `[b, a]`. Serialized as they are, they hash to two
   digests for the same key. Sorting by `Canon` gives `[a, b]` in both.

</details>

---

→ Next: [`02-the-wire-tree.md`](02-the-wire-tree.md)
