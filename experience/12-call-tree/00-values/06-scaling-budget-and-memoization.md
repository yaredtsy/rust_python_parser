# 00.06 — Scaling: budgets, fan-out caps, and memoisation

The previous file stopped *recursion*. This one stops everything else that
makes a context-sensitive tree expensive — and names the one optimisation
that matters most on a real codebase. Read this before you write a single
line of `01-functions/` or `02-classes/`'s machinery, because retrofitting
a budget into an interpreter that doesn't check one anywhere is far more
work than threading it through from the start.

**Reading:** [`plan/03-call-tree/08-termination-and-cycles.md`](../../../plan/03-call-tree/08-termination-and-cycles.md)
— this file is the practitioner's summary; that one has the full rationale
and all six layers.

---

## Four cheap layers, applied in order

```rust
pub struct Limits {
    pub max_depth:         usize = 24,
    pub max_nodes:         u32   = 100_000,
    pub max_union_fanout:  Option<usize> = Some(4),   // None = no union cap
    pub max_chain_fanout:  usize = 8,
    pub deadline:          Duration = Duration::from_secs(10),
}
```

| layer | what it bounds | checked where |
|---|---|---|
| ancestor guard | recursion (previous file) | every `resolve_call` |
| depth cap (`max_depth`) | how deep a path can go | every `resolve_call`, before recursing |
| node budget (`max_nodes`) | **total work for the whole request** | one global counter, ticked at the top of every `resolve_call` — no exceptions |
| fan-out caps | how wide a union or attribute chain can explode | `cap_fanout` for unions (`03-env-and-scope.md`), `member`'s chain walk (`02-classes/07`) |

**Changing the union cap.** It's a request setting, not a constant:
`{"max_union_fanout": 8}` in `initialize`'s config, or
`pylspt-dev calls f.py --limits max_union_fanout=8`. Use `null` / `=off` to
turn it off — useful for measuring how much precision the cap costs on a
real project, not for shipping. When it does fire, `cap_fanout` prunes
non-project values first, so it only throws away a union whose *project*
values alone exceed the cap.

```rust
struct Budget { remaining: u32, hit_limit: bool }
impl Budget {
    fn tick(&mut self) -> bool {
        if self.remaining == 0 { self.hit_limit = true; return false }
        self.remaining -= 1;
        true
    }
}
```

One counter, for the **whole request**, not per-branch — this is the hard
latency guarantee: work is bounded regardless of what the input looks like.
Without it, one adversarial or just unusually fan-out-heavy file hangs the
driver, and a hung driver is a worse user experience than an imprecise one.

**Report truncation, don't hide it:**

```jsonc
{ "call_frame_stack": {...},
  "truncated": true,
  "stats": { "nodes": 100000, "depth_hits": 12, "budget_exhausted": true,
             "annotation_fallbacks": 7 } }
```

Additive to the existing wire shape, so it's not a breaking change, and it
turns "why is this tree incomplete" from a support ticket into a number you
can read off the response. Instrument **every** limit this way, including
the annotation-fallback counter from the previous file — the same
principle applies to any place precision quietly degrades.

## Why `add_child`'s dedup doesn't count as a brake

```python
# call_resolver.py:37-45 — dedupes siblings by qname within ONE frame
```

This reduces *breadth* within a single frame, but it is a **presentation**
rule, not a termination mechanism — don't rely on it to bound anything.
`pylspt` applies it only when saving (`00-values/05`), so the analysis tree
has 50 `emit` nodes for 50 calls and the node budget counts all of them.
The saved tree has one, which is why its size hides the real cost.

## The big win: memoise context-independent subtrees

The insight, stated plainly: **most functions don't use their parameters as
callees.**

```python
def log_event(name, payload):
    logger.info(name)          # doesn't depend on any parameter
    metrics.bump(name)
```

