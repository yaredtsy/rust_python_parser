# 02.03 — The solver and the graph

**You build:** reuse. The solver keeps one slot per context, walks each
context once, and hands its summary to every later call with the same key.
Because a reused call is not walked, the tree can't be built during the walk
any more: it is rendered from the graph of contexts at the end.
**Fixtures:** `python/graph.py`, `python/keys.py`, `../01-functions/python/wall.py`
**Plan:** `plan/03-call-tree/13-solver-and-limits.md` § "The solver",
`14-output.md` § "The tree is a view", `15` § "No-reuse mode"

---

## The fixture

```python
def run(cb): cb()
def read_a(): run(a)
def read_b(): run(b)
def twice(): run(a); run(a)
def main(): read_a(); read_b(); twice()

# expect:
# main
#   read_a
#     run
#       a
#   read_b
#     run
#       b
#   twice
#     run
#       a
#     run
#       a
```

## Predict

1. List the contexts. Which of `twice`'s two calls to `run` walk `run`, and
   which reuse a summary?
2. `twice`'s two `run` nodes both have an `a` child. In summary mode, `run`'s
   body is walked once in all of `main`. Where does the second `a` node come
   from?
3. Draw the graph: one node per context, one arrow per call edge. Is it a
   tree?

---

## Build

### Decision 1: one slot per context, one walk per key

```rust
pub struct Slot<'db> {
    pub state: State,
    pub result: Option<Arc<FunctionResult<'db>>>,
}

pub enum State {
    New,
    Running,     // its walk is in progress
    Ready,       // walked: its summary can be reused
}

pub enum Got<'db> {
    Ready(Arc<FunctionResult<'db>>),
    Recursive,   // the context is running: a recursive call. Step 05 replaces this
}

pub fn get(&mut self, ctx: ContextId) -> Got<'db> {
    match self.slots[ctx].state {
        State::Ready if self.opts.reuse => {
            self.stats.reuse_hits += 1;
            return Got::Ready(self.slots[ctx].result.clone().unwrap());
        }
        State::Running => return Got::Recursive,
        _ => {}
    }
    self.slots[ctx].state = State::Running;
    self.stack.push(ctx);
    let result = Arc::new(walk_context(self, ctx));
    self.stack.pop();
    self.slots[ctx] = Slot { state: State::Ready, result: Some(result.clone()) };
    Got::Ready(result)
}
```

`call_project` keys the call, records the edge, calls `get`, and resolves the
summary's returns. That is all summary mode does at a call.

**Why it is safe:** rules 1 and 2 from step 02. The summary depends only on
the key, and holds nothing that means something only inside the walk that made
it. So the summary of `run(a)` built under `read_a` is exactly what a walk
under `twice` would have built.

`Arc` makes handing a summary out a pointer copy.

### Decision 2: `Running` means recursion

The solver walks depth-first, on one thread. A context whose walk is in
progress is on the solver's stack, so reaching it again means the current
path called back into it: recursion. `get` answers `Recursive`, the edge is
still recorded, and the call's value is `[Unknown]`, as in inline mode. The
path guard on qualified names (`s.path`) is gone from the walker: the slot
states replace it.

Step 05 replaces `[Unknown]` with the recursive call's real value.

### Decision 3: the tree is rendered from the graph

`twice`'s second `run(a)` is not walked, so there is no walk to collect its
children. But its summary is in the slot, and the summary has `calls`: every
call site of `run`'s body, with the contexts it reached. The contexts and
their edges form a graph, and every path of the tree can be read off it.

```
graph.py, as a graph of contexts

main ──► read_a ──► run(a) ──► a
  │                   ▲
  ├────► twice ───────┘ (two sites, one context)
  │
  └────► read_b ──► run(b) ──► b
```

8 contexts, 8 walks, 2 reuses. The tree has 12 nodes: it is the graph unfolded
along every path.

```rust
// src/render.rs
/// The children of a node that stands for `ctxs`: every context of ONE function
/// reached at one site on this path. One child per (site, callee function).
pub fn render(s: &Solver<'_>, ctxs: &[ContextId], path: &mut Vec<String>) -> Vec<Node> {
    let mut out = Vec::new();
    for (site, targets) in sites_union(s, ctxs) {             // same body: the sites line up
        for (func, callee_ctxs) in group_by_function(s, &targets) {
            let q = s.sem.qname(func);
            let mut node = Node::call(q.clone(), s.site_range(func_of(ctxs), site));
            if path.contains(&q) {
                node.marks.push(Mark::Recursive);             // shown, not entered
            } else {
                path.push(q);
                node.children = render(s, &callee_ctxs, path);
                path.pop();
            }
            out.push(node);
        }
    }
    out
}
```

