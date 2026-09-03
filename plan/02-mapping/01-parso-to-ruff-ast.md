# 02.01 — `parser.py` → `ruff_python_ast`

Porting the 184-line parso walk. This is the easy half. Do it first — it gives
you a working `parse_file` RPC and a test harness before you touch inference.

---

## Concept mapping

| parso | ruff | Note |
|---|---|---|
| `parso.parse(src)` | `parsed_module(db, file).load(db)` | cached |
| `parso.python.tree.Class` | `ast::StmtClassDef` | |
| `parso.python.tree.Function` | `ast::StmtFunctionDef` | **covers `async def` too** |
| `lambdef` | `ast::ExprLambda` | separate node in ruff |
| `atom_expr` + call `trailer` | `ast::ExprCall { func, arguments }` | **structurally different — see below** |
| `node.start_pos` → `(line, col)` | `node.range().start()` → `TextSize` | byte offset |
| `node.children` | typed fields | no generic children list |
| manual recursion | `SourceOrderVisitor` | |
| `node.get_doc_node()` | first stmt is `Expr(StringLiteral)` | manual |

---

## The one real structural difference: calls

parso gives you a flat `atom_expr` whose children are trailers. Your
`_visit_call` iterates them, accumulating a `prefix_children` list to build the
name, and emits one `CallNode` per call-trailer with an incrementing
`call_index`.

Ruff gives you a **nested** tree. `a.b().c(1)` is:

```
ExprCall {                              ← outer, call_index 1
  func: ExprAttribute {
    value: ExprCall {                   ← inner, call_index 0
      func: ExprAttribute { value: Name("a"), attr: "b" },
      arguments: []
    },
    attr: "c"
  },
  arguments: [1]
}
```

So `call_index` — which in parso is "position in the trailer list" — becomes
**"depth from the innermost call in this chain"**. To reproduce your numbering:

```rust
/// Walk a call chain inside-out, mirroring parso's trailer order.
/// Returns calls with call_index 0,1,2… from innermost to outermost.
fn flatten_call_chain<'a>(outer: &'a ast::ExprCall) -> Vec<&'a ast::ExprCall> {
    let mut chain = Vec::new();
    let mut cur = Some(outer);
    while let Some(call) = cur {
        chain.push(call);
        // descend through the callee: f(...)(...)  and  f(...).attr(...)
        cur = match call.func.as_ref() {
            ast::Expr::Call(inner) => Some(inner),
            ast::Expr::Attribute(attr) => match attr.value.as_ref() {
                ast::Expr::Call(inner) => Some(inner),
                _ => None,
            },
            ast::Expr::Subscript(sub) => match sub.value.as_ref() {
                ast::Expr::Call(inner) => Some(inner),
                _ => None,
            },
            _ => None,
        };
    }
    chain.reverse();          // innermost first == call_index 0
    chain
}
```

**Verify this against real inputs.** Write a differential test: run the Python
`JediParser` and the Rust parser over the same 200 files, compare
`(name, line, column, end_line, end_column, call_index, call_col_pos)` tuples.
This is the single highest-value test in the project.

### The other three call fields

| Field | parso | ruff |
|---|---|---|
| `position` | `atom_expr.start_pos` .. `trailer.end_pos` | **`chain[0].range().start()` .. `this_call.range().end()`** — note the start is the *outermost chain's* start, i.e. `a` in `a.b().c()`, so all calls in a chain share `line`/`column` |
| `call_col_pos` | column of `(` | **`call.arguments.range().start()`** — no token scanning needed, see below |
| `name` | source text of the prefix children | source slice `&source[call.func.range()]`, then `.trim()` |

### `call_col_pos` is one field access

`Arguments` is documented as spanning **"from the left to right parentheses
(inclusive)"** **[verified, `ruff_python_ast/src/nodes.rs:3483`]**. So its start
*is* the `(`:

```rust
let open_paren: TextSize = call.arguments.range().start();
let call_col_pos = to_position(idx, source, TextRange::empty(open_paren)).column;
```

No token list, no `SimpleTokenizer`, no scanning past comments or line
continuations. This holds even for
`foo (  # comment\n  1)` — the `Arguments` range starts at the real `(`.

The same is true for class definitions: `class Foo(Bar):` has an `Arguments`
node covering `(Bar)`, which you get from `class_def.arguments`.

For `name`, your Python builds it from `_get_clean_code` which strips prefixes
(whitespace/comments). A raw source slice will include internal whitespace and
comments for multi-line callees like:

```python
obj \
  . method (x)
```

Rare, but decide: raw slice (fast, matches source) or normalised (matches the
old output). **Raw slice is right** — but note it as a known parity difference
and check whether v-noc downstream cares.

---

## The visitor

