# 00.05 — `Frame`, `PathKey`, and the ancestor guard

The vocabulary `01-functions/` and `02-classes/` both lean on constantly:
what a node in the output tree actually *is*, and the one rule that keeps
recursive Python from becoming an infinite Rust loop.

**Reading:** [`plan/03-call-tree/09-path-identity.md`](../../../plan/03-call-tree/09-path-identity.md)
in full — this file is a compressed on-ramp to it, not a replacement.

---

## Graph vs tree, in one picture

```
        graph (ty_ide, pyan)              tree (your driver)
        ────────────────────              ──────────────────
      main                               main
      ├──► emit ──► write                ├── emit
      │       ▲                          │   └── JsonWriter.write
      └──► log ┘                         ├── emit
                                         │   └── CsvWriter.write
      write appears ONCE                └── log
                                             └── JsonWriter.write

                                         write appears 3×, and the
                                         first two are DIFFERENT write
```

A call graph answers *"what exists?"* — one node per function, full stop.
Your tool answers *"what happens, on this path?"* — one node per function
**per path that reaches it**. `plan/03-call-tree/02`'s whole argument against
`ty_ide::outgoing_calls` is this distinction; everything from here on is
mechanism for keeping it.

## What identifies a node: `(parent frame, target_qname)`

★ **Confirmed by the author, no ambiguity:** two calls to the same function
*from the same frame* produce **one** node. The same function called from
**different** frames produces **different** nodes.

```python
def main():
    emit(JsonWriter(), a)      # both from main →
    emit(CsvWriter(),  b)      # ONE emit node, two leaves inside it
```
```
root
└── emit  (call_count=1)
    ├── JsonWriter.write
    └── CsvWriter.write
```

versus the cross-scope form, which is the one every fixture in this
exercise should prefer, because the same-frame case looks almost identical
to what a path-*free* tool would print and proves nothing
(`plan/03-call-tree/09` §"Why the example in this plan uses two callers"):

```python
def read_json(): emit(JsonWriter(), data)
def read_csv():  emit(CsvWriter(),  data)
def main(): read_json(); read_csv()
```
```
main
├── read_json
│   └── emit
│       └── JsonWriter.write     ← ONLY this one
└── read_csv
    └── emit
        └── CsvWriter.write      ← ONLY this one
```

Two `emit` nodes, one per frame — not because there were two call *sites*
(there's one, textually, inside `emit` itself), but because `emit` is
reached from two different parents.

## `Frame` — the node, in memory

```rust
struct Frame<'db> {
    qname: String,
    target_id: String,
    parent: Option<FrameId>,     // index, not Rc<RefCell<>> — see below
    children: Vec<FrameId>,
}

impl Frame<'_> {
    fn is_ancestor(&self, qname: &str) -> bool {
        let mut cur = Some(self);
        while let Some(f) = cur {
            if f.qname == qname { return true }
            cur = f.parent.map(|id| &arena[id]);
        }
        false
    }
}
```

Use an **arena** (`Vec<Frame>` + `FrameId(u32)`), not `Rc<RefCell<Frame>>`.
Parent links become plain indices, the borrow checker has nothing to fight
you about, and the whole tree frees in one `Vec` drop instead of a cascade
of reference-count decrements. The **wire** type — what actually gets
serialized — drops `parent` entirely (it would make `Serialize` recurse
forever) and is a separate, simpler struct:

```rust
#[derive(Serialize)]
struct FrameJson { target_qname: String, target_id: String, call_count: u32, children: Vec<FrameJson> }
```

## `add_child` returns an *existing* frame — read this twice

```python
# call_resolver.py:37-45
def add_child(self, child):
    for existing in self.children:
        if existing.target_qname == child.target_qname:
            existing.call_count += 1
            return existing
    self.children.append(child); child.parent = self; return child
```

This is the mechanism behind "same frame, same qname → one node." Two
consequences that catch people the first time:

1. **`call_count` starts at 0 and counts *additional* calls.** One call → 0.
   Three calls → 2. Counter-intuitive on sight — match it anyway, it's part
   of the wire contract.
2. **A frame can be walked into more than once, with different
   environments**, and must not assert it starts empty:

```rust
let child = frame.add_child(qname, id);        // may return an EXISTING frame
let callee_env = self.bind_parameters(func, &args, receiver);
self.walk_body(func, &callee_env, child);      // walks INTO a frame that may already have children
```

Call `emit` twice from `main` with two different writers, and `walk_body`
runs **twice** against the *same* child frame — once per writer — and both
runs' results land as siblings inside that one node (the `call_count=1`
example above).