**The invariant:** a path in the rendered tree is a sequence of contexts, and
each callee was resolved under the context above it. Put that sentence in the
doc comment of `render`.

**Why a context set and not one context:** one call site can reach several
contexts of the same function. Not yet: in this part one site reaches each
function under one key. Part 04 splits calls by argument type, and then one
`build(h)` site reaches `build(JsonHandler)` and `build(CsvHandler)`. They are
one call in the source, so they render as one node whose children are the
union of both contexts' children.

**Why the recursion rule moved to the renderer:** the graph is finite, but a
cycle in it unfolds forever. The renderer stops at a function already on the
path, by qualified name, the same rule as inline mode and the Jedi driver. So
the two modes draw the same `[recursive]` leaves.

### Decision 4: no-reuse mode checks rule 1

```rust
pub struct Options {
    pub mode: Mode,
    pub reuse: bool,     // false: a Ready slot is walked again instead of reused
}

impl Default for Options {
    fn default() -> Self {
        Options { mode: Mode::Inline, reuse: true }
    }
}
```

Inline mode ignores `reuse`: it has no slots.

With `reuse = false`, `get` walks a `Ready` context again instead of handing
out its summary. The output must be identical, byte for byte.

**What it checks:** if a walk depends on anything its key does not hold, then
the reused answer (built under the first caller) and the recomputed answer
(built under this caller) differ, and the check fails on the day such an
input appears. **What it does not check:** that the tree is right. Both modes
can lose the same information and agree. That is what the expected trees are
for.

No-reuse costs as much as the exponential walk, so run it on fixtures, not on
big projects.

### Decision 5: `check_both` gets a third run

Step 02's `check_both` ran inline and summary. Add no-reuse:

```rust
// src/interp/fixture.rs
pub fn check_both(fixture: &str) {
    check_with(fixture, &[
        inline(),
        summary(),
        Options { reuse: false, ..summary() },   // decision 4
    ]);
}
```

`inline()`, `summary()` and `check_with` are unchanged: the
`..Options::default()` in the helpers picks up `reuse: true`.

Every test from part 01 except `p01_11_closures` now runs three times. When a
step lets the modes differ, the fixture gets a mode-specific block
(`# expect inline:` / `# expect summary:`), and the harness picks the one for
its mode. The first one arrives in step 05, with the harness change.

### Yours

- `interp/solver.rs`: `Slot`, `State`, `Got`, `get`, `walk_context`,
  `stack`, `slots`, `Stats.reuse_hits`. Remove `path` from the solver.
- `render.rs`: `render`, `sites_union`, `group_by_function`.
- `call_tree` in summary mode: key the entry (no arguments), `get` it, render
  from it. The frame no longer collects child nodes in summary mode.
