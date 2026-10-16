# Object 5 — `SourceOrderVisitor` and `TraversalSignal`

Walking the tree without enumerating 94 node types by hand.

This object is four files, in [`05-object-visitor/`](05-object-visitor/).
Read them in order; each one assumes the one before it.

| part | file | covers |
|---|---|---|
| 1 | [How the walk works](05-object-visitor/01-how-the-walk-works.md) | the trait, the **three layers**, which walks fire `enter_node`, trait-with-defaults, source order vs evaluation order |
| 2 | [Stopping the walk](05-object-visitor/02-stopping-the-walk.md) | `Skip`, the root guard, `AnyNodeRef`, the four parity behaviours, the recursive driver |
| 3 | [Examples, exercises, exam](05-object-visitor/03-examples-and-exam.md) | `CallCounter`, `ScopeScanner`, 11 exercises, 16 questions with answers |
| 4 | [Wiring it up](05-object-visitor/04-wiring-it-up.md) | `Ctx`, `node_for_definition`, `nodes_for_call_chain`, both `main`s — **running code**, and the two bugs that only showed up when it ran |

---

## The short version

A **trait with default methods**: you implement the two or three you care about,
and the defaults walk everything else. Two hooks do the work.

```rust
impl<'a> SourceOrderVisitor<'a> for ScopeScanner<'a> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        // "should I descend into this?"  ← where parser.py's early return goes
    }
    fn visit_expr(&mut self, expr: &'a Expr) {
        // "I saw an expression"
        walk_expr(self, expr);        // ★★ forget this and the walk stops dead
    }
}
```

The five things that bite, each unpacked in the parts above:

1. **`enter_node` lives in `walk_*`, not in `visit_*`.** Override `visit_expr`
   without calling `walk_expr` and you silence *both* hooks for that node.
   (part 1)
2. **`walk_body` and `walk_annotation` fire no hooks at all**, and the four
   operator walks have empty bodies. A body is not a node. (part 1)
3. **`leave_node` runs even when you `Skip`** — so enter/leave stay balanced and
   a scope stack is safe. (part 1)
4. **`enter_node` fires for your own root too.** Skip definitions without a root
   guard and every function comes out empty. Compare **ranges**, not
   `AnyNodeRef`s — `==` on those is a deep structural compare. (part 2)
5. **`Skip` is the whole design.** It is why there is one visitor per scope, and
   why the driver re-enters with **`walk_node(def)`** — not `walk_body(&def.body)`,
   which silently drops every call in a default argument. (part 2)

Everything is verified against rev `ac201b8`, with file:line citations inline.
Part 4's code is `src/scan.rs` and `src/bin/{scan,pylspt-dev}.rs` in this repo:
it compiles, runs on all five fixtures, and both binaries agree byte-for-byte.

---

## ⚠ Which of this the pipeline keeps

This object builds **one** visitor that collects defs and calls together,
because that is `parse_file`'s tree. [File 8](08-staged-pipeline.md) splits that
job in three, and the split moves most of this page:

| | where it lives after the split |
|---|---|
| `SourceOrderVisitor`, the three layers, the `walk_*` rule | everywhere — this is how you read any ruff traversal |
| `enter_node` / `Skip`, the root guard, `walk_node` | the `parse_file` tree (`src/scan.rs`), unchanged |
| the four parity behaviours (lambdas, decorators, defaults, nesting) | **stage 3**, restated as what `eval` does with a `def` or a `lambda` |
| collecting calls into `Vec<&ExprCall>` | **nowhere** — the interpreter finds a call by reaching it |
| finding defs for the ID pipeline | **stage 1**, with `StatementVisitor` instead: five methods, no expressions |

Read all four parts first anyway. Stage 3 is these rules with a different
mechanism, and the rules are the hard part.
