# 07 — Putting it together: the node tree

Six objects, one module, and the first real output your driver produces.

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

  // call only:
  "call_index": 0,
  "call_col_pos": 14,

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
   ├── Expr::Call                           object 4   → a node, with call_index
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
    pub call_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_col_pos: Option<usize>,
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
emit `"call_index": null` on a class node, or omit the key? pydantic's default is
to include it. Look at the golden JSON before you choose — this is a
byte-identical-output gate, and `null` versus absent is a diff.

---

## The scanner and the driver

Object 5 gave you the shape. Assembled:

```rust
/// Scan ONE scope. Returns its direct children, with nested scopes
/// recursed into.
fn scan_scope<'a>(
    ctx: &Ctx<'a>,             // db, SourceCode, whatever you need
    root: AnyNodeRef<'a>,
    body: &'a [Stmt],
) -> Vec<Node> {
    let mut scanner = ScopeScanner::new(root.range());
    walk_body(&mut scanner, body);

    let mut out = Vec::new();

    // definitions found in this scope → a node each, then recurse
    for def in scanner.defs {
        out.push(node_for_definition(ctx, def));
    }
    // calls found in this scope → one node per call in each chain
    for call in scanner.calls {
        out.extend(nodes_for_call_chain(ctx, call));
    }

    out.sort_by_key(|n| (n.position.line, n.position.column));
    out
}
```

Three decisions visible in there, all of which need checking against the
goldens:

1. **Definitions before calls, then sorted by position.** Does the Python driver
   emit source order? `_scan_children` walks parso's children in order, so
   probably yes — but *verify*, because your two-pass structure does not
   naturally produce it.
2. **`sort_by_key` on `(line, column)`.** Ties are possible (a call chain shares
   a start), so make sure the sort is **stable** — `sort_by_key` is, in Rust.
3. **Position dedup happens in the scanner**, before this point (quirk 10).

---

## Checking yourself against the quirk list

This is the real deliverable. Go through `plan/00-orientation/01`'s behavioural
contract and, for each quirk, name the line of your code that implements it.

| quirk | what it says | where in your code |
|---|---|---|
| **8** | lambdas dropped, with their subtree | `enter_node`: `ExprLambda => Skip` |
| **9** | nested defs/classes terminate the scan | `enter_node`: `FunctionDef \| ClassDef => Skip` |
| **10** | position dedup on identical 4-tuples | `FxHashSet<TextRange>` in the scanner |
| **11** | `async def` uses the `async` keyword position | **nothing** — ruff's range already does it |
| **12** | `call_index` counts call-trailers in one chain | `flatten_call_chain` + `enumerate()` |
| — | decorators are outside the funcdef in parso | empty `visit_decorator` override |
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
          "children": [],
          "call_index": 0,
          "call_col_pos": 9
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

The plan says this about M2, and it is worth taking seriously:

> *This is the milestone that proves your position/LineIndex handling. Everything
> downstream reports positions; get it exact here.*

→ [`exam.md`](exam.md), then
[`../03-the-database/README.md`](../03-the-database/README.md)
