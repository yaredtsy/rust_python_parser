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

Handled by `add_child` **[verified, call_resolver.py:37-45]**:

```python
def add_child(self, child):
    for existing in self.children:
        if existing.target_qname == child.target_qname:
            existing.call_count += 1        # ← merge
            return existing
    self.children.append(child); child.parent = self; return child
```

So `f(); f()` from one frame → **one** child with `call_count == 1`.

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

   **Not** two separate `emit` subtrees.

---

## The merge rule

**Confirmed by the author.** There is no ambiguity here — settle it once:

> **Frame identity is `(parent frame, target_qname)`.**
>
> Two calls to the same function *from the same frame* produce **one** node.
> The same function called from **different** frames produces **different**
> nodes.

So "one unique path per function" means *one node per (path, function)*, not
*one node per call site*. Two call sites on the same path are the same path.

### Three design consequences

**1. A frame is walked more than once, with different environments.**

```rust
let child = frame.add_child(qname, id);        // may RETURN AN EXISTING child
let callee_env = self.bind_parameters(func, &args, receiver);
self.walk_body(func, &callee_env, child);      // walks INTO the shared child
```

Call `emit` twice from `main` and `walk_body` runs twice against the *same*
`child` frame — once with `writer → JsonWriter`, once with `writer → CsvWriter`.
Their results accumulate inside that one node, deduped by qname in turn.

This is fine, but it must be deliberate: **`add_child` returns an existing frame,
and `walk_body` must be able to add to a frame that already has children.**
Do not assert that a frame is empty when you start walking it.

**2. `PathKey` is not the frame identity.** It is only used for:
- the ancestor cycle guard (`contains`)
- the depth cap (`depth`)
- memo keys where a suffix of the path matters

That is a simpler design than keying frames on the whole call string.

**3. Tree size is bounded, but work is not.**

The merge bounds each frame's children by the number of *distinct qnames* called
in that body — not by the number of call sites. So the stored tree stays small.

But calling `emit` 50 times from one frame still walks `emit`'s body **50 times**,
and each walk recurses. The node count hides the real cost. Two cheap fixes:

```rust
// (a) same callee, same argument values, same frame → the walk is pure waste
if !self.walked.insert((child.id, fingerprint(&args))) { return; }
```

```rust
// (b) if the callee is context-independent, one walk is enough regardless of args
if is_context_independent(db, func) && child.already_walked { return; }
```

Fix (a) is a few lines and helps immediately — repeated identical calls are
common. Fix (b) is [`08` layer 5](08-termination-and-cycles.md#layer-5--memoise-context-independent-subtrees--the-big-win).

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
struct Frame<'db> { parent: Option<FrameId>, /* ... */ }

// Wire: no parent, derives Serialize.
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
