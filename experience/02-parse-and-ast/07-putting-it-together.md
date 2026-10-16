# 07 — Putting it together: the node tree

Six objects, one module, and the first real output your driver produces.

> ### ⚠ This chapter builds the one-pass tree. The pipeline wants three passes.
>
> Everything below is correct and still ships: it is `parse_file`'s `nodes`
> array, defs and calls in one tree, from one traversal. It is also the *last*
> thing in this exercise that treats "defs and calls together" as the goal.
>
> [File 8](08-staged-pipeline.md) splits it:
>
> - **stage 1** collects defs only, as owned data in a salsa query per file —
>   `StatementVisitor`, no calls, no lambdas;
> - **stage 2** mints the missing IDs, writes them into the docstrings
>   bottom-up, and re-runs stage 1 because the write invalidated every range;
> - **stage 3** finds calls by *evaluating*, not by collecting — a call is found
>   when the interpreter's `eval` reaches an `ExprCall`.
>
> Read this chapter first: stage 3 is this chapter's `Skip` rules restated as
> evaluation semantics, and stage 1 is this chapter's scanner with five of its
> six mechanisms deleted. Then read file 8 to find out which of the code below
> the pipeline actually calls.

---

## What you are building

`parse_file`'s `nodes` array, from `plan/00-orientation/01`:

```jsonc
{
  "id": "FunctionSchema/<uuid>",   // or "ClassSchema/<uuid>"; null for calls
  "name": "do_thing",
  "type": "class" | "function" | "call",
  "position": { "line": 1, "column": 0, "end_line": 3, "end_column": 12 },
  "children": [ /* recursive */ ],

  // class only:
  "base_classes": ["pkg.mod.Base", "builtins.object"]   // exercise 08
}
```

```
ProgramFile
   │  parsed_module(...).load(db)          object 1
   ▼
ModModule ──body──►  Vec<Stmt>              object 2
   │
   ├── Stmt::FunctionDef / ClassDef         object 3   → a node, then RECURSE
   ├── Expr::Call                           object 4   → ONE node per call
   │
   │  walked by ScopeScanner                object 5
   │  docstring → ID                        object 6
   │  range → Position                      exercise 01
   ▼
Vec<Node>  ──serde_json──►  JSON
```

---

## The module

`src/nodes.rs`:

```rust
use crate::position::Position;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    /// "FunctionSchema/<uuid>" | "ClassSchema/<uuid>" | null for calls
    pub id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: NodeKind,
    pub position: Position,
    pub children: Vec<Node>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_classes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Class,
    Function,
    Call,
}
```

**Rust notes — the serde attributes are the contract:**

- `#[serde(rename = "type")]` — `type` is a Rust keyword, so the field is `kind`
  in Rust and `"type"` in JSON. **The JSON name is what matters** (`MEMORY.md`).
- `#[serde(rename_all = "lowercase")]` on the enum turns `NodeKind::Function`
  into `"function"`, not `"Function"`.
- `skip_serializing_if = "Option::is_none"` — omit the field entirely when
  absent, rather than emitting `null`.

⚠ That last one is a **decision you must verify at M0**. Does the Python driver
emit `"base_classes": null` on a function node, or omit the key? pydantic's
default is to include it. Look at the golden JSON before you choose — this is a
byte-identical-output gate, and `null` versus absent is a diff.

⚠ **`call_index` and `call_col_pos` used to be here and are gone** — see object
5 part 4 §4. They were Jedi's cursor-position tiebreak; one node per `ExprCall`
makes `position` unique, so the range is the whole handle. Removing them is a
wire change: check v-noc before shipping, and expect the M2 gate to differ on
calls.

---

## The scanner and the driver

Object 5 gave you the shape. Assembled:

```rust
/// Scan ONE scope. Returns its direct children, with nested scopes
/// recursed into.
fn scan_scope<'a>(ctx: &Ctx<'a>, root: AnyNodeRef<'a>) -> Vec<Node> {
    let mut scanner = ScopeScanner::new(root.range());
    walk_node(&mut scanner, root);        // ★ the NODE, not its body — see below

    let mut out = Vec::new();

    // definitions found in this scope → a node each, recursed into
    for def in scanner.defs {
        let children = scan_scope(ctx, def);          // ★★ the recursion
        out.push(node_for_definition(ctx, def, children));
    }
    // calls found in this scope → exactly one node each
    for call in scanner.calls {
        out.push(node_for_call(ctx, call));
    }

    out.sort_by_key(|n| (n.position.line, n.position.column));
    out
}

// and the module, whose body is not a node:
let mut scanner = ScopeScanner::new(module.range());
walk_body(&mut scanner, &parsed.syntax().body);
```

