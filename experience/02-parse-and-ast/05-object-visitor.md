# Object 5 — `SourceOrderVisitor` and `TraversalSignal`

Walking the tree without enumerating 58 node types by hand.

---

## What it is

A **trait with default methods**. You implement the handful you care about;
the defaults walk everything else for you.

```rust
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal, walk_body, walk_expr, walk_stmt, walk_module,
};

pub trait SourceOrderVisitor<'a> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        TraversalSignal::Traverse            // ← default: descend
    }
    fn leave_node(&mut self, node: AnyNodeRef<'a>) {}

    fn visit_stmt(&mut self, stmt: &'a Stmt) { walk_stmt(self, stmt) }
    fn visit_expr(&mut self, expr: &'a Expr) { walk_expr(self, expr) }
    fn visit_mod(&mut self, module: &'a Mod) { walk_module(self, module) }
    fn visit_decorator(&mut self, d: &'a Decorator) { walk_decorator(self, d) }
    fn visit_parameters(&mut self, p: &'a Parameters) { walk_parameters(self, p) }
    // …and ~15 more, all with walking defaults
}

pub enum TraversalSignal { Traverse, Skip }
```

**[verified]** from `ruff_python_ast/src/visitor/source_order.rs:13, 237`.

"Source order" means it visits nodes in the order they appear in the file —
which is what you want for `call_index` numbering and for output that reads
sensibly.

---

## Why this solves object 2's problem

In object 2, example 2, you tried to find every call by hand and hit the wall:
33 `Expr` variants, and any of them can contain another expression. `BinOp` has
two children, `Compare` has a list, `FString` has elements, `Subscript` has
value and slice, comprehensions have generators and conditions.

The visitor already knows all of it. There are **28 `walk_*` functions**
**[verified]** — `walk_expr`, `walk_body`, `walk_comprehension`,
`walk_except_handler`, `walk_format_spec`, `walk_keyword`, `walk_match_case`,
`walk_type_params`, and so on — one per construct that contains children.

So your job shrinks to: *"tell me when you see a call, and let me decide when to
stop descending."*

---

## Rust: the trait-with-defaults pattern

This is worth understanding properly, because it is how most of ruff's
extensible machinery works.

```rust
struct CallFinder<'a> {
    calls: Vec<&'a ExprCall>,
}

impl<'a> SourceOrderVisitor<'a> for CallFinder<'a> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            self.calls.push(call);
        }
        walk_expr(self, expr);        // ★★ KEEP GOING
    }
}
```

Three things going on:

**1. You override one method.** Everything else uses the default, which walks.

**2. `walk_expr(self, expr)` is the "call super" step.** Rust has no
`super.visit_expr(expr)`, so the default body is a free function you call
explicitly. **Forgetting it stops the traversal dead** at that node — which is
the single most common visitor bug, and it fails silently.

**3. `&mut self`** — the visitor accumulates into itself. That is why `calls` is a
field rather than a return value: a walk cannot return anything, so it collects.

### Using it

```rust
let mut finder = CallFinder { calls: Vec::new() };
walk_body(&mut finder, &ast.body);
println!("{} calls", finder.calls.len());
```

**Rust note — the `'a` lifetime.** `CallFinder<'a>` stores references *into the
AST*, so it cannot outlive the `ParsedModuleRef`. That is correct and free: you
are collecting pointers, not copying nodes. If the borrow checker complains
about the visitor outliving the tree, the fix is to collect owned data (ranges,
names) instead of node references — which is what your node tree does anyway.

---

## `TraversalSignal::Skip` — the rule from `parser.py`

Your `_scan_children` **stops** at a nested def or class (`parser.py:78-80`):
calls inside a nested function belong to *that* function, not the enclosing one.

`enter_node` expresses this directly:

```rust
fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
    match node {
        AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::StmtClassDef(_) => {
            // emit a node for it, recurse separately with a FRESH visitor,
            // and do not descend here
            TraversalSignal::Skip
        }
        _ => TraversalSignal::Traverse,
    }
}
```

That gives you the nesting your wire format wants: one visitor per scope, each
producing its own children.

### ⚠ But `enter_node` fires for your own root too

If you start a visitor at a `StmtFunctionDef` and `enter_node` skips
`StmtFunctionDef`, you skip the very thing you were asked to walk — and get
nothing.

So the visitor needs to know its own root:

```rust
struct ScopeScanner<'a> {
    root: TextRange,              // the range of the scope we started at
    out: Vec<Node>,
}

fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
    if node.range() == self.root {
        return TraversalSignal::Traverse;      // this is me; go in
    }
    match node { … }
}
```

Comparing **ranges** is unambiguous and cheap. `plan/02-mapping/01` sketches
`node == self.root` on `AnyNodeRef`; check what that comparison actually does at
your revision before relying on it — range equality has no such doubt.

---

## `AnyNodeRef` — one type for any node

```rust
pub enum AnyNodeRef<'a> {
    StmtFunctionDef(&'a StmtFunctionDef),
    StmtClassDef(&'a StmtClassDef),
    ExprCall(&'a ExprCall),
    …    // a variant for every node type in the AST
}
```

