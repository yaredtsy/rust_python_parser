# 03.08 — Termination, cycles, and the work budget

**Do not skip this chapter.** It is the one place where the Rust port can be
*worse* than the Python one, because Rust is fast enough to actually reach the
pathological parts of the search space that Jedi was too slow to find.

---

## Why the search is unbounded

A context-sensitive call tree is exponential in depth. If the average function
makes 3 project-internal calls:

| Depth | Nodes |
|---:|---:|
| 5 | 243 |
| 10 | 59,049 |
| 15 | 14,348,907 |
| 20 | 3.5 × 10⁹ |

Your current driver survives because Jedi is slow enough that requests time out,
users learn to only ask about shallow entry points, and `_is_project_code`'s
project-only rule prunes hard. **Rust removes the accidental brake.** You must
install a deliberate one.

---

## Layer 1 — the ancestor guard (existing behaviour, keep exactly)

```python
# call_resolver.py:157
if call_frame_stack.is_ancestor(qname): continue
```

Walks the `parent` chain and skips if `qname` is already on it.

```rust
impl Frame<'_> {
    fn is_ancestor(&self, qname: &str) -> bool {
        let mut cur = Some(self);
        while let Some(f) = cur {
            if f.qname == qname { return true }
            cur = f.parent;
        }
        false
    }
}
```

O(depth) per check. With a depth cap it's fine, but if you profile it hot,
carry an `FxHashSet<QNameId>` down the recursion (clone-on-push, or push/pop
around the recursive call). Intern qnames to `u32` first — you compare them
constantly, and string comparison in this loop is pure waste.

**This handles recursion, not blowup.** `f→g→h→f` is stopped;
`main` calling 20 things that each call 20 things is not.

⚠ **`is_ancestor` is output semantics, not a termination mechanism.** It defines
what "unique path per function" means (`call_resolver.py:157`) and is visible in
the JSON — so it is part of the contract and not yours to redesign.