### The four names in it, and where they are written

`Ctx`, `ScopeScanner::new`, `node_for_definition` and `node_for_call` are
**your** code. Paste this block into a `src/bin/*.rs` on its own and you get ~46
compile errors — the chapter hands you the pieces, and assembling them is the
exercise. Do it yourself first; **object 5 part 4
([Wiring it up](05-object-visitor/04-wiring-it-up.md)) has all four written out
as code that compiles and runs**, plus the two `main`s that call them. What you
owe each one:

| name | what it must do | where the pieces are |
|---|---|---|
| `Ctx<'a>` | carry `&LineIndex` / `SourceCode` and the source text, so every helper can make a `Position` | `src/positions.rs` |
| `ScopeScanner::new(range)` | the struct literal from object 5 part 2, with `defs`/`calls` empty | object 5 part 2, example 2 |
| `node_for_definition(ctx, def, children)` | name, `NodeKind`, the range (**decorators included** — `DECORATORS_IN_RANGE`), `ID:` from the docstring (`null` if absent — quirk 4), `base_classes` for a class | objects 3 and 6 |
| `node_for_call(ctx, call)` | one `Node` per `ExprCall`: `call.range()` as the position, `call.func` for the name | object 4 |

### ⚠ `walk_node(root)`, not `walk_body(&def.body)`

An earlier version of this page entered each scope with `walk_body(&mut scanner,
body)`. That is wrong, and quietly: it **drops every call in a default
argument**, because layer 2 visits `parameters` before `body` and entering at the
body skips it. On `edges.py:default_args` the `walk_body` driver reports zero
calls where parso reports two. It also makes object 5's root guard and
`visit_decorator` override into dead code — which is the tell. Object 5 part 2
has the measured comparison.

### Open decisions, all of which need checking against the goldens

Decision 0 is **settled by [file 8](08-staged-pipeline.md)** rather than by a
golden: once calls are found by evaluating a body, a decorator expression is
evaluated in the scope the `def` statement is in, which is parso's answer and
needs no special case. The empty `visit_decorator` stays only for the
`parse_file` tree, and it stays the one behaviour here that loses data.

0. **Decorator *contents*.** The range question is settled —
   `DECORATORS_IN_RANGE = true`, decorators are inside it (object 3, and the
   decision note in `src/scan.rs`). The subtree question is not: the empty
   `visit_decorator` drops a decorator's calls from *every* scope, so
   `edges.py`'s `functools.wraps(build)` reaches no node's children. parso puts
   it in the enclosing scope; ruff's tree would put it under the decorated
   function. Today's answer matches neither and loses a call.
1. **Definitions before calls, then sorted by position.** Does the Python driver
   emit source order? `_scan_children` walks parso's children in order, so
   probably yes — but *verify*, because your two-pass structure does not
   naturally produce it.
2. **`sort_by_key` on `(line, column, end_line, end_column)`.** Calls nested in
   one expression share a start, so the **end** breaks the tie — which is what
   puts `build`, `render`, `strip` in the order they run. Sorting on the start
   alone would leave that to the sort's stability.

---

## Checking yourself against the quirk list

This is the real deliverable. Go through `plan/00-orientation/01`'s behavioural
contract and, for each quirk, name the line of your code that implements it.

| quirk | what it says | where in your code |
|---|---|---|
| **8** | lambdas dropped, with their subtree | `enter_node`: `ExprLambda => Skip` |
| **9** | nested defs/classes terminate the scan | `enter_node`: `FunctionDef \| ClassDef => Skip` |
| **10** | position dedup on identical 4-tuples | **unnecessary** — two `ExprCall`s cannot share a range |
| **11** | `async def` uses the `async` keyword position | **nothing** — ruff's range already does it |
| **12** | `call_index` counts call-trailers in one chain | **dropped** — one node per `ExprCall`, position is the handle |
| — | decorators are outside the funcdef in parso | **deliberately not matched** for the range (`DECORATORS_IN_RANGE`); empty `visit_decorator` override for the subtree, which is still open |
| **4** | no ID → `id: null` here (**not** dropped) | `node_for_definition` |

**Any quirk you cannot point at is one you have not implemented**, and every one
of them is visible in the JSON.

Quirk 11 having an empty cell is the good kind of surprise — write it down as
"verified unnecessary" rather than leaving it blank, or someone will later
"fix" it.

---

## The command

```
pylspt-dev parse <file> [--pretty]
```

