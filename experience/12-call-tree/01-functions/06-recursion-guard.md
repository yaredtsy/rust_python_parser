# 01.06 — The recursion guard

**You build:** a path of the functions being walked. A call to one already on
the path becomes a `[recursive]` leaf and is not entered.
**Fixture:** `python/recursion.py`
**Plan:** `plan/03-call-tree/14-output.md` § "The tree is a view" (the
`Recursive` mark), `01-what-jedi-actually-does.md` step 5e

---

## The fixture

```python
def f(): f()
def g(): h()
def h(): g()
def countdown(n):
    if n:
        countdown(n - 1)
def leaf(): pass
def twice():
    leaf()
    leaf()

def main():
    f(); g(); countdown(3); twice()

# expect:
# main
#   f
#     f [recursive]
#   g
#     h
#       g [recursive]
#   countdown
#     countdown [recursive]
#   twice
#     leaf
#     leaf
```

## Predict

1. Without a guard, what happens on `f()`?
2. `twice` calls `leaf` twice. Why must the second call still be walked?
3. Where should the guard look: at every function walked so far in this
   request, or only at some of them?

---

## Build

### Decision 1: stop on the current path, not on "seen before"

```rust
// in Walk
path: Vec<String>,     // qualified names of the bodies being walked, entry first

// in call_function, before anything else
let qname = self.sem.qname(def);
if self.path.contains(&qname) {
    let mut node = Node::call(qname, site);
    node.marks.push(Mark::Recursive);
    self.frame.children.push(node);
    return;
}
self.path.push(qname.clone());
// … walk the body …
self.path.pop();
```

**Why the path:** a call tree shows each function once per path that reaches
it. `leaf` is reached twice in `twice`, on two different paths (two call
sites), and both must be walked. Only a call back into a function that is
still being walked on *this* path is a cycle.

**Rejected:**
- A request-wide "already walked" set. It stops the second `leaf`, and the
  result is a call graph, not a tree.
- A depth limit alone. `f → f` would be walked to the limit, producing a
  chain of hundreds of `f` nodes, and every real cycle would cost as much as
  the deepest legitimate path.

### Decision 2: compare qualified names, like the Jedi driver

The Jedi driver's ancestor check compares qualified names
(`call_resolver.py:157`). Part 02's renderer uses the same rule. Using the
same comparison here keeps inline mode and the golden files in agreement.

The one case where name and definition differ: two `def f` under `if X:` /
`else:` share a name. Both are treated as one function for the guard, as Jedi
does.

### Decision 3: show the recursive call, don't drop it

The Jedi driver drops a recursive call from the tree. pylspt keeps it as a
leaf marked `Recursive`: the code does make that call, and hiding it makes a
recursive function look like it calls nothing. This is one of the listed,
on-purpose differences from the golden files (`plan/03-call-tree/15` §
"Parity with the Jedi driver").

### What the guard does not solve

The value of a recursive call. The callee is not walked, so nothing says what
it returns. For now the call's value is still ty's answer. Step 10 makes it
`Unknown`, and part 02 (steps 05–06) finds the real value with a fixpoint,
without walking any body twice.

Deep but non-recursive chains are not stopped by this guard. Each level is a
few Rust stack frames, so a chain of thousands of functions can overflow the
stack. Part 02 adds `max_stack`.

### Yours

- `path: Vec<String>` in `Walk`. `call_tree` pushes the entry's name before
  walking it.
- The guard in `call_function`.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/call.rs: call_function, replaced
fn call_function(&mut self, def: Definition<'db>, site: TextRange) {
    let qname = self.sem.qname(def);
    if self.path.contains(&qname) {
        let mut node = Node::call(qname, site);
        node.marks.push(Mark::Recursive);
        self.frame.children.push(node);
        return;
    }

    let parsed = self.sem.parsed(def);
    let Some(node) = self.sem.function_node(def, &parsed) else { return };

    self.path.push(qname.clone());
    let caller = std::mem::replace(&mut self.frame, Frame::new(self.sem, def));
    self.walk_stmts(&node.body, Env::default());
    let callee = std::mem::replace(&mut self.frame, caller);
    self.path.pop();

    self.frame.children.push(Node { qname, site: Some(site), marks: Vec::new(), children: callee.children });
}
```

```rust
// src/interp/mod.rs: call_tree, after creating the walk
let qname = sem.qname(def);
walk.path.push(qname.clone());
```

</details>

---

## Run

```rust
#[test]
fn p01_06_recursion() {
    check("01-functions/python/recursion.py");
}
```

---

## Where you are

```
Walk  { sem, frame, path: [qname] }
```

Every input terminates now. It can still take a very long time: step 13.

---

## Exercise

1. Add `def a(): b()`, `def b(): c()`, `def c(): a()` and call `b()` from
   `main`. Write the expected tree.
2. `countdown` really does recurse three times at runtime. Why is one
   `[recursive]` leaf the right output, and not three nested `countdown`
   nodes?
3. Why can't the guard live in the renderer, the way it will in part 02?

<details><summary>Answers</summary>

1. ```
   main
     b
       c
         a
           b [recursive]
   ```
2. Conditions are never evaluated, so the analysis can't know how many times
   it recurses: `n` could be anything. The tree shows the call exists, once,
   at the point where the path closes. Any fixed number of nested nodes would
   be a guess.
3. In inline mode the tree *is* the walk: the walker produces each node by
   walking its callee. Without the guard in the walker, there is no tree to
   render, only an infinite walk. In part 02 the walker records edges between
   contexts, and recursion there is a cycle in a finite graph, so the
   renderer can stop it.

</details>

---

→ Next: [`07-arguments.md`](07-arguments.md)
