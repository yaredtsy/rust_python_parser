# 03.09 — Path identity

> *"it's not a call graph, it's like a tree, and from every point, I create a
> path for every function — unique path"*

This chapter pins down what "unique path" means operationally, because it is the
property that distinguishes your tool from every off-the-shelf call hierarchy,
and it is the thing most likely to get quietly lost during the port.

---

## Graph vs tree

**Call graph** (what `ty_ide`, pyan, pycallgraph give you):
nodes = functions, edges = "may call". Each function appears **once**.
Size O(functions). Answers *"what exists?"*

**Your call tree:** nodes = *activations*. A function appears once **per path
that reaches it**. Size potentially exponential. Answers *"what happens?"*

```
        graph                          tree
        ─────                          ────
      main                            main
      ├──► emit ──► write             ├── emit
      │       ▲                       │   └── JsonWriter.write
      └──► log ┘                      ├── emit
                                      │   └── CsvWriter.write
      write appears once              └── log
                                          └── JsonWriter.write

                                      write appears 3×, and the
                                      first two are DIFFERENT write
```

Same source, different question, different answer.

---

## What identifies a node

In the current implementation, a node's identity is **its position in the tree**
— literally the chain of `parent` pointers. `CallFrameStack` has
`target_qname` + `target_id` but nothing distinguishing two activations of the
same function; they are distinct because they are different objects at different
tree positions.

That's fine on the wire (the JSON nesting encodes it) but weak in memory, where
you need identity for the cycle guard, memoisation, and dedup. Make it explicit:

```rust
/// The call string: the chain of definitions from root to here.
/// This IS the "unique path".
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PathKey(SmallVec<[DefId; 16]>);

impl PathKey {
    fn push(&self, def: DefId) -> PathKey { /* ... */ }
    fn contains(&self, def: DefId) -> bool { /* the is_ancestor guard */ }
    fn depth(&self) -> usize { self.0.len() }
}
```

This is **call-string sensitivity** in the program-analysis literature — the
classic *k*-CFA family, with `k = MAX_DEPTH` (i.e. unbounded until the cap).
Useful to know: it means the termination problem you have is a known one with
known mitigations, and [`08`](08-termination-and-cycles.md)'s memoisation is the
standard "summary-based" answer to it.

`PathKey` gives you, in one type:
- the ancestor guard (`contains`),
- the depth cap (`depth`),
- the memoisation key (when a subtree *is* path-dependent, key on the relevant
  suffix rather than the whole path),
- and a stable node identifier for diffing two runs.

---

## Two calls to the same function from the same frame