`enter_node` receives one of these because it is called for *every* node kind,
not just statements or expressions. It implements `Ranged`, so `node.range()`
always works.

You will mostly `match` two or three variants and `_ => Traverse` the rest.

---

## The three behaviours you must carry over

From `plan/00-orientation/01`:

### Quirk 8 — lambdas are dropped, **with their subtree**

```rust
AnyNodeRef::ExprLambda(_) => TraversalSignal::Skip,
```

`parser.py:121` returns `None` for `lambdef`, and because parso's `Lambda` is a
subclass of `Function`, line 80's early `return` drops **the whole subtree**. So
`log(x)` inside `lambda x: log(x)` must not appear anywhere in your output.

Emit nothing, and `Skip`. Not "emit nothing and keep walking" — that would find
the inner call.

### Quirk 10 — position dedup

```rust
struct ScopeScanner<'a> {
    seen: FxHashSet<TextRange>,
    …
}
// before emitting:
if !self.seen.insert(range) { return; }
```

`_scan_children` drops nodes sharing an identical 4-tuple position. In Rust that
is one 8-byte hash key instead of a tuple of four boxed integers.

### Decorators — skip them (object 3's trap)

`decorator_list` is a field of the def, so the default walk enters it. Override
`visit_decorator` to do nothing:

```rust
fn visit_decorator(&mut self, _d: &'a Decorator) {
    // deliberately empty: parso puts decorators OUTSIDE the funcdef,
    // so calls inside them belong to the enclosing scope.
    // PARITY: plan/02-mapping/01, experience/02 object 3.
}
```

Note what this does *not* do: it does not call `walk_decorator`. That is the
"forgetting to walk" bug — used deliberately, which is the only time it is
correct.

---

## Example 1 — count calls, correctly

```rust
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_expr};
use ruff_python_ast::{Expr, ExprCall};

#[derive(Default)]
struct CallCounter<'a> {
    calls: Vec<&'a ExprCall>,
}

impl<'a> SourceOrderVisitor<'a> for CallCounter<'a> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            self.calls.push(call);
        }
        walk_expr(self, expr);
    }
}

// usage — note: walk_body is a FREE FUNCTION, not a method.
// There is no `visit_body` on the trait [verified]; the entry points are
// walk_module / walk_body / walk_stmt / walk_expr.
let mut counter = CallCounter::default();
walk_body(&mut counter, &ast.body);
println!("{} calls", counter.calls.len());
```

Run it on `python/calls.py`. Compare with the hand-written attempt from object
2, example 2 — the visitor should find strictly more, including the calls inside
the f-string, the comprehension and the default arguments.

Then **delete the `walk_expr` line** and run again. The count collapses to the
number of top-level expression statements. That is the failure mode; see it once.

---

## Example 2 — the scope scanner (the real thing)

```rust
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_expr};
use ruff_python_ast::{AnyNodeRef, Decorator, Expr};
use ruff_text_size::{Ranged, TextRange};
use rustc_hash::FxHashSet;

struct ScopeScanner<'a> {
    root: TextRange,
    seen: FxHashSet<TextRange>,
    /// Definitions found in this scope — to be recursed into separately.
    defs: Vec<AnyNodeRef<'a>>,
    /// Calls found in this scope.
    calls: Vec<&'a Expr>,
}

impl<'a> SourceOrderVisitor<'a> for ScopeScanner<'a> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        if node.range() == self.root {
            return TraversalSignal::Traverse;
        }
        match node {
            AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::StmtClassDef(_) => {
                self.defs.push(node);          // remember it, recurse later
                TraversalSignal::Skip          // quirk: do not descend here
            }
            AnyNodeRef::ExprLambda(_) => TraversalSignal::Skip,   // quirk 8
            _ => TraversalSignal::Traverse,
        }
    }

    fn visit_decorator(&mut self, _d: &'a Decorator) {
        // PARITY: parso puts decorators outside the funcdef.
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if matches!(expr, Expr::Call(_)) && self.seen.insert(expr.range()) {
            self.calls.push(expr);
        }
        walk_expr(self, expr);
    }
}
```

Then the driver is a loop, not a recursive visitor:

```
scan(root):
    scanner = ScopeScanner { root: root.range(), … }
    walk the root
    for each def in scanner.defs:
        child_nodes = scan(def)        ← fresh scanner, one level down
```

**One visitor per scope.** That is the design, and it falls out of `Skip`.

---

## Exercise

**A.** Write `CallCounter` from example 1 and run it on all five fixtures. Record
the counts. Compare `calls.py` against the hand-written version from object 2 —
how many did the manual version miss, and which ones?

**B.** Delete `walk_expr(self, expr)` and record the new counts. Put it back.
This is the bug you must be able to recognise instantly.

**C.** Write `ScopeScanner` from example 2 and the recursive driver. Run it on
`python/nested.py`. Verify:
- `log()` is under `inner`, not under `outer`
- `Container.method`'s `log()` is under `method`
- `Inner.deep`'s `build()` is three levels down
- `with_blocks`'s `conditional` is **found** (a def inside an `if`)

