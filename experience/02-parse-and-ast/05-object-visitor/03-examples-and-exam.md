# Object 5 · Part 3 — Examples, exercises, exam

Two visitors to write, ten exercises to run, sixteen questions to answer.

**Part 3 of 3** · prev: [Part 2 — Stopping the walk](02-stopping-the-walk.md) ·
[Part 1 — How the walk works](01-how-the-walk-works.md)

---

## Example 1 — count calls, correctly

```rust
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, walk_body, walk_expr};
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

// usage — note: walk_body is a FREE FUNCTION. There is also a `visit_body`
// METHOD on the trait (source_order.rs:145) whose default calls it; override
// that if you want a hook per suite. Neither one fires enter_node.
let mut counter = CallCounter::default();
walk_body(&mut counter, &parsed.syntax().body);
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
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal, walk_expr, walk_node,
};
use ruff_python_ast::{AnyNodeRef, Decorator, Expr, ExprCall};
use ruff_text_size::{Ranged, TextRange};
use rustc_hash::FxHashSet;

struct ScopeScanner<'a> {
    root: TextRange,
    seen: FxHashSet<TextRange>,
    /// Definitions found in this scope — to be recursed into separately.
    defs: Vec<AnyNodeRef<'a>>,
    /// Calls found in this scope.
    calls: Vec<&'a ExprCall>,
}

impl<'a> SourceOrderVisitor<'a> for ScopeScanner<'a> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        if node.range() == self.root {
            return TraversalSignal::Traverse;              // this is me; go in
        }
        match node {
            AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::StmtClassDef(_) => {
                self.defs.push(node);                      // remember it, recurse later
                TraversalSignal::Skip                      // parser.py:78-80
            }
            AnyNodeRef::ExprLambda(_) => TraversalSignal::Skip,   // quirk 8: drop the subtree
            _ => TraversalSignal::Traverse,                // ★ identifiers, args, everything
        }
    }

    fn visit_decorator(&mut self, _d: &'a Decorator) {
        // PARITY: parso puts decorators outside the funcdef. Deliberately does
        // not call walk_decorator, so neither this node nor its subtree is seen.
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            if self.seen.insert(call.range()) {            // quirk 10
                self.calls.push(call);
            }
        }
        walk_expr(self, expr);                             // ★★ KEEP GOING
    }
}
```

Two details in that code that are easy to get wrong:

- **`calls: Vec<&'a ExprCall>`, not `Vec<&'a Expr>`.** You want the concrete
  node — object 4's `flatten_call_chain`, `call.arguments.range.start()` and
  `call.func` all need `ExprCall`. Bind it in the `if let` and store it.
- **`call.range()`, not `call.range`.** `ExprCall` has no `range` field; the
  range is computed by the `Ranged` impl (object 4). You need
  `use ruff_text_size::Ranged;` in scope for the dedup key to compile at all.

---

## Exercise

**A.** Write `CallCounter` from example 1 and run it on all five fixtures. Record
the counts. Compare `calls.py` against the hand-written version from object 2 —
how many did the manual version miss, and which ones?

**B.** Delete `walk_expr(self, expr)` and record the new counts. Put it back.
This is the bug you must be able to recognise instantly.

**C.** Write `ScopeScanner` from example 2 and the recursive driver, entered with
`walk_node`. Run it on `python/nested.py`. Verify:
- `log()` is under `inner`, not under `outer`
- `Container.method`'s `log()` is under `method`
- `Inner.deep`'s `build()` is three levels down
- `with_blocks`'s `conditional` is **found** (a def inside an `if`)

**D.** Test quirk 8: on `python/edges.py`'s `has_lambda`, confirm your output has
exactly **one** call (`build()`) and that `log(x)` inside the lambda appears
nowhere. Then change `ExprLambda` to `Traverse` and watch it appear. Put it back.

**E.** Test the decorator skip: on `decorated_async`, confirm
`functools.wraps(build)` does **not** appear as a child. Then remove your
`visit_decorator` override and watch the phantom child appear. Confirm
separately that the def's `.range` still starts at the `@` either way — the
override fixes children, not ranges.

**F.** Test the root guard: start a scanner at a `StmtFunctionDef` **without**
the `node.range() == self.root` check. What do you get? Explain why in one
sentence.

**G.** Add position dedup and try to trigger it. (Hint: you need two emissions
with identical ranges — think about what a call chain shares, and about
`Stmt::Expr` versus its inner expression. If it never fires on real code, say
so; a quirk that never triggers is still worth knowing about.)

**H.** Add `leave_node` and a `depth: usize` field; increment in `enter_node`,
decrement in `leave_node`, and print the tree indented. Now return `Skip` for
`ExprLambda` and confirm the depth still returns to zero at the end — that is
the "leave fires even on Skip" guarantee, and it is what makes enter/leave safe
for scope stacks.

