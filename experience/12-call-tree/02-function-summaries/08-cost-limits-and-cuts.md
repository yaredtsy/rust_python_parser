# 02.08 — Cost limits and cuts

**You build:** the limits that keep one request bounded: stack depth, number
of contexts (in total, and per function), and a deadline. When one fires, the
call gets a **cut** result. A cut is marked, spreads to whoever read it, and is
never reused.
**Fixture:** `python/cuts.py` (runs with `max_stack = 4`)
**Plan:** `plan/03-call-tree/13-solver-and-limits.md` § "Truncation", § "Limits",
§ "Stack depth in Rust", `03-design.md` § "The rules", rule 3

---

## The fixture

```python
# limits: max_stack=4

def leaf(): return handler
def mid():  return leaf()
def top():  return mid()
def b():    top()
def a():    b()

def main():
    a()          # main(0) → a(1) → b(2) → top(3) → mid(4): cut
    top()()      # main(0) → top(1) → mid(2) → leaf(3): complete

# expect summary:
# main [incomplete]
#   a [incomplete]
#     b [incomplete]
#       top
#         mid
#           leaf
#   top
#     mid
#       leaf
#   handler
```

(Inline mode has no limits: its tree is the same, with no marks.)

## Predict

1. When `top` is first walked (under `b`), its call to `mid` is cut. What does
   `top`'s summary return at that point?
2. Then `main` calls `top()` directly. If `top`'s first summary were reused,
   what would `main` miss?
3. Why is `top` under `b` *not* marked, in the final tree?

---

## Build

### Decision 1: a cost limit gives a cut result, which is not stored

```rust
pub struct Limits {
    // precision (rule 3: decided by the key and the code)
    pub max_rounds: u32,                  // 32. Step 05 (counted), step 06 (CYCLE)
    pub fp_depth: u8,                     // 2. Step 07 (closures), part 04 (objects)
    // cost (each one cuts, is marked, and is never reused)
    pub max_stack: usize,                 // 256
    pub max_contexts: u32,                // 50_000
    pub contexts_per_func: u32,           // 64, counted in this request only
    pub deadline: Option<Duration>,       // 10 s
    // checks
    pub reuse: bool,                      // step 03
}

pub enum Cut { Stack, Contexts, ContextsPerFunc, Deadline }
pub struct Status { pub gaps: Gaps, pub cut: Option<Cut> }
```

In `get`, before walking a new context:

```rust
if let Some(cut) = self.cost_limit(ctx, depth) {
    self.reader_read_cut(cut);
    return Got::Ready(Arc::new(FunctionResult::cut(cut)));   // returns [Unknown], no calls. Not stored
}
```

The edge to the cut context was already recorded. Its value is `[Unknown]`:
"something, we stopped looking". Not empty: empty would claim the call never
returns.

### Decision 2: a walk that read a cut is cut too, and is never reused

`reader_read_cut` marks the reader. When the reader's walk ends, its status
gets the cut, and its slot becomes `State::Cut`, never `Ready`. `get` treats a
`Cut` slot like a new one: the next call walks it again.

```rust
pub enum State { New, Running { depth: usize }, Parked { head: ContextId }, Resolving, Ready, Cut }
```

And when a caller gets back a result with a cut, it is cut too (`pass_cut_up`).
So the cut travels up to the entry: in the fixture `top`, `b`, `a` and `main`
all read it.

**Why never reuse it:** `top`'s first summary says it returns `Unknown`, but
that is because of where `top` was reached (deep), not because of its key.
Reaching `top` from `main` directly has room to finish. If the first summary
were reused there, `main` would get `Unknown` and lose `handler`, and the tree
would be wrong in a way that depends on which call came first.

**This is rule 3:** precision depends only on the key and the code; cost
limits only truncate. A `Ready` summary is a function of its key. A limit that
depends on the path (depth, how many contexts exist so far, the clock) can
never produce a `Ready` summary, so it can never change a reusable answer.

### Decision 3: precision limits are reusable, cost limits are not

| limit | depends on | result |
|---|---|---|
| `max_rounds` (steps 05, 06) | the group's equations: keys and code | `CYCLE` gap. **Reusable** |
| `fp_depth` (step 07) | the values, to a fixed depth | `Unknown` past it. **Reusable** |
| `max_stack` | how deep the path is | cut. Never reused |
| `max_contexts` | how many contexts exist so far | cut. Never reused |
| `contexts_per_func` | how many contexts of this function exist so far | cut. Never reused |
| `deadline` | the clock | cut. Never reused |

Part 04 adds more precision limits (`max_path`, fan-out caps) and applies
`fp_depth` to objects, all on the reusable side.

**Why `contexts_per_func` cuts instead of widening:** the alternative, past
64 contexts give every further call to that function one catch-all context,
looks cheaper. But which calls get the precise contexts and which the
catch-all would depend on which came first. A summary would then depend on the
order of the request, and with a cache across requests (`plan/03-call-tree/15`,
v1.1), on which request ran first.

### Decision 4: cuts are marked in the tree