Your `_scan_children` has a distinctive rule: **stop at nested defs/classes**
(they become children of themselves, not of the enclosing scope). `TraversalSignal`
expresses this directly.

```rust
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal, walk_body, walk_stmt, walk_expr,
};

struct ScopeScanner<'a> {
    source: &'a str,
    line_index: &'a LineIndex,
    out: Vec<Node>,
    /// The scope node we started at — we descend into it, but not into siblings.
    root: AnyNodeRef<'a>,
}

impl<'a> SourceOrderVisitor<'a> for ScopeScanner<'a> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        if node == self.root {
            return TraversalSignal::Traverse;
        }
        match node {
            AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::StmtClassDef(_) => {
                // emit the node, recurse separately with a fresh scanner,
                // and do NOT walk into it here — mirrors parser.py:78-80
                TraversalSignal::Skip
            }
            _ => TraversalSignal::Traverse,
        }
    }

    fn visit_expr(&mut self, expr: &'a ast::Expr) {
        if let ast::Expr::Call(call) = expr {
            self.out.extend(self.visit_call_chain(call));
        }
        walk_expr(self, expr);   // keep descending: args can contain calls
    }
}
```

Two behaviours to carry over deliberately:

- **Lambdas dropped** (`parser.py:121`). In ruff, `ExprLambda` is an expression,
  so just never emit a node for it — but **do** walk into its body, since parso
  would have found calls there via the generic `children` walk. Check what your
  current code actually does here; `_scan_children` walks `lambdef` children
  because `isinstance(node, Function)` is `False` for parso lambdas... actually
  parso's `Lambda` *is* a `Function` subclass, so `_visit_node` is called,
  `_visit_function` returns `None`, and the node is dropped **along with its
  entire subtree** (the `return` at line 80). **So: skip lambda bodies entirely.**
  Match that.

- **Position dedup** (`parser.py:95-108`). Keep an `FxHashSet<TextRange>` and
  drop repeats. Cheaper in Rust: one `u64` key instead of a 4-tuple.

---

## Positions: the `LineIndex` bridge

Your wire format is **1-based line, 0-based column** (parso's convention).

```rust
use ruff_source_file::LineIndex;
use ruff_text_size::{Ranged, TextSize};

let line_index = LineIndex::from_source_text(source);

fn to_position(idx: &LineIndex, source: &str, range: TextRange) -> NodePosition {
    let start = idx.line_column(range.start(), source);
    let end   = idx.line_column(range.end(), source);
    NodePosition {
        line:       start.line.get(),        // OneIndexed → 1-based ✓
        column:     start.column.get() - 1,  // OneIndexed → 0-based ✓
        end_line:   end.line.get(),
        end_column: end.column.get() - 1,
    }
}
```

> ⚠ **Column semantics.** parso columns are *character* offsets within the line.
> Ruff's `LineIndex` can give you UTF-8 byte, UTF-16 code unit, or character
> columns depending on which method you call. For files with non-ASCII
> identifiers or string literals these differ. Check `LineIndex`'s available
> methods at your pinned revision and pick the *character* variant to match
> parso. **[check]** — this is a real parity bug generator; add a test with an
> emoji in a docstring above a function.

Build the `LineIndex` **once per file**, not per node.

---

## `async def` positions

`parser.py:126` uses the parent `async_stmt` range so the node starts at the
`async` keyword. In ruff, `StmtFunctionDef` has an `is_async: bool` field and
**its range already includes the `async` keyword** — there is no separate
wrapper node. So this special case simply disappears. **[check]** — verify with
one test; if ruff's range starts at `def`, prepend manually.

---

## Docstring / ID extraction

No `get_doc_node()`. The docstring is the first statement of the body if it is
a string expression:

```rust
fn docstring(body: &[ast::Stmt]) -> Option<&ast::ExprStringLiteral> {
    match body.first()? {
        ast::Stmt::Expr(e) => e.value.as_string_literal_expr(),
        _ => None,
    }
}
```

Then the ID regex. Your Python uses `ID:\s*([^\s]+)` on the *unquoted* value.
`ExprStringLiteral` gives you `.value.to_str()` — already unquoted and
concatenated across implicit adjacency. Better than your manual `[3:-3]`
slicing, which breaks on `r"""..."""` and on implicitly concatenated docstrings.

> `ty_python_core::definition::docstring_from_body` exists **[verified]** — it
> is `pub(crate)` to `ty_python_semantic`, but under Option A you can use it
> directly. Cheap win.

Use the `regex` crate with a `LazyLock` static, or just hand-roll `find("ID:")`
— it's faster and this is on the hot path for every def in the project.

---

## Deliverable for this chapter

`parse_file` RPC working end to end, returning byte-identical `nodes` JSON to
the Python driver for a corpus of ≥200 real files, with `resolve_mro=false`.

---

→ Next: [`02-id-injection.md`](02-id-injection.md)
