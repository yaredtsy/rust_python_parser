# Object 5 · Part 1 — How the walk works

`SourceOrderVisitor`, the three layers, and which hooks fire where.

**Part 1 of 3** · next: [Part 2 — Stopping the walk](02-stopping-the-walk.md) ·
[Part 3 — Examples, exercises, exam](03-examples-and-exam.md)

Walking the tree without enumerating 94 node types by hand.

This object is split across three files, because the visitor is the one piece of
ruff machinery you will be *inside* every day, and because most of its bugs are
silent. This part is the mechanism: what the trait is, what actually happens on
one traversal step, and which hooks fire where. Everything in all three parts is
read off
`ruff_python_ast/src/visitor/source_order.rs`, `src/node.rs` and
`src/generated.rs` at rev `ac201b8`.

---

## What it is

A **trait with default methods**. You implement the handful you care about;
the defaults walk everything else for you.

```rust
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal,
    walk_body, walk_expr, walk_stmt, walk_module, walk_node,
};

pub trait SourceOrderVisitor<'a> {
    #[inline]
    fn enter_node(&mut self, _node: AnyNodeRef<'a>) -> TraversalSignal {
        TraversalSignal::Traverse            // ← default: descend
    }
    #[inline(always)]
    fn leave_node(&mut self, _node: AnyNodeRef<'a>) {}

    fn visit_stmt(&mut self, stmt: &'a Stmt)    { walk_stmt(self, stmt) }
    fn visit_expr(&mut self, expr: &'a Expr)    { walk_expr(self, expr) }
    fn visit_body(&mut self, body: &'a [Stmt])  { walk_body(self, body) }
    fn visit_mod(&mut self, module: &'a Mod)    { walk_module(self, module) }
    fn visit_decorator(&mut self, d: &'a Decorator) { walk_decorator(self, d) }
    // …33 visit_* methods in total, all with walking defaults
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TraversalSignal { Traverse, Skip }

impl TraversalSignal {
    pub const fn is_traverse(self) -> bool { matches!(self, TraversalSignal::Traverse) }
}
```

**[verified]** `source_order.rs:13` (trait), `:14` (`enter_node`), `:19`
(`leave_node`), `:145` (`visit_body` — it **does** exist; an earlier draft of
this page said it did not), `:232` (`TraversalSignal`).

The trait has **33 `visit_*` methods** and the module exports **34 `walk_*`
free functions** **[verified]** — counted at `ac201b8`; treat both numbers as
"about thirty, one per construct", not as API you should memorise.

