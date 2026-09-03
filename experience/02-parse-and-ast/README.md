# 02 — Parse and AST

**Goal:** for any Python file, you emit the nested node tree your `parse_file`
RPC returns — classes, functions and calls, with correct names, positions,
`call_index` and `call_col_pos`.

This is the port of `parser.py` (184 lines of parso walking) and the largest of
the early exercises. It is also where you discover that "same output" and "same
code" are very different targets.

---

## How this exercise is organised

Six objects, one file each, then the assembly. Same shape as before: lesson →
API → worked examples → exercise → its own exam → answers.

| | file | object | what it gives you |
|---|---|---|---|
| 1 | [`01-object-parsedmodule.md`](01-object-parsedmodule.md) | `parsed_module`, `ParsedModuleRef`, `ModModule` | the tree, and why you must not parse yourself |
| 2 | [`02-object-stmt-expr.md`](02-object-stmt-expr.md) | `Stmt`, `Expr` | ★ the two enums everything is made of |
| 3 | [`03-object-functiondef-classdef.md`](03-object-functiondef-classdef.md) | `StmtFunctionDef`, `StmtClassDef` | ⚠ the decorator trap |
| 4 | [`04-object-exprcall.md`](04-object-exprcall.md) | `ExprCall`, `Arguments` | ★ calls, chains, `call_col_pos` |
| 5 | [`05-object-visitor.md`](05-object-visitor.md) | `SourceOrderVisitor`, `TraversalSignal` | ★ walking, and where to stop |
| 6 | [`06-object-docstrings.md`](06-object-docstrings.md) | docstrings, the `ID:` scan | the join key into v-noc |
| 7 | [`07-putting-it-together.md`](07-putting-it-together.md) | — | `src/nodes.rs`, the JSON, the quirk checklist |

Then [`exam.md`](exam.md) for the whole exercise.

**Files 2, 4 and 5 are the core.** File 2 teaches you to read any ty code; file 4
is the shape difference that produces `call_index`; file 5 is the machinery that
makes the walk correct rather than approximately correct.

---

## Read first

- `tutorial/05-parser-and-ast.md` — all of it
- `plan/02-mapping/01-parso-to-ruff-ast.md` — the mapping table and the call
  chain section
- `plan/00-orientation/01-what-you-have-today.md` §"Behavioural contract" —
  **quirks 4 and 8 through 12 are this exercise's specification**

---

## The mental model, in one page

### Two kinds of tree

parso and libcst give you a **CST** — a concrete syntax tree. Every token,
comment and space is a node; the tree prints back to the exact original bytes.
That is why your ID injector can rewrite one docstring and leave the file
otherwise untouched.

Ruff gives you an **AST** — abstract. Whitespace and comments are not nodes.
`x=1` and `x = 1` produce identical trees. **Ruff's AST cannot round-trip your
source**, which is exactly why exercise 10 still needs libcst.

For *reading* structure the AST is better in every way: typed fields instead of a
generic `children` list, no trivia to skip, and every node carries its
`TextRange` so you can always go back to the bytes.

```
parso                                ruff
─────                                ────
node.children  → [any node]          stmt.body: Vec<Stmt>       typed
node.type == "funcdef"               Stmt::FunctionDef(def)     matched
node.get_doc_node()                  body.first() is Expr(Str)  convention
whitespace is a node                 whitespace is nowhere
```

### The shape difference that matters: calls

parso gives a **flat** `atom_expr` whose children are trailers. Ruff gives a
**nested** expression, outermost first:

```
a.b().c()

ExprCall                              ← the OUTER call. parso called this index 1
├── func: ExprAttribute (.c)
│         └── value: ExprCall         ← the INNER call. parso called this index 0
│                    ├── func: ExprAttribute (.b)
│                    │         └── value: ExprName "a"
│                    └── arguments: ()
└── arguments: ()
```

So parso's "position in the trailer list" becomes **"depth from the innermost
call in the chain"**. File 4 works this through.

### The rule that stops the walk

`_scan_children` **returns immediately** on hitting a nested `Class` or
`Function` (`parser.py:78-80`). Calls inside a nested function are children of
*that* function, never of the enclosing one.

`TraversalSignal::Skip` expresses this directly — file 5.

---

## The two traps, up front

Both are things the plan marks `[check]`. Both are now checked.

### ⚠ Decorators are inside the range

`StmtFunctionDef.range` starts at the **`@`**, not at `def` **[verified]**. parso
starts at `def`, with decorators in a separate wrapping node.

And worse: `decorator_list` is a *field of the def*, so a naive walk puts calls
inside decorators **inside the function they decorate**. parso puts them in the
enclosing scope.

Both halves are file 3. On a codebase full of `@property` and `@dataclass` this
is most of your methods, not an edge case.

### ✅ `async def` is already right

`parser.py:126`'s special case **disappears** — ruff's range already starts at
the `async` keyword **[verified]**. File 3 again.

Same mechanism, opposite outcomes. Neither is "better"; the contract is parso's
output.

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

Each has a specific job:

| fixture | what it decides |
|---|---|
| `calls.py` | is your chain flattening right, and do you keep walking into arguments |
| `nested.py` | does every call land under the right owner |
| `edges.py` | decorators, async, lambdas — the four parity traps |
| `docstrings.py` | does your ID scan match `re.findall` + `dict` (last-wins) |

---

## Before you start

You need exercise 01's `src/position.rs` — every node carries a `Position`. And
the crate split from exercise 00, file 09.

---

## Done when

- [ ] you can dump any file's AST and read it
- [ ] your node tree for `nested.py` puts every call under the right owner
- [ ] `call_index` and `call_col_pos` are right for all of `calls.py`
- [ ] no lambda body appears anywhere in your output
- [ ] no decorator's calls appear inside the function they decorate
- [ ] all 12 docstring forms extract the ID you expect (or `None` where correct)
- [ ] `id_like_text` gives you the UUID, not `"but"`
- [ ] you have written down the decorator-range and `multiline_callee` decisions
- [ ] you can point at the code implementing quirks 4, 8, 9, 10 and 12

---

→ Start: [`01-object-parsedmodule.md`](01-object-parsedmodule.md)