- `Options.reuse` (default `true`), and the third run in `check_both`.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/solver.rs
pub fn walk_context<'db>(s: &mut Solver<'db>, ctx: ContextId) -> FunctionResult<'db> {
    let func = s.contexts[ctx].func;
    let parsed = s.sem.parsed(func);
    let node = s.sem.function_node(func, &parsed).expect("contexts are only made for defs");
    s.stats.walks += 1;
    let mut w = Walk::new(s, func);
    let env = w.materialize(s, ctx, node);
    let end = w.walk_stmts(s, &node.body, env);
    w.finish(s, end, node)
}
```

```rust
// src/interp/call.rs: summary mode with reuse
fn call_project(&mut self, s: &mut Solver<'db>, def: Definition<'db>, bound: &Bound<'db>,
                targets: &mut SmallVec<[Target<'db>; 2]>) -> Values<'db> {
    let ctx = s.key(def, bound);
    targets.push(Target::Call(ctx));            // recorded now, whatever get answers
    match s.get(ctx) {
        Got::Ready(res) => self.resolve(&res.returns),
        Got::Recursive => smallvec![AbstractValue::Unknown],
    }
}
```

```rust
// src/render.rs
/// Every site of the contexts' function, in the first context's order, with
/// the union of their targets.
fn sites_union<'db>(s: &Solver<'db>, ctxs: &[ContextId]) -> Vec<(SiteId, SmallVec<[ContextId; 2]>)> {
    let mut out: Vec<(SiteId, SmallVec<[ContextId; 2]>)> = Vec::new();
    for &c in ctxs {
        let Some(res) = &s.slots[c].result else { continue };
        for edge in &res.calls {
            let i = match out.iter().position(|(site, _)| *site == edge.site) {
                Some(i) => i,
                None => { out.push((edge.site, SmallVec::new())); out.len() - 1 }
            };
            for t in &edge.targets {
                let Target::Call(ctx) = *t;
                if !out[i].1.contains(&ctx) { out[i].1.push(ctx) }
            }
        }
    }
    out
}

/// Targets grouped by function, ordered by where each function is defined
/// (step 01.09): the same order inline mode uses.
fn group_by_function<'db>(s: &Solver<'db>, targets: &[ContextId]) -> Vec<(Definition<'db>, Vec<ContextId>)> {
    let mut out: Vec<(Definition<'db>, Vec<ContextId>)> = Vec::new();
    for &t in targets {
        let f = s.contexts[t].func;
        match out.iter_mut().find(|(g, _)| *g == f) {
            Some((_, v)) => v.push(t),
            None => out.push((f, vec![t])),
        }
    }
    out
}
```

```rust
// src/interp/mod.rs
pub fn call_tree(db: &ProjectDatabase, file: File, entry: &str, opts: &Options) -> Option<(Node, Stats)> {
    match opts.mode {
        Mode::Inline => inline_tree(db, file, entry, opts),   // step 02's call_tree, minus the Summary arm
        Mode::Summary => {
            let (mut s, root_ctx) = solve(db, file, entry, opts)?;
            let qname = s.sem.qname(s.contexts[root_ctx].func);
            let mut path = vec![qname.clone()];
            let mut root = Node::root(qname);
            root.children = render(&s, &[root_ctx], &mut path);
            s.stats.contexts = s.contexts.len() as u32;
            Some((root, s.stats))
        }
    }
}

/// Summary mode, stopped before rendering: the solver, with the entry solved.
pub fn solve<'db>(db: &'db ProjectDatabase, file: File, entry: &str, opts: &Options)
    -> Option<(Solver<'db>, ContextId)>
{
    let sem = Sem::new(db);
    let def = sem.entry(file, entry)?;
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed)?;
    let mut s = Solver::new(sem, opts.clone());
    // Called with no arguments (step 01.11). A walk of the entry's own frame
    // does the binding, exactly as step 02 did.
    let bound = Walk::new(&mut s, def).bind_parameters(&mut s, def, node, &EvaluatedArgs::default(), None);
    let root_ctx = s.key(def, &bound);
    s.get(root_ctx);
    Some((s, root_ctx))
}
```

**Why `solve` is its own function:** some tests check the solver's state, not
the tree: a function's returns (step 06), the graph (step 05.01). `call_tree`
renders and drops the solver, so those tests call `solve` and read `s`
directly.

</details>

---

## Run

```rust
#[test]
fn p02_03_solver() {
    check_both("02-function-summaries/python/graph.py");
    let st = stats_for("02-function-summaries/python/graph.py", "main", &summary());
    assert_eq!((st.walks, st.reuse_hits), (8, 2));

    let st = stats_for("02-function-summaries/python/keys.py", "main", &summary());
    assert_eq!((st.walks, st.reuse_hits), (12, 5));

    let st = stats_for("01-functions/python/wall.py", "main", &summary());
    assert_eq!(st.walks, 7);
}
```

The `keys.py` numbers are right for now. Two later steps change the key and
change them, and each says so: step 04 gives (11, 6), step 04.06 gives
(10, 6).

Then the whole suite. Every part 01 fixture except `closures.py` passes in all
three runs.

---

## Where you are

```
Solver { sem, opts, fps, contexts, slots: ContextId → Slot, stack, stats }
Slot   { state: New | Running | Ready, result }
render(contexts) → the tree, one node per (site, function) per path
```

Summary mode walks each key once. The wall from step 01.13 walks 7 bodies
instead of 15, and its 10 × 6 version 61 instead of 1,111,111. The rendered
tree is still as big as before: that is part 05's problem.

---

## Exercise

1. In `graph.py`, which call is the first to get `Got::Ready` from a slot that
   was already `Ready`?
2. Every fixture passes with reuse and without it. Does that prove rule 1
   holds? If not, what kind of fixture would catch a walk that reads something
   outside its key?
3. Why does `call_project` push the target *before* calling `get`?

<details><summary>Answers</summary>

1. `twice`'s first `run(a)`. `run(a)` was walked under `read_a`, so by the time
   `twice` runs, its slot is `Ready`.
2. No. It proves reuse changes nothing *on these inputs*. A walk that reads
   something outside its key only differs between two callers whose keys are
   equal while that extra thing differs. The catching fixture has exactly
   that: two callers, one key, different hidden input. For functions the key
   hides nothing the walker reads; part 04's objects hide attributes deeper
   than `fp_depth`, and its fixtures are built that way. The real guarantee is
   by construction: `materialize` reads only the key.
3. The edge exists whatever `get` answers: for a recursive call (step 05 makes
   that `Pending`) the callee's value is unknown, but the call itself is
   certain, and its context is already known. Recording it first means no
   answer of `get` can lose it.

</details>

---

→ Next: [`04-relevant-parameters.md`](04-relevant-parameters.md)