Termination for the *value* queries in
[`10`](10-return-values-and-state.md#the-five-things-to-get-right) is a separate,
unobservable concern, handled by an in-flight memo entry rather than an ancestor
check. Reusing `is_ancestor` there is wrong in both directions — it misses
`f→g→f` through returns, and it blanks out recursive functions whose return
value is perfectly determinable.

## Layer 2 — depth cap

```rust
const MAX_DEPTH: usize = 24;
if frame.depth >= MAX_DEPTH { self.stats.depth_truncations += 1; return; }
```

Make it configurable via `initialize`'s `config` dict — the field already exists
in `InitializeParams` **[verified, rpc.py:17]** and is currently unused. Free
extension point, no wire-format change.

## Layer 3 — global node budget ★ the important one

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

Called at the top of every `resolve_call`. **One counter for the whole request**,
not per-branch. Start at 100_000 and tune.

This is your hard latency guarantee: work is bounded regardless of input shape.
Without it, one adversarial file makes the driver hang, and a hung driver is a
worse user experience than an imprecise one.

**Report truncation in the response.** Add a field alongside `call_frame_stack`:

```jsonc
{ "call_frame_stack": {...},
  "truncated": true,
  "stats": { "nodes": 100000, "depth_hits": 12, "budget_exhausted": true } }
```

Additive, so it doesn't break the existing contract, and it turns "why is this
tree incomplete" from a mystery into a number.

## Layer 4 — fan-out caps

From [`04`](04-value-domain.md) and [`06`](06-attributes-and-self.md):

```rust
const MAX_UNION_FANOUT: usize = 4;   // union arms explored per expression
const MAX_CHAIN_FANOUT: usize = 8;   // values carried through an attr chain
```

A union of 40 types at one node multiplies everything below it by 40. Cap and
count. If the counter fires often on real projects, the cap is too low —
instrument before tuning.

## Layer 5 — memoise context-independent subtrees ★★ the big win

The insight: **most functions don't use their parameters as callees.**

```python
def log_event(name, payload):
    logger.info(name)          # ← callee doesn't depend on any parameter
    metrics.bump(name)
```

`log_event`'s subtree is identical on every path. Computing it 500 times is pure
waste — and it's the dominant cost in real codebases, where leaf utility
functions are called from everywhere.

**Compute, once per function, whether its call tree is context-dependent:**

```rust
/// A function is context-INdependent if no parameter (incl. `self`) can flow
/// into a callee position, transitively.
#[salsa::tracked]
fn is_context_independent(db: &dyn Db, func: FunctionDef<'_>) -> bool {
    // Small local taint pass over the body:
    //   - seed: every parameter name (incl. `self`) is tainted
    //   - propagate through assignments:  x = <tainted>  taints x
    //     ★ required once local assignment tracking exists — see ch.10
    //   - a call whose callee expression mentions a tainted name → dependent
    //   - `self.anything(...)`                                   → dependent
    //   - a tainted name passed as an argument to another call   → dependent
    //     (it could become a callee one level down)
    //   - otherwise                                              → independent
}
```

> ⚠ **Value flow makes this check stricter.** Before
> [`10`](10-return-values-and-state.md), a syntactic name scan sufficed; with
> local assignments recorded, `x = w; x.write()` is context-dependent through
> `x`, and a scan that only looks at parameter names would wrongly memoise it.
> Being conservative is free — over-marking costs some memoisation, under-marking
> produces wrong trees.

Then:

```rust
if is_context_independent(db, func) {
    if let Some(cached) = self.memo.get(&func) {
        frame.graft(cached.clone());     // Arc clone of a prebuilt subtree
        return;
    }
    let subtree = self.walk_body_fresh(func);
    self.memo.insert(func, Arc::new(subtree.clone()));
    frame.graft(subtree);
}
```

**Expected effect on real projects: large.** Leaf utilities, logging, validation
helpers, and property getters dominate node counts and are almost all
context-independent.

Two cautions:
- The **cycle guard still applies** to a grafted subtree. A memoised subtree
  containing `f` grafted underneath an ancestor `f` would violate the
  `is_ancestor` invariant. Either record each memoised subtree's qname set and
  skip the memo when it intersects the current ancestor chain, or don't memoise
  subtrees that contain recursion. The second is simpler and nearly as
  effective.
- `call_count` dedup is **per frame**. A grafted subtree carries its own counts;
  make sure grafting doesn't double-count when the same callee already exists in
  the target frame.

**Build this at Milestone 6, not Milestone 4.** Get correctness first, measure,
then add. But design `Frame` so grafting is possible from the start —
retrofitting an `Arc`-shared subtree into a tree of owned `Vec<Frame>` is
unpleasant.

## Layer 6 — per-request wall clock

```rust
if self.started.elapsed() > self.deadline { self.budget.remaining = 0; }
```

Check every 1024 ticks (cheap: `if self.stats.nodes & 0x3FF == 0`). Belt and
braces — the node budget bounds *work*, this bounds *time* when individual
nodes turn out to be expensive.

---

## Interaction with `add_child`'s dedup

```python
# call_resolver.py:37-45 — dedupes by qname, bumps call_count
```

This is *not* a termination mechanism — it dedupes siblings within one frame,
so `f(); f()` yields one child. It reduces breadth but not depth, and it does
not prevent blowup. Don't rely on it. It is a *presentation* rule.

---

## Recommended defaults

```rust
pub struct Limits {
    pub max_depth:          usize = 24,
    pub max_nodes:          u32   = 100_000,
    pub max_union_fanout:   usize = 4,
    pub max_chain_fanout:   usize = 8,
    pub deadline:           Duration = Duration::from_secs(10),
    pub memoise_independent: bool = true,
}
```

All overridable via `initialize`'s `config` dict.

**Instrument every limit.** A counter per limit, returned in `stats`. When
someone reports "the tree is missing things", the answer should be a number
from a log line, not an afternoon with a debugger.

---

## Test

- **Direct recursion** `f→f`: terminates, one `f` node.
- **Mutual recursion** `f→g→f`: terminates.
- **Deep chain** 100 functions each calling the next: truncates at `MAX_DEPTH`,
  `truncated: true`.
- **Wide fan-out**: 10 functions each calling 10, depth 6 → budget exhausts
  cleanly, no OOM, no hang.
- **Adversarial**: point it at a large real project's entry point and record
  p50/p95/p99 latency and node counts. **Do this before shipping**, not after.

---

→ Next: [`09-path-identity.md`](09-path-identity.md)
