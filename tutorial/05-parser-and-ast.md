# 5. The parser and the AST

How text becomes a tree, and how Ruff's tree differs from parso's.

---

## Two kinds of tree

There are two ways to make a syntax tree.

**A concrete tree (CST)** keeps *everything*: spaces, comments, blank lines,
even which quote style you used. You can turn it back into the exact original
text. parso and libcst both do this.

**An abstract tree (AST)** keeps only the *meaning*. It throws away spaces and
comments. You cannot rebuild the original text from it exactly.

| | parso / libcst | Ruff |
|---|---|---|
| Kind | concrete (CST) | abstract (AST) |
| Keeps whitespace | yes | no |
| Keeps comments | yes | no (they are in the token list) |
| Can rebuild source exactly | **yes** | **no** |
| Speed | slower | much faster |
| Memory | larger | smaller |

> **This matters for your ID injection.** Your `id_injector.py` works because
> libcst can rebuild the file perfectly. Ruff cannot do that — if you build the
> text back from the AST, you would reformat the user's whole file.
>
> **The answer is simple: keep using libcst.** It has a Rust version on
> crates.io (the crate is `libcst`, the module is `libcst_native`), and **ruff
> itself already depends on it** for the same reason:
>
> ```toml
> # ruff/Cargo.toml:132
> libcst = { version = "1.8.4", default-features = false }
> ```
>
> So the rule is: **use ruff's AST to read, use libcst to rewrite.** Ruff's
> parser is fast and cached, so it does all the analysis. libcst is slower but
> lossless, so it does the small number of file rewrites. Each tool does the job
> it is good at.
>
> Details in [`plan/02-mapping/02`](../plan/02-mapping/02-id-injection.md).

---

## Parsing a file

```rust
use ruff_python_parser::{parse_module, parse_unchecked, Mode, ParseOptions};

// simple form
let parsed = parse_module(source)?;

// form that never fails (gives a partial tree on bad syntax)
let parsed = parse_unchecked(source, ParseOptions::from(Mode::Module));
```

`Parsed<T>` gives you four things:

```rust
parsed.syntax()                     // the tree itself
parsed.tokens()                     // the flat token list (comments live here)
parsed.errors()                     // syntax errors
parsed.unsupported_syntax_errors()  // "this needs Python 3.12"
```

### Error recovery

If the file has a syntax error, Ruff still gives you a tree. It just marks the
broken part. This matters for a language server, where the user is typing and
the file is broken half the time.

Your driver must never crash on a bad file. Use `parse_unchecked` and check
`errors()` if you care.

### ⚠ The version trap

```rust
parse_module(source)                            // ← targets Python 3.10!
ParseOptions::from(Mode::Module)                // ← targets Python 3.10!
```

Both quietly use `PythonVersion::default()`, which is **3.10**. Meanwhile ty
defaults to **3.14**.

Always say the version:

```rust
ParseOptions::from(Mode::Module).with_target_version(version)
```

Or better: do not parse yourself at all. Ask the database:

```rust
let parsed = parsed_module(db, python_file).load(db);
```

That version is cached *and* uses the right Python version. Chapter 6 explains
the caching. The plan has a whole chapter on this trap
([`plan/01-crates/03`](../plan/01-crates/03-python-version.md)) because it is
easy to hit and hard to notice.

---

## The tree shape

Ruff's AST has two big enums:

- **`Stmt`** — statements. Things that *do* something.
- **`Expr`** — expressions. Things that *are* a value.

```rust
pub enum Stmt {
    FunctionDef(StmtFunctionDef),
    ClassDef(StmtClassDef),
    Return(StmtReturn),
    Assign(StmtAssign),
    If(StmtIf),
    For(StmtFor),
    // ... and more
}

pub enum Expr {
    Call(ExprCall),
    Name(ExprName),
    Attribute(ExprAttribute),
    Lambda(ExprLambda),
    NumberLiteral(ExprNumberLiteral),
    // ... 34 in total
}
```

### A real node, field by field

```rust
pub struct StmtFunctionDef {
    pub node_index: AtomicNodeIndex,      // internal id, ignore it
    pub range: TextRange,                 // where it is
    pub is_async: bool,                   // ← `async def` is a FLAG, not a wrapper
    pub decorator_list: ThinVec<Decorator>,
    pub name: Identifier,                 // the function's name
    pub type_params: Option<Box<TypeParams>>,   // def f[T]() — Python 3.12+
    pub parameters: Box<Parameters>,
    pub returns: Option<Box<Expr>>,       // the `-> int` part
    pub body: ThinVec<Stmt>,              // the statements inside
}
```

Note `is_async: bool`. In parso, `async def` is wrapped in an `async_stmt`
node, which is why your `parser.py:126` has a special case:

```python
if node.parent and node.parent.type == "async_stmt":
    position = self._get_position(node.parent)
```

In Ruff there is no wrapper. The `range` already covers the `async` keyword.
**That special case just disappears.** (Confirm with one test, but this is the
expected behaviour.)

---

## The biggest shape difference: calls

This is the one thing that will actually change your code.

### parso: flat

parso gives you an `atom_expr` with a list of "trailers":

```
a.b().c(1)

atom_expr
├── name "a"
├── trailer ".b"
├── trailer "()"      ← call trailer 1
├── trailer ".c"
└── trailer "(1)"     ← call trailer 2
```