```json
{
  "nodes": [
    {
      "id": "FunctionSchema/aaaaaaaa-1111-1111-1111-111111111111",
      "name": "outer",
      "type": "function",
      "position": { "line": 10, "column": 0, "end_line": 21, "end_column": 12 },
      "children": [
        {
          "id": null,
          "name": "build",
          "type": "call",
          "position": { "line": 15, "column": 4, "end_line": 15, "end_column": 11 },
          "children": []
        },
        {
          "id": "FunctionSchema/aaaaaaaa-2222-2222-2222-222222222222",
          "name": "inner",
          "type": "function",
          "position": { "line": 17, "column": 4, "end_line": 19, "end_column": 13 },
          "children": [ /* log() lives HERE, not under outer */ ]
        }
      ]
    }
  ],
  "content": "…",
  "modified": false
}
```

`content` and `modified` come from exercise 10 (injection). For now: the file's
text, and `false`.

**Remember `plan/04-build/00-dev-cli.md`'s rule** — this command lives in
`src/bin/pylspt-dev.rs` and contains *no analysis*. It parses arguments, calls
`pylspt::nodes::parse_file(...)`, and prints. If the server and the CLI can
disagree, your fixtures prove nothing.

---

## Verify against the fixtures

For each fixture, predict then check:

### `python/nested.py`

| node | must be under |
|---|---|
| `build()` in `outer` | `outer` |
| `log()` in `inner` | **`inner`**, not `outer` |
| `inner()` call | `outer` |
| `build()` in `Container.registry` | `Container` (a class body is a scope) |
| `log()` in `method` | `method` |
| `build()` in `Inner.deep` | `deep`, three levels down |
| `conditional` def | `with_blocks` — it is inside an `if` |
| `log()` in `conditional` | `conditional` |
| the `try/except/finally` calls | `with_blocks` |

### `python/edges.py`

| check | expected |
|---|---|
| `decorated`'s position | starts at `@` (or your documented alternative) |
| `functools.wraps(build)` | **absent** from `decorated_async`'s children |
| `plain_async`'s position | starts at `async` |
| `has_lambda`'s children | exactly **one** call — `build()` |
| `one_liner`'s children | one call |
| `NoDocstring.method` | `id: null`, node still present |
| `comprehensions` | one call, inside the list comprehension |
| `default_args` | two calls, from the parameter defaults |

### `python/calls.py`

Every function's call count and indices, from object 4's checklist. `nested_args`
is three calls, all index 0.

### `python/docstrings.py`

Twelve rows from object 6's table. `not_first_statement` must have `id: null`.

---

## Snapshot it

```
pylspt-dev test python/ --bless
```

Write each file's output to `<name>.expected.json`, then diff on subsequent
runs. `--bless` rewrites them deliberately, so you review the change as a
`git diff`.

⚠ **Generate the first snapshots from the Python driver, not from your Rust** —
otherwise you are snapshotting your own bugs (M0, `plan/04-build/02`). If the
Python driver is not runnable right now, hand-check at least `nested.py` and
`edges.py` line by line and mark the rest provisional.

Wire the same comparison into `cargo test` with `insta`, which ruff itself uses.

---

## Done when

- [ ] `src/nodes.rs` exists with `Node`, `NodeKind`, and the scanner
- [ ] `pylspt-dev parse <file> --pretty` emits the wire shape
- [ ] every check in the four fixture tables passes
- [ ] you can point at the code implementing quirks 4, 8, 9, 10, 12 and the
      decorator skip
- [ ] quirk 11 is documented as "unnecessary, verified"
- [ ] the decorator range decision is written down in a comment
- [ ] the `multiline_callee` name decision is written down
- [ ] `null` vs absent for optional fields is checked against the goldens (or
      flagged as an open M0 question)
- [ ] the CLI contains no analysis code

---

## What you have now

Three of `parse_file`'s three fields, minus MRO: a node tree with real names,
real positions, real IDs and correct nesting. That is **M2** in
`plan/04-build/02-milestones.md`, and its gate is byte-identical `nodes` JSON
against the Python driver for a 200-file corpus.

What you do **not** have yet is a pipeline. This tree is a snapshot of a file;
injection has to write to that file, and the call tree has to walk it with an
environment. Both of those want the scanner split —
[file 8](08-staged-pipeline.md).

The plan says this about M2, and it is worth taking seriously:

> *This is the milestone that proves your position/LineIndex handling. Everything
> downstream reports positions; get it exact here.*

→ [`08-staged-pipeline.md`](08-staged-pipeline.md), then [`exam.md`](exam.md),
then [`../03-the-database/README.md`](../03-the-database/README.md)