"Source order" means it visits nodes in the order they appear in the file —
which is what you want for `call_index` numbering and for output that reads
sensibly. There is a second visitor that does *not*; see
[Source order vs evaluation order](#source-order-vs-evaluation-order) below.

---

## ★★ How the walk actually works — three layers

This is the section the rest of the page depends on. If you only skim one
thing, skim the trace at the end of it.

Every traversal step passes through three distinct pieces of code:

```
  layer 3   visitor.visit_expr(expr)          ← YOUR override, or the default
                │  default body is:
                ▼
  layer 1   walk_expr(visitor, expr)          ← enter_node / leave_node live HERE
                │  if Traverse:
                ▼
  layer 2   ExprCall::visit_source_order(v)   ← generated, crate-private,
                │                                one visitor.visit_*() per field
                ▼
            visitor.visit_expr(func)          ← back to layer 3, one level down
            visitor.visit_arguments(args)
```

### Layer 1 — `walk_*`: the hook layer

Almost every `walk_*` function has exactly this body:

```rust
// source_order.rs:262   [verified]
pub fn walk_expr<'a, V>(visitor: &mut V, expr: &'a Expr)
where
    V: SourceOrderVisitor<'a> + ?Sized,
{
    let node = AnyNodeRef::from(expr);
    if visitor.enter_node(node).is_traverse() {
        match expr {
            Expr::BoolOp(expr) => expr.visit_source_order(visitor),
            Expr::Call(expr)   => expr.visit_source_order(visitor),
            …  // 33 arms, one per Expr variant
        }
    }
    visitor.leave_node(node);          // ★ runs even when you Skip
}
```

Three facts fall straight out of that body, and each one is a bug you will
otherwise write:

1. **`enter_node` is called by `walk_*`, not by `visit_*`.** If you override
   `visit_expr` and *don't* call `walk_expr`, `enter_node` never fires for that
   expression either. The two hooks are not independent.
2. **`leave_node` fires even when `enter_node` returned `Skip`.** Enter/leave
   are always balanced, so a depth counter or a scope stack built on them stays
   correct across skips. That is deliberate and worth relying on.
3. **The `match` is on the *outer* enum.** `AnyNodeRef::from(expr)` has already
   flattened `Expr::Call(c)` into `AnyNodeRef::ExprCall(c)` — which is why
   `enter_node` never sees `Stmt` or `Expr`, only the 94 concrete variants.

### Layer 2 — `visit_source_order`: the child order

Each node type has a generated (or, for a handful, hand-written) method listing
its children **in source order**:

```rust
// generated.rs:10827   [verified]
impl ExprCall {
    pub(crate) fn visit_source_order<'a, V>(&'a self, visitor: &mut V)
    where V: SourceOrderVisitor<'a> + ?Sized,
    {
        let ExprCall { range_start: _, func, arguments, node_index: _ } = self;
        visitor.visit_expr(func);
        visitor.visit_arguments(arguments);
    }
}
```

Note the destructuring-with-`_` idiom: it is how ruff guarantees the method is
updated when a field is added. Add a field to `ExprCall` and this stops
compiling until someone decides whether to visit it. Copy that idiom in your
own code when you exhaustively handle a struct.

These methods are **`pub(crate)`** — you cannot call them. Your entry points
are the `walk_*` free functions and `walk_node`.

Two child orders are worth knowing by heart, because they are where "source
order" earns its name:

```rust
// StmtFunctionDef — generated.rs:10107   [verified]
for elm in decorator_list { visitor.visit_decorator(elm); }   // ← ★ decorators FIRST
visitor.visit_identifier(name);
if let Some(tp) = type_params { visitor.visit_type_params(tp); }
visitor.visit_parameters(parameters);                          // ← ★ defaults live here
if let Some(r) = returns { visitor.visit_annotation(r); }
visitor.visit_body(body);
```

```rust
// Arguments — node.rs:324   [verified]
for arg_or_keyword in self.iter_source_order() {
    match arg_or_keyword {
        ArgOrKeyword::Arg(arg)      => visitor.visit_expr(arg),
        ArgOrKeyword::Keyword(kw)   => visitor.visit_keyword(kw),
    }
}
```

`Arguments` does **not** visit all positionals then all keywords — it
interleaves them by source position via `iter_source_order()`. So in
`f(a(), k=b(), *c())` you see `a()`, `b()`, `c()` in written order, which is
what your `call_index` numbering assumes.

### Layer 3 — `visit_*`: your override point

The default body is the `walk_*` call. That is the whole of the
"trait with defaults" pattern, and the next section unpacks it.

### The trace — `obj.f(1)` as a statement

Read this once, slowly. `│` depth is call depth.

```
walk_body(&[stmt])                       ← no enter_node: see the table below
└─ visit_stmt(Stmt::Expr)
   └─ walk_stmt
      ├─ enter_node(StmtExpr)            → Traverse
      ├─ StmtExpr::visit_source_order
      │  └─ visit_expr(Expr::Call)
      │     └─ walk_expr
      │        ├─ enter_node(ExprCall)   → Traverse      ★ your call hook
      │        ├─ ExprCall::visit_source_order
      │        │  ├─ visit_expr(Expr::Attribute)   → walk_expr
      │        │  │  ├─ enter_node(ExprAttribute)
      │        │  │  ├─ visit_expr(Expr::Name "obj") → enter/leave, no children
      │        │  │  ├─ visit_identifier("f")       → enter/leave
      │        │  │  └─ leave_node(ExprAttribute)
      │        │  └─ visit_arguments(Arguments)    → walk_arguments
      │        │     ├─ enter_node(Arguments)
      │        │     ├─ visit_expr(1)              → enter/leave
      │        │     └─ leave_node(Arguments)
      │        └─ leave_node(ExprCall)
      └─ leave_node(StmtExpr)
```

Two things to take from the trace:

- `visit_expr` fires for the **callee** (`obj.f`) as well as for the call. If
  your visitor counts `Expr::Call` it is unaffected, but if it collects *names*
  you will see the callee twice unless you `Skip` or guard.
- `Identifier` is a node. `enter_node` fires for `f` in `obj.f`, for a function's
  name, for a parameter's name, for an import alias. Any `match` in `enter_node`
  must have a `_ => Traverse` arm or you will accidentally skip identifiers.

---

## ⚠ Which walks fire the hooks, and which don't

This is not uniform, and the exceptions are load-bearing.

| walk function | `enter_node` / `leave_node`? | what it does |
|---|---|---|
| `walk_stmt`, `walk_expr`, `walk_decorator`, `walk_arguments`, `walk_parameters`, `walk_keyword`, `walk_comprehension`, `walk_identifier`, … | **yes** | enter → `visit_source_order` → leave |
| `walk_module` | **yes** | on `ModModule` / `ModExpression` |
| `walk_node` | **yes** | the generic one; takes `AnyNodeRef` |
| **`walk_body`** | **no** | `for stmt in body { visitor.visit_stmt(stmt) }` |
| **`walk_annotation`** | **no** | `visitor.visit_expr(expr)` — a pure forwarder |
| **`walk_bool_op`, `walk_operator`, `walk_unary_op`, `walk_cmp_op`** | **no** | empty bodies; the parameter is literally `_visitor` |

**[verified]** `source_order.rs:202` (`walk_body`), `:248` (`walk_annotation`),
`:526`–`:552` (the four no-ops).

Consequences you will actually hit:

- **A body is not a node.** `&[Stmt]` has no `AnyNodeRef` variant, so there is
  nothing for `enter_node` to receive. If you want "entering a suite" as an
  event, hook `visit_body` (layer 3) — not `enter_node`.
- **An annotation is not distinguishable in `enter_node`.** `-> int` and a
  parameter's `: int` arrive as an ordinary `ExprName`. If you must tell
  annotations apart from value expressions, override `visit_annotation` and set
  a flag on `self`, because by the time `enter_node` sees the node the context
  is gone. (For this tool you don't care: annotations can contain calls, and
  parso counted them, so letting them through is correct.)
- **Operators are invisible.** `walk_bool_op` and friends do nothing at all, so
  `+`, `and`, `not`, `<=` never reach `enter_node`. Their *operands* do, through
  the enclosing `ExprBinOp` / `ExprBoolOp` / `ExprCompare`. Fine for you; a
  surprise if you ever want token-level work.

---

## Why this solves object 2's problem

In object 2, example 2, you tried to find every call by hand and hit the wall:
33 `Expr` variants, and any of them can contain another expression. `BinOp` has
two children, `Compare` has a list, `FString` has elements, `Subscript` has
value and slice, comprehensions have generators and conditions.

The visitor already knows all of it — that is what layer 2 *is*: a hand-audited,
compiler-checked list of every child of every node in the language, refreshed
whenever the AST changes.

So your job shrinks to: *"tell me when you see a call, and let me decide when to
stop descending."* Two hooks, two lines each.

---

## Rust: the trait-with-defaults pattern

Worth understanding properly, because it is how most of ruff's extensible
machinery works (`Transformer`, `StatementVisitor`, the formatter's `Format`
traits all use it).

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

**1. You override one method.** Everything else uses the default, which walks.

**2. `walk_expr(self, expr)` is the "call super" step.** Rust has no
`super.visit_expr(expr)` — a trait's default body is not inherited code you can
reach from an override, it is simply replaced. So ruff publishes the default's
body as a free function and the default calls it too. **Forgetting it stops the
traversal dead** at that node — the single most common visitor bug, and it fails
silently.

**3. `&mut self`** — the visitor accumulates into itself. A walk returns `()`;
there is nowhere for a result to go but a field.

**4. `V: SourceOrderVisitor<'a> + ?Sized`.** Every `walk_*` is generic over the
visitor and relaxes the implicit `Sized` bound. Two consequences: each visitor
type gets its own monomorphised, fully-inlined copy of the walk (hence the
`#[inline]` on every trait method — this is a zero-cost abstraction, not a
vtable), **and** `&mut dyn SourceOrderVisitor` also satisfies the bound, so you
*can* box a visitor when you need dynamic dispatch. You won't here.

### Using it

```rust
let mut finder = CallFinder { calls: Vec::new() };
walk_body(&mut finder, &parsed.syntax().body);
println!("{} calls", finder.calls.len());
```

**Rust note — the `'a` lifetime.** `CallFinder<'a>` stores references *into the
AST*, so it cannot outlive the `ParsedModuleRef`. That is correct and free: you
are collecting pointers, not copying nodes. If the borrow checker complains
about the visitor outliving the tree, the fix is to collect owned data (ranges,
names) instead of node references — which is what your node tree does anyway.

**Rust note — `'a` appears twice and they are the same.** `impl<'a>
SourceOrderVisitor<'a> for CallFinder<'a>` ties the lifetime the trait hands you
(`&'a Expr`) to the lifetime your struct stores. Writing
`impl<'a, 'b> SourceOrderVisitor<'a> for CallFinder<'b>` compiles only if you
add `'a: 'b`, and there is no reason to.

---

## Source order vs evaluation order

`ruff_python_ast::visitor` exports **two** visitor traits, and they visit
children in different orders:

| | `visitor::source_order::SourceOrderVisitor` | `visitor::Visitor` |
|---|---|---|
| order | as written in the file | as evaluated at runtime |
| `enter_node` / `leave_node` | **yes** | **no** |
| visits `Identifier` | yes | no |
| entry points | `walk_module`, `walk_body`, `walk_stmt`, `walk_expr`, `walk_node` | `walk_stmt`, `walk_expr`, … |

The difference is not academic. For `[build() for _ in gen()]`:

```rust
// source order — generated.rs:10699   [verified]
visitor.visit_expr(elt);                                   // build()  first
for elm in generators { visitor.visit_comprehension(elm); } // gen()    second

// evaluation order — visitor.rs:447   [verified]
for comprehension in generators { visitor.visit_comprehension(comprehension); }
visitor.visit_expr(elt);                                   // reversed
```

Python really does evaluate `gen()` before `build()`; parso, and therefore your
Python implementation, reported them in **written** order. So
`SourceOrderVisitor` is not a stylistic preference here — picking the other one
silently permutes your output on every comprehension in the corpus, and the
diff would look like a numbering bug rather than a visitor choice.

`enter_node`/`leave_node` only existing on the source-order trait is the other
reason: the whole "stop at a nested def" design of
[part 2](02-stopping-the-walk.md) has no equivalent on `visitor::Visitor`.

---

**Next:** [Part 2 — Stopping the walk](02-stopping-the-walk.md), where
`enter_node` earns its keep.

---