Your `_visit_call` walks that list and numbers the calls 0, 1, 2 as it goes.

### Ruff: nested

Ruff builds a tree, inside out:

```
a.b().c(1)

ExprCall                          ← the OUTER call: .c(1)
├── func: ExprAttribute
│   ├── value: ExprCall           ← the INNER call: .b()
│   │   ├── func: ExprAttribute
│   │   │   ├── value: ExprName "a"
│   │   │   └── attr: "b"
│   │   └── arguments: ()
│   └── attr: "c"
└── arguments: (1)
```

So the *first* call in source order is the *deepest* node in the tree.

**What this means for `call_index`:** in parso, `call_index` is "position in the
trailer list". In Ruff, it becomes "how deep from the innermost call". To get
the same numbers, you walk the chain inward, then reverse:

```rust
fn flatten_call_chain(outer: &ExprCall) -> Vec<&ExprCall> {
    let mut chain = Vec::new();
    let mut current = Some(outer);
    while let Some(call) = current {
        chain.push(call);
        current = match call.func.as_ref() {
            Expr::Call(inner) => Some(inner),
            Expr::Attribute(a) => match a.value.as_ref() {
                Expr::Call(inner) => Some(inner),
                _ => None,
            },
            _ => None,
        };
    }
    chain.reverse();          // now index 0 is the innermost = first in source
    chain
}
```

Test this against real files. It is the most likely place for your node output
to differ from the Python driver.

---

## Walking the tree

parso gives every node a `.children` list, so you can write one recursive
function. Ruff does not — each node has its own named fields. So you use a
visitor.

```rust
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal, walk_expr, walk_body,
};

struct CallFinder<'a> {
    calls: Vec<&'a ast::ExprCall>,
}

impl<'a> SourceOrderVisitor<'a> for CallFinder<'a> {
    // Decide whether to go into a node at all.
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        match node {
            // Do not walk into nested functions or classes.
            // This is your parser.py:78-80 rule.
            AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::StmtClassDef(_) => {
                TraversalSignal::Skip
            }
            _ => TraversalSignal::Traverse,
        }
    }

    fn visit_expr(&mut self, expr: &'a ast::Expr) {
        if let ast::Expr::Call(call) = expr {
            self.calls.push(call);
        }
        walk_expr(self, expr);      // ← do not forget this
    }
}

// use it
let mut finder = CallFinder { calls: Vec::new() };
walk_body(&mut finder, &function.body);
```

Two rules, repeated because they matter:

1. **Call `walk_expr` (or `walk_stmt`) yourself**, or the walk stops.
2. **`enter_node` returning `Skip` stops the descent** at that node.

---

## Docstrings

parso has `node.get_doc_node()`. Ruff does not. A docstring is just the first
statement, if it happens to be a string:

```rust
fn docstring(body: &[Stmt]) -> Option<&ExprStringLiteral> {
    match body.first()? {
        Stmt::Expr(e) => e.value.as_string_literal_expr(),
        _ => None,
    }
}
```

Then read the text:

```rust
let text: &str = string_literal.value.to_str();
```

This is actually **better** than your Python. Your code does:

```python
# parser.py:44-47
if val.startswith('"""') or val.startswith("'''"):
    val = val[3:-3]
elif val.startswith('"') or val.startswith("'"):
    val = val[1:-1]
```

That breaks on `r"""..."""` (the `r` prefix is not stripped) and on strings
written in two pieces. Ruff's `.to_str()` handles all of it, because the parser
already did the work.

---

## Tokens (you will rarely need them)

There is a flat token list next to the tree:

```rust
let tokens: &Tokens = parsed.tokens();
```

Tokens are in order, each with a kind and a range. **Comments live only here**,
since the AST throws them away.

But before reaching for tokens, check whether a node range already answers your
question. Usually it does.

**Example — where is the `(` of a call?** Your `call_col_pos` field needs it.
You might reach for the token list. You do not have to. The `Arguments` node is
documented as spanning *"from the left to right parentheses (inclusive)"*, so:

```rust
let open_paren = call.arguments.range().start();     // that IS the `(`
```

One field access, no scanning. This works even with comments or line breaks
between the callee and the paren.

> **General lesson:** Ruff puts a lot of information into node ranges. When you
> think "I need to scan the source for a character", first check whether some
> node's range already points at it.

---

## Summary

| Task | parso | Ruff |
|---|---|---|
| parse | `parso.parse(src)` | `parsed_module(db, file)` |
| node kind | `node.type == "..."` (string) | `match` on an enum |
| children | `node.children` (list) | named fields + visitor |
| `async def` | wrapper node | `is_async: bool` |
| call chain | flat trailers | nested tree |
| docstring | `get_doc_node()` | first statement, if a string |
| comments | in the tree | in the token list |
| rebuild source | exact | **not possible** |

---

## Check yourself

1. Why can Ruff not rebuild the source text exactly?
2. What Python version does `parse_module(source)` assume?
3. In `a.b().c()`, which call is deeper in the Ruff tree?
4. What happens if `enter_node` returns `Skip` for `StmtFunctionDef`?
5. Where do comments live, if not in the AST?

---

→ Next: [`06-salsa-the-database.md`](06-salsa-the-database.md)
