# 01.13 — The wall

**You build:** a counter of walks. Then you use it to see the one thing wrong
with everything you built in this part.
**Fixtures:** `python/wall.py`, and `python/make_wall.py` to generate bigger
ones
**Plan:** `plan/03-call-tree/03-design.md` § "Why summaries"

---

## The fixture

```python
def l3a(): pass
def l3b(): pass
def l2a(): l3a(); l3b()
def l2b(): l3a(); l3b()
def l1a(): l2a(); l2b()
def l1b(): l2a(); l2b()
def main(): l1a(); l1b()

# expect:
# main
#   l1a
#     l2a
#       l3a
#       l3b
#     l2b
#       l3a
#       l3b
#   l1b
#     … the same again
```

## Predict

1. How many bodies does inline mode walk for `main`? How many distinct
   functions are there?
2. Add one more level. How do both numbers change?
3. Of the 15 walks, how many could have been skipped, if you knew the answer
   from an earlier walk? What would "the answer" have to include?

---

## Build

### Decision 1: count every walk

```rust
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub walks: u32,      // bodies walked, the entry included
    pub nodes: u32,      // tree nodes produced
}
```

`call_function` increments `walks` each time it walks a body, and `nodes` each
time it pushes a node. `call_tree` returns the stats with the tree.

**Why now:** the plan has a rule that every limit and every cost is counted,
so that a missing edge or a slow request can be explained by a number instead
of a debugger session. Part 02 adds a counter for every new mechanism. This is
the first one.

### Yours

- `Stats` in `interp/mod.rs`. `call_tree` returns `Option<(Node, Stats)>`, and
  `check` ignores the stats.
- A test that asserts the counts for `wall.py`.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/fixture.rs
#[test]
fn p01_13_wall() {
    check("01-functions/python/wall.py");
    let stats = stats_for("01-functions/python/wall.py", "main", &Options::default());
    assert_eq!(stats.walks, 15);
    assert_eq!(stats.nodes, 14);
}
```

`stats_for` opens the fixture the way `check` does and returns the stats of
one entry.

</details>

---

## Run

```bash
cargo test interp::fixture
```

Then try a bigger wall with the `calls` binary from step 01 (build it with
`--release`):

```bash
cd experience/12-call-tree/01-functions/python
python3 make_wall.py 4 8          # wall_big.py: 33 functions, 87,381 walks
python3 make_wall.py 10 6         # wall_big.py: 61 functions, 1,111,111 walks
```

Time both. Then delete `wall_big.py`.

---

## What the counter says

| width × depth | functions | walks, inline |
|---|---|---|
| 2 × 3 (`wall.py`) | 7 | 15 |
| 4 × 8 | 33 | 87,381 |
| 10 × 6 | 61 | 1,111,111 |

Walks grow as width^depth. Functions grow as width × depth.

Real code is not a perfect wall, but it has the shape everywhere: a logger, a
config reader, a database helper, a base class `__init__`. Each one is called
from dozens of places, each of which is called from dozens more. The Jedi
driver has hard caps for this reason (6 executions per function, 200 per
inference state), and past them it silently returns nothing.

### The waste

Look at the 8 walks of `l3a`. Each one starts with the same env (no
parameters), reads the same module globals (through ty, which answers the same
way on every path), and finds the same calls. Seven of the eight are pure
repetition.

But not every repeated walk is waste. In step 07, `run` was walked twice,
once with `cb = on_ok` and once with `cb = on_err`, and the two walks found
different children. Those two walks were both needed.

So the question is: **what does a walk depend on?** If two calls give a
function the same inputs, its walk finds the same calls and returns the same
values, and one walk can answer both.

From everything in this part, a walk of a function depends on:

- its **arguments**, as bound to its parameters (step 07);
- for a closure, its **free variables**, read from the run where its `def`
  ran (step 11);
- **module globals and imports**, which ty answers the same way on every path
  (steps 02, 04);
- the **code**.

Not on the caller's locals (step 05). Not on how deep the call is, or which
path reached it.

The first two are the call's **key**. The record of what one walk found (its
calls and its returns, written so that any caller with the same key can use
it) is the function's **summary** for that key. Part 02 builds both, and
replaces the one function of this part that walks the callee inline.

### What summaries will not fix

The tree itself. `main`'s tree in a 10 × 6 wall has over a million nodes,
because the tree has one node per path, and there are a million paths. Part
02 makes the **analysis** cost linear in the number of distinct keys. The
**output** is still per path, which is why part 05 caps how many nodes are
rendered (`render_nodes`) and marks where it cut, and why v-noc merges equal
siblings when it saves.

---

## Where you are: the end of part 01

```
src/sem.rs              Sem: entry, qname, type_of, shape, member, is_project,
                        parsed, function_node, definition_of, body_scope,
                        free_variable_scope, decorated_defs, type_of_import
src/interp/
  mod.rs      call_tree → (Node, Stats), Options, Mode::Inline
  tree.rs     Node { qname, site, marks, children }, Mark, to_text
  value.rs    AbstractValue = Function | Closure | Module | Ty | Unknown; Values; lift
  env.rs      Env { locals, scope, enclosing }, join
  walk.rs     Walk { sem, frame, path, activations }, Frame, walk_stmts, eval, lookup
  call.rs     eval_call, call_function (inline)        ← the one part 02 replaces
  bind.rs     EvaluatedArgs, bind_parameters, default_value
  member.rs   eval_attribute, member (modules)
```

---

## Exercise

1. A function `log(msg)` is called from 200 places, each with a different
   string literal. With a key made of the argument values, how many walks of
   `log` would that be? What should the key hold instead?
2. `def run(cb): cb()` is called with `on_ok` from 100 places and `on_err`
   from 100 places. How many walks of `run` does it take, at minimum, to build
   a correct tree?
3. Name something a walk depends on that is not in the list above, if you can
   find one in this part. (Hint: what does `free_variable_scope` read?)

<details><summary>Answers</summary>

1. 200, if the key holds the literal values. But the walker never looks at a
   string's contents: conditions are not evaluated (step 09), and a literal is
   never called. So the key should hold the string's *type*, `str`, and all 200
   calls share one walk. Part 02 calls this widening literals.
2. Two: one per distinct callback. The other 198 calls can reuse one of
   those two walks.
3. The code itself, through ty: the semantic index, the scopes, the decorator
   list, ty's types for globals. All of those are the same for every call in
   one request, and they change only when a file is edited. That is why part
   02 can key on arguments and free variables alone, and why the cache is
   cleared on any edit.

</details>

---

→ Next part: [`../02-function-summaries/README.md`](../02-function-summaries/README.md)
