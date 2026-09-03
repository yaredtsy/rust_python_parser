# 02 — Parse and AST

**Goal:** for any Python file, you emit the nested node tree your `parse_file`
RPC returns — classes, functions and calls, with correct names, positions,
`call_index` and `call_col_pos`.

This is the port of `parser.py` (184 lines of parso walking), and it is the
largest of the early exercises. It is also the one where you will discover that
"same output" and "same code" are very different targets.

---

## Read first

- `tutorial/05-parser-and-ast.md` — all of it
- `plan/02-mapping/01-parso-to-ruff-ast.md` — the mapping table and the call
  chain section
- `plan/00-orientation/01-what-you-have-today.md` §"Behavioural contract" —
  quirks 8 through 12 are this exercise's specification

---

## The mental model

### Two kinds of tree

parso and libcst give you a **CST** — a concrete syntax tree. Every token,
comment and space is a node; the tree can be printed back to the exact original
bytes. That is why your ID injector can rewrite one docstring and leave the file
otherwise untouched.

Ruff gives you an **AST** — an abstract syntax tree. Whitespace and comments are
not nodes. `x=1` and `x = 1` produce identical trees. **Ruff's AST cannot
round-trip your source**, which is exactly why exercise 10 still needs libcst.

For *reading* structure, the AST is better in every way: typed fields instead of
a generic `children` list, no trivia to skip, and every node carries its
`TextRange` so you can always go back to the bytes.

```
parso                                ruff
─────                                ────
node.children  → [any node]          stmt.body: Vec<Stmt>       typed
node.type == "funcdef"               Stmt::FunctionDef(def)     matched
node.get_doc_node()                  body.first() is Expr(Str)  manual
whitespace is a node                 whitespace is nowhere
```

### The shape difference that matters: calls

This is the one real structural change, and it is where your `call_index` comes
from.

parso gives a **flat** `atom_expr` whose children are trailers:

```
atom_expr
├── Name "a"
├── trailer ".b"
├── trailer "()"      ← call_index 0
├── trailer ".c"
└── trailer "()"      ← call_index 1
```

Ruff gives a **nested** expression, outermost first:

```
a.b().c()

ExprCall                            ← the OUTER call.  parso called this index 1
├── func: ExprAttribute (.c)
│         └── value: ExprCall       ← the INNER call.  parso called this index 0
│                    ├── func: ExprAttribute (.b)
│                    │         └── value: ExprName "a"
│                    └── arguments: ()
└── arguments: ()
```

So parso's "position in the trailer list" becomes **"depth from the innermost
call in the chain"**. To reproduce the numbering you walk the chain inside-out:
follow `call.func` down through `Attribute`, `Call` and `Subscript` nodes,
collecting every `ExprCall`, then reverse. `plan/02-mapping/01` sketches
`flatten_call_chain` — write your own version and test it against
`python/calls.py`, which has a case for each descent kind.

The three call fields, and where each comes from:

| field | parso | ruff |
|---|---|---|
| `position` | `atom_expr.start_pos` .. `trailer.end_pos` | **`chain[0].range().start()`** .. `this_call.range().end()` — every call in one chain shares its start |
| `call_col_pos` | column of `(` | **`call.arguments.range().start()`** — one field access |
| `name` | text of the prefix children | source slice of `call.func.range()`, trimmed |

`call_col_pos` deserves a note. `Arguments` is documented as spanning **"from
the left to right parentheses (inclusive)"** **[verified,
`ruff_python_ast/src/nodes.rs:3483`]**, so its start *is* the `(`. No token
scanning, no `SimpleTokenizer`, and it stays correct through
`foo (  # comment\n  1)`. The same field exists on `StmtClassDef.arguments` for
`class Foo(Bar):`.

### The visitor, and the rule that stops it

`_scan_children` has a distinctive behaviour (`parser.py:78-80`): on reaching a
nested `Class` or `Function` it **returns immediately**. Calls inside a nested
function are children of *that* function, never of the enclosing one.

`TraversalSignal` expresses this directly:

```rust
fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
    // TraversalSignal::Traverse  → walk into this node
    // TraversalSignal::Skip      → do not walk into it
}
```

Emit the def/class node, return `Skip`, and recurse separately with a fresh
scanner rooted at that node. That produces the nesting your wire format wants
and matches parso's behaviour exactly.

---

## The API, verified at `ac201b8`