In the Jedi driver this is `add_child` **[verified, call_resolver.py:37-45]**,
which merges *while walking*. `pylspt` produces the same saved tree with
`merge_for_output` instead, after the walk ([§ Where the rule applies](#-where-the-rule-applies-on-output-never-during-analysis)):

```python
def add_child(self, child):
    for existing in self.children:
        if existing.target_qname == child.target_qname:
            existing.call_count += 1        # ← merge
            return existing
    self.children.append(child); child.parent = self; return child
```

So `f(); f()` from one frame → **one** saved child with `call_count == 1`.

⚠ Two consequences to preserve exactly:

1. **`call_count` starts at 0.** It counts *additional* calls beyond the first.
   One call → 0. Three calls → 2. Counter-intuitive; match it.

2. **The merge is by `target_qname` only — arguments are ignored.**
   ```python
   emit(JsonWriter())
   emit(CsvWriter())      # same frame, same qname → MERGED
   ```
   Both merge into one `emit` child, and — critically — `_analyze_function` is
   then called **twice on the same merged frame**, once per argument set. The
   two argument sets' results accumulate as *siblings inside one `emit` node*:

   ```
   main
   └── emit  (call_count=1)
       ├── JsonWriter.write
       └── CsvWriter.write
   ```

   **Not** two separate `emit` subtrees — in the saved tree. The analyser
   holds two until output.

---

## The merge rule

**Confirmed by the author.** There is no ambiguity here — settle it once:

> **Frame identity in the saved tree is `(parent frame, target_qname)`.**
> The analyser keeps one node per call and merges only on output — see
> below.
>
> Two calls to the same function *from the same frame* produce **one** node.
> The same function called from **different** frames produces **different**
> nodes.

So "one unique path per function" means *one node per (path, function)*, not
*one node per call site*. Two call sites on the same path are the same path.

### ★ Where the rule applies: on output, never during analysis

The merge is a rule about **what is saved to the graph DB** (and what the
UI shows). The analyser never merges. While walking, every call that is
walked gets its **own** node — one per run — and the `(parent, qname)`
merge runs once, when the finished tree is turned into the JSON the DB
stores.

```python
a = make_handler(JsonWriter())
b = make_handler(CsvWriter())
a.handle(1)
b.handle(2)
```

```
analysis tree (in memory)              saved to the DB (merged)
─────────────────────────              ────────────────────────
main                                   main
├── make_handler   ← run 1             ├── make_handler (call_count=1)
├── make_handler   ← run 2             └── handle       (call_count=1)
├── handle         ← run 1's closure       ├── JsonWriter.write
│   └── JsonWriter.write                   └── CsvWriter.write
└── handle         ← run 2's closure
    └── CsvWriter.write
```

The saved tree is exactly what the Jedi driver produces today. The analysis
tree keeps the two runs apart until then, so nothing in the walk (values,
`is_ancestor`, reuse) ever sees a node that mixes two runs.

**1. `add_child` always appends.** It never returns an existing child.
Each node records its `target_qname`, `target_id` and the call-site
position.

```rust
let child = frame.add_child(qname, id, call.range());   // always a NEW node
let callee_env = self.bind_parameters(func, &args, receiver, enclosing, env);
self.walk_function(func, &callee_env, Mode::tree(child));
```

**2. The output merge reproduces `add_child`'s numbers exactly.** Group
siblings by `target_qname`, keeping first-seen order; a group of `k` nodes
becomes one node with `call_count = k - 1`; its children are the
concatenation of the group's children, merged the same way, recursively.

That matches the Jedi driver's in-walk merge, where the second walk goes
*into* the existing node: there, each repeat bumps `call_count` by one and
the repeat's children are added to the same node (bumping their own counts
in turn). Merging afterwards gives the same tree, the same counts and the
same child order (first walk's children first, new ones appended). Write
the fixture both ways — this is a parity property, not a guess.

```rust
fn merge_for_output(nodes: &[FrameId], arena: &FrameArena) -> Vec<FrameJson> {
    // group by target_qname in first-seen order
    // each group → FrameJson { call_count: group.len() - 1,
    //                          children: merge_for_output(concat of their children) }
}
```

**3. `is_ancestor` is unchanged.** It walks the parent chain by qname, and
the chain has the same qnames whether or not siblings are merged.

**4. `PathKey` is not the frame identity.** It is only used for:
- the ancestor cycle guard (`contains`)
- the depth cap (`depth`)
- memo keys where a suffix of the path matters

**5. Size: the analysis tree is the work; the saved tree is small.**

The analysis tree has one node per walk, so it's as big as the work done —
and work is already bounded by the node budget
([`08`](08-termination-and-cycles.md), which ticks once per walk). The
saved tree is bounded by the number of *distinct qnames* per frame, as
before.

Calling `emit` 50 times from one frame still walks `emit`'s body 50 times.
Two fixes, both of which **reuse** a subtree rather than skipping the call
— a skipped call would drop a node, and the merged `call_count` would come
out one too low:

```rust
// (a) same callee, same entry state, same parent → graft the subtree the
//     first walk built (Arc-shared, one node's worth of memory), plus its
//     stored return value (10 § The store)
if let Some(prev) = self.walked.get(&(frame.id, ReturnKey::of(func, &callee_env))) {
    frame.graft(prev.clone()); return stored_value;
}
```

```rust
// (b) the callee is context-independent: one walk anywhere is enough
if is_context_independent(db, func) { /* 08 layer 5: graft the memoised subtree */ }
```

A grafted subtree merges at output like any other copy, so its counts come
out right for free. (An earlier version of fix (a) *skipped* the second
walk, keyed on the arguments alone. In a merge-while-walking design that
missed the count bumps of the repeat's children; here it would drop the
node entirely. Grafting fixes both.)

**Is the saved tree the only one that leaves `pylspt`?** Yes, for now: the
wire format is the merged `call_frame_stack`, for parity with the Jedi
driver (`04-build/03`). If v-noc's DB or UI ever wants the unmerged runs —
to show *which* call produced which leaf — send the analysis tree and move
`merge_for_output` to v-noc. That's a contract change, not a refactor.

### Why the example in this plan uses two *callers*

Because the same-frame case is a **bad demonstration of context-sensitivity**.
Merged, it renders as:

```
root
└── emit  (call_count=1)
    ├── JsonWriter.write
    └── CsvWriter.write
```

which is roughly what a path-free tool would print too. The information is
different, but the JSON looks similar, so it proves nothing.

Put the calls in different functions and the difference becomes undeniable —
see [`02-why-ty-alone-cannot.md`](02-why-ty-alone-cannot.md#the-illustration).
**Use the cross-scope form in every fixture that tests path sensitivity.**

---

## `_merge_frame_stack` — the second merge

`service.py:57-70` merges the per-call-site trees returned by each
`CallHierarchyResolver` into one root:

```python
matched = next((c for c in target.children if c.target_id == source_child.target_id), None)
```

Note: this one matches on **`target_id`**, while `add_child` matches on
**`target_qname`**. Different keys at different levels. Reproduce both exactly;
do not "clean this up" into one rule.

Also note `_merge_frame_stack` creates the matched node without copying
`call_count` (`service.py:64-68`), so counts from sub-resolvers are **lost** at
the merge boundary. Preserve the behaviour, add a `// PARITY:` comment.

---

## Serialisation

`to_json_tree` exists because `CallFrameStack.parent` makes `model_dump` recurse
infinitely. In Rust, keep the parent link out of the serialised type entirely:

```rust
// Internal: has parent, arena-allocated or index-based to avoid Rc cycles.
// ONE PER WALKED CALL — never merged (§ Where the rule applies).
struct Frame<'db> { parent: Option<FrameId>, call_site: TextRange, /* ... */ }

// Wire: no parent, derives Serialize. Built ONLY by `merge_for_output`,
// which is where `(parent, qname)` merging and `call_count` happen.
// `_merge_frame_stack`'s `target_id` merge (above) runs after it.
#[derive(Serialize)]
struct FrameJson {
    target_qname: String,
    target_id: String,
    call_count: u32,
    children: Vec<FrameJson>,
}
```

Use an arena (`Vec<Frame>` + `FrameId(u32)`) rather than `Rc<RefCell<>>`.
Parent links become indices, the borrow checker stops fighting you, and the
whole structure drops in one `Vec` deallocation instead of a cascade of
refcount decrements.

---

## Determinism ★

Two runs on unchanged input **must** produce byte-identical output. Currently at
risk from:

- `FxHashMap`/`FxHashSet` iteration order — never iterate a hash map to produce
  output. Sort, or use `IndexMap`/`BTreeMap`.
- Rayon parallelism across call sites — collect into an indexed structure, then
  merge in deterministic order.
- **Child order.** Evaluation-driven walking ([`10` §1](10-return-values-and-state.md#-evaluation-drives-the-walk--there-is-no-call-list))
  adds children in Python's evaluation order: `make` before `emit` for
  `emit(make())`. If the golden files order children by source position
  instead, sort each merged group by its first node's `call_site` in
  `merge_for_output` — never by changing the evaluation order.
  **[check]** against the golden files.
- `visited_qnames` is a Python `set` **[verified, call_resolver.py:133]** but is
  only used for membership, not iteration — safe.

Non-determinism here is poison: it makes your parity tests flaky, and it makes
v-noc's downstream diffs meaningless. Add a test that runs the same request 10×
and asserts identical output.

---

## The invariant, stated

> For every function activation in the tree, the path from the root to it
> corresponds to a sequence of call sites that could execute in that order, and
> the callee resolved at each step reflects the values flowing along that
> specific sequence.

Any change that breaks this turns your tool into a call graph with extra steps.
Put this sentence in a doc comment on `Interp`.

---

→ Next: [`10-return-values-and-state.md`](10-return-values-and-state.md)