**I.** Run `default_args` from `edges.py` through the scanner twice: once as a
root, once as a child of the module. Confirm `build()` and `log()` attach to
`default_args` in the first case and to nothing in the second. Explain in one
sentence why no special case was needed.

**J.** Swap `SourceOrderVisitor` for `ruff_python_ast::visitor::Visitor` in
`CallCounter` (drop `enter_node`; the method names match). Run it on
`comprehensions` in `edges.py` and on a `[f() for _ in g()]` of your own.
Does the count change? Does the **order** change? Which of the two would have
been a silent parity bug?

---

## Exam

**1.** What is a "trait with default methods", and why does this one need them?

**2.** Name the three layers a single traversal step passes through, and say
which one `enter_node` lives in.

**3.** What does `walk_expr(self, expr)` do, and what happens if you forget it?
Why is that bug hard to notice? What *second* hook does forgetting it also
silence?

**4.** Rust has no `super.method()`. How does this API give you the same thing?

**5.** Which `walk_*` functions do **not** call `enter_node`, and why is that not
an oversight in each case?

**6.** Does `leave_node` fire for a node whose `enter_node` returned `Skip`?
What does the answer let you build?

**7.** What does `TraversalSignal::Skip` do, and which lines of `parser.py` does
it replace? List everything it suppresses on a `StmtFunctionDef`.

**8.** Why must `enter_node` special-case the visitor's own root? What do you get
if it does not?

**9.** You have three ways to test "is this node my root": `==`, `ptr_eq`, and
range equality. What does each actually compare, and which is wrong?

**10.** What is `AnyNodeRef`, why is it described as "flattened", and why does
`enter_node` take it rather than `&Stmt`?

**11.** For quirk 8 (lambdas), why is "emit nothing" insufficient? What must you
also do, and how does that differ from what you do at a nested `def`?

**12.** How do you stop the walk entering `decorator_list`, and why is that
deliberately the same mistake as question 3?

**13.** Calls in default arguments — whose children are they, and what code in
ruff decides that?

**14.** Your scanner produces one visitor per scope rather than one visitor for
the file. What in the API makes that the natural design, and which function do
you use to start the second walk?

**15.** `CallFinder<'a>` holds `Vec<&'a ExprCall>`. What does the lifetime tie it
to, and what would you store instead if you needed the data to outlive the tree?

**16.** There are two visitor traits in `ruff_python_ast::visitor`. What is the
difference, and what would picking the wrong one do to your output?

---

## Answers

**1.** A trait whose methods have bodies, so implementors override only what they
need. This one needs them because there are 33 visit methods and 34 `walk_*`
helpers covering every construct in Python — nobody wants to implement all of
them to find calls.

**2.** Layer 3 `visit_X` (your override, default = call layer 1) → layer 1
`walk_X` (the free function: `enter_node` → dispatch → `leave_node`) → layer 2
`X::visit_source_order` (generated, `pub(crate)`, one `visitor.visit_*` per
field in source order) → back to layer 3 one level down. **`enter_node` lives in
layer 1**, inside `walk_X`.

**3.** It performs the default traversal of that expression's children. Forget
it and the walk **stops at that node** — you visit the expression but none of
its subexpressions.

Hard to notice because nothing errors and you still get *some* results. On
`python/calls.py` you would find the outer calls and silently miss every nested
one, and the tree would look plausible.

It also silences **`enter_node` and `leave_node`** for that node, because both
are called from inside `walk_expr`. So a `visit_expr` override that skips
`walk_expr` disables your `Skip` logic for that node too.

**4.** The default method body is exposed as a **free function** you call
explicitly: `walk_expr(self, expr)`. Rust traits have no inherited
implementation to call into, so the pattern is to make the default's body
callable by name — and the default itself just calls it.

**5.** `walk_body` (`for stmt in body { visit_stmt(stmt) }`), `walk_annotation`
(a pure forward to `visit_expr`), and `walk_bool_op` / `walk_operator` /
`walk_unary_op` / `walk_cmp_op` (empty bodies).

Not an oversight: a `&[Stmt]` body and "this expression is an annotation" are
not nodes — they have no `AnyNodeRef` variant to hand `enter_node`. The four
operator walks take a `BoolOp`/`Operator`/`UnaryOp`/`CmpOp`, which are plain
C-like enums with no range and no children, so there is nothing to enter.

**6.** **Yes.** `leave_node` is outside the `if`, so enter/leave are always
balanced. That lets you maintain a depth counter, an indentation level, or a
scope stack in the two hooks without special-casing skips.

**7.** It tells the walker not to descend into that node's children — it skips
the whole of layer 2 for that node. It replaces the early `return` at
`parser.py:78-80`, which stopped `_scan_children` at a nested `Class` or
`Function` so their calls became children of *them*.