`render` marks a node `[incomplete]` when any context it shows has a cut or a
gap, or was cut before it was walked. The root gets the mark too. A missing
edge under a marked node is explained by a number in `Stats.cuts`, not by a
debugger session.

In the fixture, `top`'s slot ends up holding its second, complete walk (from
`main`), so `top` under `b` renders complete. `b` and `a` were computed from
`top`'s cut result, so they stay marked.

### Decision 5: the Rust stack is a limit too

`get` recurses through `walk`, `eval` and `eval_call`. 256 levels of that can
overflow the default thread stack long before `max_stack` fires. Run the
solver on a thread with a big stack (`std::thread::Builder::stack_size`), or
grow it on demand with `stacker::maybe_grow` at the top of `get`. **[check]**
the stack used per level on your machine, then set `max_stack` from it.

Inline mode has the same problem with no limit at all: it is one more reason it
is a scaffold, not something to run on real code.

### Yours

- `Limits` (move `reuse`, `max_rounds` and `fp_depth` into it), `Cut`, `Status.cut`,
  `State::Cut`, `cost_limit`, `reader_read_cut`, `pass_cut_up`.
- In a recursion group, if any member read a cut, every member is cut.
- `Stats.cuts`, by kind.
- The harness: a `# limits: k=v,…` line in a fixture applies to summary mode,
  and a fixture with limits skips the no-reuse run (no-reuse compares outputs
  with no cuts at all).
- `render`: the `[incomplete]` mark.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/solver.rs
fn cost_limit(&mut self, ctx: ContextId, depth: usize) -> Option<Cut> {
    let l = &self.opts.limits;
    if depth >= l.max_stack { return Some(Cut::Stack) }
    if self.stats.walks >= l.max_contexts { return Some(Cut::Contexts) }
    let func = self.contexts[ctx].func;
    let n = self.per_func.entry(func).or_default();
    if *n >= l.contexts_per_func { return Some(Cut::ContextsPerFunc) }
    if l.deadline.is_some_and(|d| self.started.elapsed() > d) { return Some(Cut::Deadline) }
    *n += 1;
    None
}

fn reader_read_cut(&mut self, cut: Cut) {
    self.stats.cuts.count(cut);
    if let Some(&reader) = self.stack.last() {
        self.slots[reader].cut.get_or_insert(cut);
    }
}

/// After `get` produced or reused a result: a cut in it makes the reader cut.
fn pass_cut_up(&mut self, res: &FunctionResult<'db>) {
    if let Some(cut) = res.status.cut {
        if let Some(&reader) = self.stack.last() {
            self.slots[reader].cut.get_or_insert(cut);
        }
    }
}
```

`finish_alone` and `settle` copy `slot.cut` into the result's status, and set
the state to `Cut` instead of `Ready` when it is there.

</details>

---

## Run

```rust
#[test]
fn p02_08_cuts() {
    check_both("02-function-summaries/python/cuts.py");
}
```

Then every test. Nothing else may change: with the default limits, no fixture
in this tutorial gets anywhere near them.

---

## Where you are: the end of part 02

```
Solver   { sem, opts { mode, limits }, fps, contexts, slots, stack, heads, fixpoint, stats }
Slot     { state, result, current, walk, lowest, joins, cut }
State    = New | Running{depth} | Parked{head} | Resolving | Ready | Cut
Context  { func, args, free }
Fp       = Unknown | Data | Function | Module | Closure | Back | Union
FunctionResult { calls, returns, status { gaps, cut } }
SVal     = Plain | Closure
```

Compare with `00-values/README.md`: everything above is there, minus what
objects add (`alias`, `Instance`, `Opaque`, `Bound`, `Ref`, `Obj`, effects,
new objects). Parts 03 and 04 add those.

---

## Exercise

1. A function has 100 different callbacks passed to it from 100 callers, and
   `contexts_per_func` is 64. What happens to calls 65–100? Is anything
   reused?
2. Write a fixture where the deadline makes two runs of the same request give
   different trees. Why does the determinism check (part 05) turn the
   deadline off?
3. A cut call returns `[Unknown]`, not empty. Give a line of code where the
   difference changes the tree.

<details><summary>Answers</summary>

1. Each of calls 65–100 is cut: its edge is recorded, its value is `Unknown`,
   and its caller is marked incomplete. Nothing about them is reused, because a
   cut result is never stored. Calls 1–64 got real contexts, which are `Ready`
   and reused as usual.
2. Any fixture big enough to take longer than the deadline on a slow machine
   and less on a fast one: the cut lands in a different place each time.
   Determinism is about the analysis, not the clock, so the check runs with
   `deadline=off`, or asserts that no `Cut::Deadline` happened.
3. `log(cut_call())`: with `Unknown`, `log` is called normally and its value
   used; with empty, the call would be treated as one Python never makes
   (step 01.10) and its value dropped. Or `x = cut_call(); x()`: `Unknown`
   gives no node but keeps the call site; empty means "no value reaches", the
   same tree here, but different everywhere the value flows on.

</details>

---

→ Next part: [`../03-classes/README.md`](../03-classes/README.md)