```rust
// getting the tree — the two-line prologue from the plan
use ruff_db::parsed::parsed_module;
let parsed = parsed_module(db, program_file.python_file(db)).load(db);
let ast: &ModModule = parsed.syntax();
// also: parsed.tokens(), parsed.errors(), parsed.unsupported_syntax_errors()

// ruff_python_ast
pub struct StmtFunctionDef {              // covers `async def` too
    pub range: TextRange,
    pub is_async: bool,
    pub decorator_list: ThinVec<Decorator>,
    pub name: Identifier,
    pub type_params: Option<Box<TypeParams>>,
    pub parameters: Box<Parameters>,
    pub returns: Option<Box<Expr>>,
    pub body: Vec<Stmt>,
}
pub struct StmtClassDef { range, decorator_list, name, type_params, arguments: Option<Box<Arguments>>, body }
pub struct ExprCall     { range, func: Box<Expr>, arguments: Arguments }
pub struct ExprLambda   { range, parameters, body }

// name range without the body — from the Identifier TRAIT
use ruff_python_ast::identifier::Identifier;
func_def.identifier() -> TextRange        // just the `greet` in `def greet(...)`

// the visitor
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal, walk_body, walk_stmt, walk_expr, walk_module,
};
pub trait SourceOrderVisitor<'a> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal { Traverse }
    fn leave_node(&mut self, node: AnyNodeRef<'a>) {}
    fn visit_stmt(&mut self, stmt: &'a Stmt) { walk_stmt(self, stmt) }
    fn visit_expr(&mut self, expr: &'a Expr) { walk_expr(self, expr) }
    // …and visit_mod, visit_annotation, visit_decorator, visit_parameters, …
}

// docstrings — no get_doc_node(); the docstring is just the first statement
match body.first() {
    Some(Stmt::Expr(e)) => e.value.as_string_literal_expr(),   // Option<&ExprStringLiteral>
    _ => None,
}
// then .value.to_str() — already unquoted, already concatenated across
// implicit adjacency, already correct for r"""…""" prefixes
```

> `.value.to_str()` is a real upgrade over your Python's `[3:-3]` slicing, which
> breaks on `r"""…"""` and on implicitly concatenated docstrings. `python/docstrings.py`
> has both cases so you can prove it.

---

## The two traps I can save you from

Both are things the plan marks `[check]`. I checked them; here are the answers,
with where to look so you can confirm.

### ⚠ Trap 1: decorators are inside the range

```python
@functools.cache
def decorated():
    ...
```

**In ruff, `StmtFunctionDef.range` starts at the `@`, not at `def`.**
**[verified]** — `parse_decorators` binds `start = self.node_start()` *before*
consuming the `@`, then passes that same `start` into
`parse_function_definition(decorators, start)`
(`ruff_python_parser/src/parser/statement.rs:2894, 3021`).

In parso, `Function.start_pos` is at `def`; decorators live in a wrapping
`decorated` node. **So decorated functions get a different `position` unless you
handle it.** On a codebase using `@property`, `@staticmethod` or
`@functools.cache` this is not an edge case — it is most of your methods.

Your options: use `decorator_list.last()` to find where the decorators end and
start from the following `def`/`async` token, or accept the difference and
document it. `python/edges.py` has the fixtures. **Decide deliberately** — this
is exactly the class of silent parity difference the M2 gate in
`plan/04-build/02-milestones.md` exists to catch.

### ✅ Trap 2 (not a trap): `async def` is already right

`parser.py:126` has a special case using the parent `async_stmt` position so the
range starts at the `async` keyword. In ruff there is no wrapper node — and
**the range already includes `async`** **[verified]**, because the parser passes
`async_start` into `parse_function_definition`
(`statement.rs:2853`).

So this special case simply disappears. The plan guessed it would; now you know.
Confirm with `plain_async` in `python/edges.py`.

---

## The fixtures

```
python/
├── helpers.py ....... the callees. every other file imports from it.
├── calls.py ......... 9 call shapes: chains, nesting, subscripts, f-strings
├── nested.py ........ scope nesting; defs inside if/try; classes in methods
├── edges.py ......... decorators, async, lambdas, one-liners, comprehensions
└── docstrings.py .... 12 docstring forms your ID extraction must survive
```

---

## Build it

### Step 1 — look at a tree before you write a walk

Before any visitor, print one. Take the smallest file you can and dump the AST
with `{:#?}` — every ruff AST node derives `Debug`.

```
cargo run -- dump python/edges.py | head -100
```

Read it. Find the `StmtFunctionDef` for `decorated` and look at where its
`range` starts. That is trap 1, and seeing it in the dump is worth more than
reading my paragraph about it.

**Keep this dump command.** You will use it in every later exercise, and it is
the fastest way to answer "what does that node actually look like".

### Step 2 — the node types

In `src/nodes.rs`, define the wire types from
`plan/00-orientation/01-what-you-have-today.md`:

```rust
pub struct Node {
    id: Option<String>,          // "FunctionSchema/<uuid>" | "ClassSchema/<uuid>"
    name: String,
    kind: NodeKind,              // Class | Function | Call
    position: Position,          // from exercise 01
    children: Vec<Node>,
    call_index: Option<usize>,   // calls only
    call_col_pos: Option<usize>, // calls only
    base_classes: Option<Vec<String>>,   // classes only, exercise 08
}
```

Derive `serde::Serialize` now with the right field names — matching the JSON is
the actual contract (see `MEMORY.md`), so let the type enforce it.

### Step 3 — the scanner

One visitor, applied recursively per scope:

- `enter_node`: if the node is the scanner's own root, `Traverse`. If it is a
  `StmtFunctionDef` or `StmtClassDef`, emit a node, recurse with a fresh scanner
  rooted there, and return `Skip`. Otherwise `Traverse`.
- `visit_expr`: on `Expr::Call`, flatten the chain and emit one node per call,
  then keep walking (arguments can contain more calls).
- Lambdas: emit nothing **and skip the subtree** — quirk 8. `log(x)` inside the
  lambda in `edges.py` must not appear anywhere in your output.
- Keep an `FxHashSet<TextRange>` for the position dedup — quirk 10.

Run against `python/nested.py`. The tree you get should have `log()` under
`inner`, not under `outer`, and `deep`'s `build()` three levels down.

### Step 4 — call chains

Implement chain flattening and test each case in `python/calls.py`:

| function | what it tests |
|---|---|
| `chained` | two calls, indices 0 and 1, sharing a start position |
| `deep_chain` | three calls |
| `nested_args` | calls in arguments are separate chains, not part of this one |
| `call_of_call` | `func` is itself `ExprCall` |
| `subscripted` | descent through `ExprSubscript` |
| `multiline_callee` | what is `name` when the callee spans lines? |

`multiline_callee` is the one worth thinking about. Your Python builds the name
from cleaned prefix children; a raw source slice gives you
`obj \\\n        . render` including the backslash, newline and spaces.
`plan/02-mapping/01` argues for the raw slice and flagging it as a known
difference. **Note which you chose and why** — you will need that note during
parity testing, when someone asks why one name has a newline in it.

### Step 5 — docstrings and IDs

Extract the docstring, then find `ID:` in it. Test every case in
`python/docstrings.py`.

Two that will catch a naive implementation:

- `not_first_statement` — the string is the *second* statement, so it is **not**
  a docstring. Its ID must not be picked up.
- `id_like_text` — the word "ID:" appears in prose before the real key.

Your Python uses `re.findall(r"(\S+)\s*:\s*(\S+)")` and takes pairs. Hand-roll
the scan in Rust; it is about twenty lines, it runs on every def in the project,
and a regex crate for this is not worth the dependency.

Remember quirk 4: **a def with no `ID:` produces no node in the call tree and is
not descended into.** For `parse_file` you still emit the node with `id: null`.
Two different rules for two different RPCs, and mixing them up produces a tree
that is missing subtrees for no visible reason.

### Step 6 — check yourself against the quirk list

Go through quirks 8–12 in `plan/00-orientation/01` and, for each, name the line
of your code that implements it. Any quirk you cannot point at is one you have
not implemented — and every one of them is observable in the JSON.

---

## Traps

- **Forgetting to walk into arguments.** `wrap(build(), key=build())` has two
  calls in its arguments. If `visit_expr` returns early on `Expr::Call` without
  calling `walk_expr`, you lose them silently.
- **Treating `ExprLambda` like a function.** parso's `Lambda` *is* a `Function`
  subclass, so `_visit_function` returns `None` and the whole subtree is
  dropped. Match that: skip the body.
- **Assuming the AST holds source text.** Names come from slicing the source
  with a node range, not from the node. `SourceCode::slice` from exercise 01.
- **`parse_module(source)` instead of `parsed_module(db, file)`.** Bypasses both
  the cache and the version wiring, and the semantic layer will later reject
  nodes from a module parsed at the wrong version. Exercise 04 shows the test
  that enforces this. Never parse a db file yourself.
- **Comparing `AnyNodeRef` with `==` and expecting identity.** Check what that
  comparison actually does at your revision before relying on it to detect "am I
  at my own root" — comparing ranges is unambiguous.

---

## Done when

- [ ] you can dump any file's AST and read it
- [ ] your node tree for `nested.py` puts every call under the right owner
- [ ] `call_index` and `call_col_pos` are right for all six cases in `calls.py`
- [ ] no lambda body appears anywhere in your output
- [ ] all 12 docstring forms extract the ID you expect (or `None` where correct)
- [ ] you have written down your decision on the decorator range
- [ ] you can point at the code implementing each of quirks 8–12

---

→ [`exam.md`](exam.md), then [`../03-the-database/README.md`](../03-the-database/README.md)