## `PathKey` — the ancestor chain made explicit

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PathKey(SmallVec<[DefId; 16]>);

impl PathKey {
    fn push(&self, def: DefId) -> PathKey { /* append, cheap: small inline buf */ }
    fn contains(&self, def: DefId) -> bool { /* the ancestor check */ }
    fn depth(&self) -> usize { self.0.len() }
}
```

This is **call-string sensitivity** — the *k*-CFA family from the
program-analysis literature, with `k` = your depth cap, unbounded until that
cap fires. Knowing that name buys you one useful fact: the termination
problem below is a studied one, with a studied fix
([`06-scaling-budget-and-memoization.md`](06-scaling-budget-and-memoization.md)'s
"summary memoisation" is the textbook answer to it).

`PathKey` is **not** frame identity — frame identity is the `(parent,
qname)` rule above, which is simpler. `PathKey` exists for three narrower
jobs: the ancestor guard, the depth cap, and memo keys where a *suffix* of
the path (not the whole thing) is the right cache key.

## Why the search needs a brake at all

```
depth    nodes (avg 3 calls/function)
5        243
10       59,049
15       14,348,907
20       3,500,000,000
```

Your current Python driver survives this because Jedi is slow enough that
pathological requests time out before anyone notices — Rust removes that
accidental brake, so you must install a deliberate one. The ancestor guard
below stops *recursion*; it does **not** stop *blowup* (`main` calling 20
things that each call 20 things is perfectly acyclic and perfectly
explosive). `06-scaling-budget-and-memoization.md` is the rest of the
story.

## The ancestor guard — recursion, specifically

```python
# call_resolver.py:157
if call_frame_stack.is_ancestor(qname): continue
```

```rust
if frame.is_ancestor(&qname) { continue }   // stop BEFORE adding a child
```

⚠ **This is output semantics, not a termination trick you're free to
redesign.** It defines what "one unique path per function" *means* — a
function may appear many times in the tree, just never nested inside
itself. It happens to also keep recursive Python from looping forever, but
that's a side effect, not its job. Contrast with the **in-flight memo** used
for return-value queries in
[`01-functions/05-returns-and-value-flow.md`](../01-functions/05-returns-and-value-flow.md) —
same-sounding problem, different mechanism, because it answers a different
kind of question (an unobservable value query, not the tree's own shape).
Conflating the two gets both wrong, in opposite directions — that file
works the counterexample.

## Exercise

```python
def f(): g()
def g(): f()          # mutual recursion

def walk(node, visit):
    visit(node)
    for c in node.children: walk(c, visit)    # recursion with a callback
```

1. Trace `f()` called from `main`. At what point does `is_ancestor` fire,
   and for which qname?
2. In `walk`, the recursive call is `walk(c, visit)` — but the *callback*
   `visit` is not what's recursing. Does `is_ancestor` ever need to consider
   `visit` separately? Why or why not?
3. Two sibling calls, `f(); f()` from one frame (not mutual — the SAME `f`,
   twice, no recursion into itself involved) — how many children does the
   parent frame have, and what's `call_count`?

<details><summary>Answers</summary>

1. `main` calls `f` → frame for `f` created, ancestors = `[root, main, f]`.
   `f`'s body calls `g` → frame for `g` created, ancestors = `[root, main,
   f, g]`. `g`'s body calls `f` → **`is_ancestor("f")` is true** (it's
   already on the chain) → that call is dropped, `f→g→f` does not recurse
   further.
2. No — `is_ancestor` only ever needs to track `walk` (the thing that's
   actually calling itself); `visit` is resolved normally as a parameter
   value each time `walk`'s body runs, and nothing about it is self-similar.
   This is `plan/03-call-tree/07`'s point: callbacks fall out of the
   ordinary machinery, with no special cycle-handling of their own.
3. **One** child (`add_child` merges by qname), `call_count == 1` (started
   at 0, bumped once for the second call).

</details>

---

→ Next: [`06-scaling-budget-and-memoization.md`](06-scaling-budget-and-memoization.md)