`log_event`'s subtree is identical on every path that reaches it. Computing
it 500 times — once per call site across a real project — is pure waste,
and leaf utility functions like this one dominate node counts in real
codebases.

```rust
#[salsa::tracked]
fn is_context_independent(db: &dyn Db, func: FunctionDef<'_>) -> bool {
    // seed: every parameter name (incl. `self`) is "tainted"
    // propagate through local assignments: x = <tainted> taints x
    // a call whose callee mentions a tainted name       → dependent
    // self.anything(...)                                → dependent
    // a tainted name passed as an argument to a call     → dependent
    // otherwise                                          → independent
}
```

```rust
if is_context_independent(db, func) {
    if let Some(cached) = self.memo.get(&func) {
        frame.graft(cached.clone());    // Arc clone of a prebuilt subtree
        return;
    }
    let subtree = self.walk_body_fresh(func);
    self.memo.insert(func, Arc::new(subtree.clone()));
    frame.graft(subtree);
}
```

Two cautions, both load-bearing:

1. **The ancestor guard still applies to a grafted subtree.** Grafting a
   memoised subtree containing `f` underneath an ancestor also named `f`
   would violate the same-function-never-nested-in-itself invariant from
   [`05-frame-pathkey-and-budget.md`](05-frame-pathkey-and-budget.md). Either
   track each memoised subtree's qname set and skip the memo when it
   intersects the current ancestor chain, or — simpler, and nearly as
   effective — just don't memoise subtrees that contain recursion.
2. **`is_context_independent` gets *stricter* once value flow exists**
   (`01-functions/05-returns-and-value-flow.md`): a parameter reaching a
   callee *through a local binding* (`x = w; x.write()`) still makes the
   function context-dependent, even though no parameter name appears
   directly at the call site. **Being conservative is free** — marking
   something dependent when it isn't costs you some memoisation; the
   reverse produces silently wrong trees.

**Build this after correctness, not before it** — plan/03-call-tree/08 puts
it at a later milestone on purpose. But design `Frame` so grafting is
*possible* from day one; retrofitting `Arc`-shared subtrees into a tree of
plain owned `Vec<Frame>` later is the kind of refactor you want to avoid.

## Exercise

```python
def helper(a, b):
    logger.info(a)
    validate(b)

def path_one(): helper(X(), Y())
def path_two(): helper(Z(), W())
def main(): path_one(); path_two()
```

1. Is `helper` context-independent? Walk the taint rule by hand.
2. If it is, how many times does `helper`'s *subtree* actually get computed
   across this whole program, with memoisation on? Without it?
3. Now change `helper` to `def helper(a, b): a.write(b)`. Is it still
   context-independent? What changed?

<details><summary>Answers</summary>

1. Yes. Neither `a` nor `b` ever appears in a callee position or a
   receiver position — `logger.info(a)` passes `a` as an *argument*, which
   doesn't taint the callee `logger.info` itself (the taint rule is about
   whether a tainted name becomes something you *call*, not about whether
   it flows anywhere at all).
2. Once, with memoisation — `path_one` and `path_two` both hit the memo
   after the first computation and graft the cached subtree. Without it,
   twice (once per distinct frame it's walked under — `path_one`'s `helper`
   and `path_two`'s `helper` are different frames per the `(parent, qname)`
   rule from the previous file).
3. **No**, now it's context-dependent — `a.write(b)` puts the *parameter*
   `a` directly in receiver position, so whether `write` resolves to
   `X.write` or `Z.write` depends on which path called `helper`. Memoising
   this subtree would silently produce the same (wrong) leaf under both
   `path_one` and `path_two`.

</details>

---

With this folder's vocabulary in hand — `AbstractValue`, `OriginId`/`ValueId`,
`Env`, the annotation fallback, `Frame`/`PathKey`, and the budget — you're
ready for the two case-by-case folders. Do the exam first.

→ [`exam.md`](exam.md), then [`../01-functions/README.md`](../01-functions/README.md)