On a `StmtFunctionDef` it suppresses, in one signal: the decorator list, the
name `Identifier`, type params, the parameter list (and therefore every
default-argument expression), the return annotation, and the body.

**8.** Because you start the scanner *at* a definition, and the rule says "skip
definitions". Without the guard you skip yourself and produce **nothing** — an
empty scope for every function in the file.

**9.** `==` is the derived `PartialEq` on `AnyNodeRef` (`generated.rs:5829`),
which compares the *pointed-to values* structurally and recursively: O(subtree),
and `true` for two distinct but identical sibling functions. **That is the wrong
one**, and it is the one `plan/02-mapping/01` sketches. `ptr_eq`
(`node.rs:584`) compares pointer and kind — O(1), referential, correct.
`node.range() == self.root` compares two `u32`s on `Copy` data — O(1), correct,
and lets you store a `TextRange` instead of an `AnyNodeRef`. Use ranges.

**10.** An enum with a variant for every node type in the AST (94 at `ac201b8`),
holding a reference to the concrete node. **"Flattened"** means there is no
`Stmt`/`Expr` wrapper variant: `AnyNodeRef::from(&Stmt)` yields
`AnyNodeRef::StmtFunctionDef(..)` directly, which is why `enter_node` can match
concrete kinds in one level.

`enter_node` takes it because it is called for *every* kind of node —
statements, expressions, decorators, parameters, keywords, comprehensions,
identifiers, f-string elements — and `&Stmt` could not represent most of those.
It is `Copy` and implements `Ranged`, so passing it by value and calling
`node.range()` are both free.

**11.** Because "emit nothing and keep walking" still finds calls **inside** the
lambda body: `ExprLambda::visit_source_order` visits `parameters` then `body`,
and the call is in the body. parso drops the entire subtree (its `Lambda` is a
`Function` subclass, so `_visit_function` returns `None` and line 80 returns
early). So you must also `Skip`.

The difference from a nested `def`: both get `Skip`, but a `def` is **recorded**
in `defs` and walked again as its own scope, while a lambda is recorded nowhere
and never walked. Same signal, opposite bookkeeping.

`python/edges.py`'s `has_lambda` distinguishes the two implementations: one call
in your output, or two.

**12.** Override `visit_decorator` with an **empty body** — deliberately not
calling `walk_decorator`. It is exactly the mistake from question 3, used on
purpose: here, stopping the traversal is the goal, because parso puts decorators
outside the funcdef. It also suppresses `enter_node`/`leave_node` for the
`Decorator` and its subtree, which is likewise intended.

Worth a `// PARITY:` comment, so the next reader knows the empty body is
intentional and not an unfinished stub. And note it does not touch
`StmtFunctionDef.range`, which still starts at the `@` — that is object 3's
separate fix.

**13.** The **function's**. `parameters` is a field of `StmtFunctionDef`, visited
by `StmtFunctionDef::visit_source_order` between the name and the body
(`generated.rs:10107`), and `ParameterWithDefault::visit_source_order`
(`node.rs:373`) visits the parameter then its default expression. parso nests
them the same way, so this needs no special case — unlike decorators, which the
parser places inside the range but parso places outside.

**14.** `Skip`. Once a definition stops the traversal, the enclosing visitor
cannot see inside it — so producing that definition's children requires a
*second* walk starting there. One visitor per scope is not a choice you make; it
is what `Skip` leaves you with.

You start that second walk with **`walk_node(&mut scanner, node)`**, which takes
an `AnyNodeRef` directly — which is why `defs` stores `AnyNodeRef<'a>` rather
than `&Stmt`.

**15.** To the `ParsedModuleRef` the nodes live in — the visitor cannot outlive
the loaded tree. That is cheap and correct while you are walking.

If you need the data to outlive the tree — and your node tree does, because it
gets serialised after the borrow ends — store **owned** data: `TextRange`,
`String` names, your `Position` struct. Same rule as exercise 00 object 6 and
exercise 01 object 4: **lower to owned at the boundary.**

**16.** `visitor::source_order::SourceOrderVisitor` visits children as written;
`visitor::Visitor` visits them in evaluation order, has no
`enter_node`/`leave_node`, and does not visit `Identifier`s.

Picking `Visitor` would keep the same *set* of calls but permute their **order**
wherever the two disagree — most visibly comprehensions, where source order is
`elt` then `generators` (`generated.rs:10699`) and evaluation order is
`generators` then `elt` (`visitor.rs:447`). Since your `call_index` and your
output ordering are positional and parso reported written order, that is a
silent parity bug that would present as bad numbering rather than as a wrong
visitor. Losing `enter_node` would also cost you the entire `Skip` design.