**D.** Test quirk 8: on `python/edges.py`'s `has_lambda`, confirm your output has
exactly **one** call (`build()`) and that `log(x)` inside the lambda appears
nowhere. Then change `ExprLambda` to `Traverse` and watch it appear. Put it back.

**E.** Test the decorator skip: on `decorated_async`, confirm
`functools.wraps(build)` does **not** appear as a child. Then remove your
`visit_decorator` override and watch the phantom child appear.

**F.** Test the root guard: start a scanner at a `StmtFunctionDef` **without**
the `node.range() == self.root` check. What do you get? Explain why in one
sentence.

**G.** Add position dedup and construct a Python line that triggers it. (Hint:
you need two nodes with identical ranges — think about what a call chain shares,
and check whether your implementation actually collides. If it never fires on
real code, say so; a quirk that never triggers is still worth knowing about.)

---

## Exam

**1.** What is a "trait with default methods", and why does this one need them?

**2.** What does `walk_expr(self, expr)` do, and what happens if you forget it?
Why is that bug hard to notice?

**3.** Rust has no `super.method()`. How does this API give you the same thing?

**4.** What does `TraversalSignal::Skip` do, and which lines of `parser.py` does
it replace?

**5.** Why must `enter_node` special-case the visitor's own root? What do you get
if it does not?

**6.** Why compare ranges rather than `AnyNodeRef` values to detect the root?

**7.** What is `AnyNodeRef`, and why does `enter_node` take it rather than
`&Stmt`?

**8.** For quirk 8 (lambdas), why is "emit nothing" insufficient? What must you
also do?

**9.** How do you stop the walk entering `decorator_list`, and why is that
deliberately the same mistake as question 2?

**10.** Your scanner produces one visitor per scope rather than one visitor for
the file. What in the API makes that the natural design?

**11.** `CallFinder<'a>` holds `Vec<&'a ExprCall>`. What does the lifetime tie it
to, and what would you store instead if you needed the data to outlive the tree?

---

## Answers

**1.** A trait whose methods have bodies, so implementors override only what they
need. This one needs them because there are ~20 visit methods and 28 `walk_*`
helpers covering every construct in Python — nobody wants to implement all of
them to find calls.

**2.** It performs the default traversal of that expression's children. Forget
it and the walk **stops at that node** — you visit the expression but none of its
subexpressions.

Hard to notice because nothing errors and you still get *some* results. On
`python/calls.py` you would find the outer calls and silently miss every nested
one, and the tree would look plausible.

**3.** The default method body is exposed as a **free function** you call
explicitly: `walk_expr(self, expr)`. Rust traits have no inherited
implementation to call into, so the pattern is to make the default's body
callable by name.

**4.** It tells the walker not to descend into that node's children. It replaces
the early `return` at `parser.py:78-80`, which stopped `_scan_children` at a
nested `Class` or `Function` so their calls became children of *them*.

**5.** Because you start the scanner *at* a definition, and the rule says "skip
definitions". Without the guard you skip yourself and produce **nothing** — an
empty scope for every function in the file.

**6.** Because range equality is unambiguous and cheap (two `u32` comparisons on
`Copy` data), whereas comparing `AnyNodeRef` values depends on what `PartialEq`
is derived over at your revision — pointer identity, structural equality, or not
implemented at all. `plan/02-mapping/01` sketches `node == self.root`; ranges
remove the doubt.

**7.** An enum with a variant for every node type in the AST, holding a reference
to the concrete node. `enter_node` takes it because it is called for *every* kind
of node — statements, expressions, decorators, parameters, comprehensions — and
`&Stmt` could not represent most of those. It implements `Ranged`, so
`node.range()` always works.

**8.** Because "emit nothing and keep walking" still finds calls **inside** the
lambda body. parso drops the entire subtree (its `Lambda` is a `Function`
subclass, so `_visit_function` returns `None` and line 80 returns early). So you
must also `Skip`.

`python/edges.py`'s `has_lambda` distinguishes the two implementations: one call
in your output, or two.

**9.** Override `visit_decorator` with an **empty body** — deliberately not
calling `walk_decorator`. It is exactly the mistake from question 2, used on
purpose: here, stopping the traversal is the goal, because parso puts decorators
outside the funcdef.

Worth a `// PARITY:` comment, so the next reader knows the empty body is
intentional and not an unfinished stub.

**10.** `Skip`. Once a definition stops the traversal, the enclosing visitor
cannot see inside it — so producing that definition's children requires a
*second* walk starting there. One visitor per scope is not a choice you make; it
is what `Skip` leaves you with.

**11.** To the `ParsedModuleRef` the nodes live in — the visitor cannot outlive
the loaded tree. That is cheap and correct while you are walking.

If you need the data to outlive the tree — and your node tree does, because it
gets serialised after the borrow ends — store **owned** data: `TextRange`,
`String` names, your `Position` struct. Same rule as exercise 00 object 6 and
exercise 01 object 4: **lower to owned at the boundary.**
