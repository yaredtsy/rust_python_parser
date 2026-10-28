# 05.02 — The wire tree

**You build:** the tree v-noc stores, as JSON. It is the renderer of step
02.03 with four additions: functions without a docstring `ID:` are hidden
(with everything under them), a node budget stops the tree from exploding,
every node carries its marks and contexts, and nothing is merged.
**Fixtures:** `python/ids.py` (`ids=on`), `python/render_cut.py`
(`render_nodes = 3`)
**Plan:** `plan/03-call-tree/14-output.md` § "The tree is a view", § "`call_count`
and v-noc's merge"

---

## The fixture

```python
# ids.py, with the ID filter on
def deeper():  """ID: …"""
def hidden():  deeper()            # no ID
def helper():  """ID: …"""; hidden()
def pick():    return deeper       # no ID
def main():
    """ID: …"""
    helper()                       # helper (and nothing under it)
    hidden()                       # nothing
    pick()()                       # deeper: the value flowed through pick

# render_cut.py, with render_nodes = 3: main [cut] > a, b
```

## Predict

1. `hidden` has no ID. Should `deeper`, called inside it, appear under
   `helper`?
2. `pick` has no ID either. Why does `deeper` still appear under `main` at
   `pick()()`?
3. Two calls to `emit` from one function: how many nodes does pylspt send, and
   what does v-noc store?

---

## Build

### Decision 1: the wire node

```rust
pub struct FrameJson {
    pub target_qname: String,
    pub target_id:    String,                 // FunctionSchema/<id> or ClassSchema/<id>
    pub call_count:   u32,                    // always 0 from pylspt (decision 4)
    pub call_site:    Option<Span>,
    pub contexts:     Vec<StableContextId>,   // which graph contexts this node shows
    #[serde(skip_serializing_if = "SmallVec::is_empty")]
    pub marks:        SmallVec<[Mark; 2]>,
    pub children:     Vec<FrameJson>,
}
```

`call_site`, `contexts` and `marks` are additions to today's wire contract:
the Jedi driver's v-noc reads the old fields and ignores the new ones.
`render` builds `FrameJson` instead of `Node`; `to_text` reads either.

### Decision 2: no ID, no node, and nothing under it

A function that is not project code, or has no docstring `ID:`, is not shown,
and neither is anything under it. That matches the Jedi driver, which skipped
those calls entirely (`call_resolver.py`, step 5d). **Values still flow
through them**: the analysis walked `pick` and `hidden` as usual. Only the
view hides them, which is why `pick()()` still finds `deeper`.

The fixtures of parts 01–04 have no IDs, so the harness has run with the
filter off (`ids=off`). The request path runs with it on. `ids.py` is the
fixture for the filter itself.

**[check]** after the ids stage of the pipeline assigns IDs, functions without
one should be rare. If a real project shows many hidden subtrees, that is a
pipeline problem, not a rendering one.

### Decision 3: a node budget, and every cut is marked

`render_nodes` (100,000) caps the number of nodes rendered. When it runs out,
the node whose children were being rendered gets `Mark::Cut`: its children
are partial. There is no depth limit by default (`render_depth = None`): the
budget is the guard.

**Why a budget at all:** the analysis is linear in contexts, the tree is not.
A 10 × 6 wall is 61 contexts and a million nodes. The view has to stop
somewhere; what matters is that it says where.

`render_nodes = 3` in `render_cut.py`: `main`, `a`, `b`, and `main` is marked.

### Decision 4: one node per call site, unmerged, `call_count = 0`

**Decided (2026-10-27): pylspt sends the tree unmerged, and v-noc merges it
when saving.** Each node is one call site, one function, one path, so its
`call_count` is 0. v-noc's backend stores one node per `(parent,
target_qname)`: siblings with the same name are grouped, the group gets
`call_count = Σ call_count + k − 1`, and their children are merged the same
way. Two calls to `emit` from one function arrive as two nodes and are stored
as one with `call_count = 1`, as Jedi's tree is today.

pylspt keeps `merge_for_output` only as the spec: for the parity differ and
`pylspt-dev calls --merged`. It never runs on the response path.

**Why fixtures use two callers:** two calls to `emit` from one function are
merged into one node, so they look exactly like a path-free tool's output and
prove nothing. Calls from *different* functions stay apart after the merge.
That is why `read_json` / `read_csv` and `both` have two parents.

### Yours

- `FrameJson`; `render` producing it, with `contexts` and `target_id`.
- `Options.require_ids` (on by default, off in the harness unless a fixture
  says `ids=on`); the hidden-subtree rule.
- `render_nodes`, `render_depth`, `Mark::Cut`.
- `merge_for_output`, for `--merged` and the parity differ only.

<details><summary>Reference: this step's code</summary>

```rust
// src/render.rs: the additions to step 02.03's loop
for (func, callee_ctxs) in group_by_function(s, &targets) {
    if !s.shown(func) { continue }                     // not project code, or no ID: hidden, with its subtree
    if !budget.tick() { out.cut = true; return out }   // the parent gets Mark::Cut
    let mut node = s.frame_json(func, site, &callee_ctxs);
    // … recursion mark, children, as before …
    if kids.cut { node.marks.push(Mark::Cut) }
    if s.any_incomplete(&callee_ctxs) { node.marks.push(Mark::Incomplete) }
    out.nodes.push(node);
}
```

`s.shown(func)` is `is_project(func) && (!require_ids || id_of(func).is_some())`,
with `id_of` reading the docstring the way `src/scan.rs` already does.

</details>

---

## Run

```rust
#[test]
fn p05_02_wire_tree() {
    check_both("05-output/python/ids.py");
    check_both("05-output/python/render_cut.py");
}
```

---

## Where you are

`pylspt-dev calls <file>` can print the graph (`--graph`), the wire tree
(default), and the merged tree (`--merged`), all from one `call_tree`.

---

## Exercise

1. A class with an `ID:` whose `__init__` has none. Is the class node shown?
   Its children?
2. Why is hiding done at render time, and not by skipping the walk of
   functions without an ID?
3. With `render_nodes = 100,000` and a 10 × 6 wall, which nodes get
   `Mark::Cut`?

<details><summary>Answers</summary>

1. The class node is shown (it has an ID): it is a `Construct` node, named by
   the class. Its children are `__init__`'s calls that have IDs. `__init__`
   itself is never a node of its own.
2. Values flow through functions without IDs (`pick` returns `deeper`).
   Skipping their walks would lose every edge that depends on them, and the
   tree would differ from Jedi's, which found `deeper` there too.
3. The node whose children were being rendered when the budget ran out, and
   every ancestor whose children are therefore partial: the marks go up the
   path that was cut.

</details>

---

→ Next: [`03-the-checks.md`](03-the-checks.md)
