# 01.05 — Following a call

**You build:** inline following. At a call to a project function, the
walker walks the callee's body right there, in a new frame, and the callee's
calls become the children of the call's node.
**Fixture:** `python/follow.py`
**Plan:** `plan/03-call-tree/01-what-jedi-actually-does.md` § "The loop"

---

## The fixture

```python
import util

def leaf(): pass
def middle(): leaf()
def top():
    middle()
    leaf()

def main():
    top()
    middle()

def across_files():
    util.chain()            # util.py: def chain(): helper()

def no_caller_scope():
    x = leaf
    reads_x()

def reads_x():
    x()                     # no local x, no global x

# expect:
# main
#   top
#     middle
#       leaf
#     leaf
#   middle
#     leaf
# expect:
# across_files
#   util.chain
#     util.helper
# expect:
# no_caller_scope
#   reads_x
```

## Predict

1. How many times is `leaf`'s body walked while building `main`'s tree?
2. `util.chain`'s body is in another file. Which semantic model must the
   walker use for `helper()` inside it, and what happens if it uses `main`'s?
3. `reads_x` is called from a function that has `x = leaf`. Why must `x()`
   find nothing?

---

## Build

### Decision 1: inline: walk the callee where it is called

```rust
fn call_function(&mut self, def: Definition<'db>, site: TextRange) {
    let parsed = self.sem.parsed(def);
    let Some(node) = self.sem.function_node(def, &parsed) else { return };

    let caller = std::mem::replace(&mut self.frame, Frame::new(self.sem, def));   // ← the decision
    self.walk_stmts(&node.body, Env::default());
    let callee = std::mem::replace(&mut self.frame, caller);

    self.frame.children.push(Node {
        qname: self.sem.qname(def),
        site: Some(site),
        marks: Vec::new(),
        children: callee.children,
    });
}
```

**Why:** it is the simplest thing that produces a per-path tree, and it is
exactly the Jedi driver's algorithm: resolve the callee, build its context,
analyse its body, recurse. Every node's children are found under the values
of *this* path, because the callee is walked from *this* call.

**What it costs:** a function reached on N paths is walked N times. Step 13
measures that, and part 02 replaces this function. It is the only function in
part 01 that part 02 replaces.

### Decision 2: one walk for the whole request, one frame per body

The callee does not get a new `Walk`. It gets a new `Frame` inside the same
`Walk`, and the caller's frame is put back when it returns.

```
Walk  { sem, frame }          ← one, for the whole request in inline mode
Frame { func, file, model, children }   ← one per body being walked; saved and restored
```

**Why:** in step 11 a closure made in one frame is called from another, and in
part 03 an object built in one frame is used in another. Ids for closures and
objects will live in the `Walk`, so they stay valid across frames. Part 02
changes this on purpose: there, each callee gets its own walk, and nothing
walk-local may cross a call. You will see why then.

### Decision 3: the callee starts with a fresh env

`Env::default()`, not a copy of the caller's. Step 07 puts the parameters
into it, and nothing else.

**Why:** Python scoping. A function sees its own locals, its enclosing
function's variables if it is nested (step 11), and module globals. It never
sees its caller's locals. `reads_x` is called from a function with `x = leaf`,
and `x()` must not resolve to `leaf`.

The Jedi plan chapter calls the two kinds of "parent" by name. The
**enclosing** function is where a function was defined. The **caller** is
where it was called from. Free variables follow the enclosing chain. Values
from the caller arrive only as arguments.

### Decision 4: each body is evaluated with its own file's model

`Frame::new` takes the semantic model of the callee's file. ty's
`inferred_type` documents that it may panic for an expression from another
file than the model's. Using `main`'s model inside `util.chain` is that bug.

### Decision 5: one node per call site and function

Each call to a project function gets its own node, with the site's range.
`top()` and `middle()` in `main` are two nodes. Two calls to `leaf` from `top`
would also be two nodes.

**Why not merge equal names:** v-noc merges siblings with the same name when
it saves the tree, and counts them in `call_count`. pylspt sends one node per
call site, unmerged, with `call_count = 0`
(`plan/03-call-tree/14-output.md` § "`call_count` and v-noc's merge").

> ⚠ Don't run anything recursive yet. `def f(): f()` now walks forever and
> overflows the stack. Step 06 stops it.

### Yours

- `call_function` in `interp/call.rs`, called from `eval_call` for each
  project `Function` callee.
- The call's value is still ty's type for the call expression.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/call.rs
use ruff_python_ast::{ExprCall, ExprRef};
use ruff_text_size::{Ranged, TextRange};

use crate::interp::env::Env;
use crate::interp::tree::Node;
use crate::interp::value::{AbstractValue, Values};
use crate::interp::walk::{Frame, Walk};
use crate::sem::Definition;

impl<'a, 'db> Walk<'a, 'db> {
    pub fn eval_call(&mut self, call: &ExprCall, env: &mut Env<'db>) -> Values<'db> {
        let callees = self.eval(&call.func, env);
        for arg in call.arguments.arguments_source_order() {
            self.eval(arg.value(), env);
        }
        for callee in &callees {
            if let AbstractValue::Function(def) = *callee
                && self.sem.is_project(def)
            {
                self.call_function(def, call.range());
            }
        }
        let ty = self.sem.type_of(&self.frame.model, ExprRef::from(call));
        self.lift(ty)
    }

    /// Inline mode: walk the callee's body now, as a child of this call.
    fn call_function(&mut self, def: Definition<'db>, site: TextRange) {
        let parsed = self.sem.parsed(def);
        let Some(node) = self.sem.function_node(def, &parsed) else { return };

        let caller = std::mem::replace(&mut self.frame, Frame::new(self.sem, def));
        self.walk_stmts(&node.body, Env::default());
        let callee = std::mem::replace(&mut self.frame, caller);

        self.frame.children.push(Node {
            qname: self.sem.qname(def),
            site: Some(site),
            marks: Vec::new(),
            children: callee.children,
        });
    }
}
```

</details>

---

## Run

```rust
#[test]
fn p01_05_follow() {
    check("01-functions/python/follow.py");
}
```

Run every test. Steps 02–04 must still pass. Their callees have empty bodies,
so following them adds no children.

---

## Where you are

```
Walk  { sem, frame }            one per request
Frame { func, file, model, children }   saved and restored around each call
```

This is a complete, if slow, call tree for code without arguments. Everything
from here on makes values flow through it.

---

## Exercise

1. How many frames exist at the deepest point of `main`'s walk? Which ones?
2. If `call_function` started the callee with a clone of the caller's env,
   which block of the fixture would fail, and how?
3. `top` calls `leaf` twice, once directly and once through `middle`. Why must
   these be two separate walks of `leaf` in inline mode, even though they find
   the same thing?

<details><summary>Answers</summary>

1. Four: while `leaf` is walked from `middle` from `top` from `main`, `leaf`'s
   frame is active, and `middle`'s, `top`'s and `main`'s are saved on the Rust
   call stack, each in a `caller` local of a `call_function` that hasn't
   returned yet.
2. `no_caller_scope`: `reads_x` would find `x = leaf` in its env and get a
   `leaf` child Python never calls.
3. In inline mode the children of a node are produced by walking that node's
   callee, so each node needs its own walk. Nothing yet says "this walk would
   find the same as that one". Saying that safely is part 02.

</details>

---

→ Next: [`06-recursion-guard.md`](06-recursion-guard.md)
