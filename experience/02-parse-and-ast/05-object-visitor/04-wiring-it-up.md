# Object 5 · Part 4 — Wiring it up: from scanner to `main`

The four names object 7's sketch leaves undefined, written out, plus the two
`main`s that call them.

**Part 4 of 4** · prev: [Part 3 — Examples, exercises, exam](03-examples-and-exam.md) ·
[Part 1 — How the walk works](01-how-the-walk-works.md) ·
[Part 2 — Stopping the walk](02-stopping-the-walk.md)

Parts 1–3 give you a visitor that finds the right nodes. This part turns that
into JSON on stdout. **Everything here compiles and runs** at `ac201b8` — it is
`src/scan.rs` and `src/bin/*.rs` in this repo, not a sketch. That distinction
matters: the version of `scan_scope` that object 7 published for months had two
bugs that only appeared when it was run against the fixtures, and this page
exists because sketches do not get debugged.

---

## The shape of the wiring

```
main                        ← src/bin/scan.rs  |  src/bin/pylspt-dev.rs
 ├─ parse / load the file    …a ParsedModule, kept alive for the whole scan
 ├─ LineIndex + Ctx          …the two borrows every helper needs
 ├─ scan_module(ctx, body)   ← src/scan.rs
 │   └─ collect
 │       ├─ scan_scope(ctx, def)           ← recursion, one scope per call
 │       │   └─ node_for_definition(…)     ← objects 3 + 6
 │       └─ node_for_call(…)              ← one node per ExprCall
 └─ serde_json → stdout
```

Two entry functions, because a module body is not a node (part 1's hooks table):

| function | entry | use for |
|---|---|---|
| `scan_module(ctx, body)` | `walk_body` | the file's top level |
| `scan_scope(ctx, def)` | `walk_node` | every `def`/`class` below it |

---

## 1. `Ctx` — the borrow bundle

Every helper needs to turn a `TextRange` into a `Position`, and some need the
source text. That is two borrows threaded through four functions, so bundle them.

```rust
/// Everything the emit helpers need. Borrows the source; never owns it.
pub struct Ctx<'src> {
    pub code: SourceCode<'src, 'src>,
}

impl<'src> Ctx<'src> {
    pub fn new(source: &'src str, index: &'src LineIndex) -> Self {
        Self { code: SourceCode::new(source, index) }
    }

    fn position(&self, range: TextRange) -> Position {
        to_position_with(&self.code, range)
    }

    fn slice(&self, range: TextRange) -> &'src str {
        &self.code.text()[range]
    }
}
```

**Rust note — why `Ctx<'src>` and not `Ctx` owning a `String`.** `SourceCode`
borrows both the text and the index, so it cannot outlive either. Making `Ctx`
own the `String` would mean `SourceCode` borrowing from its own struct — a
self-referential type, which Rust will not let you build without `Pin` or a
crate. Bind the source and the index in `main`, pass `&Ctx` down. This is the
same "lower to owned at the boundary" rule from part 3's answer 15, seen from
the other side: **`Node` is owned, `Ctx` is borrowed, and the helpers are the
boundary between them.**

Note `position` and `slice` are private: the helpers in this module use them, and
nothing outside needs them.

---

## 2. `ScopeScanner::new` and the driver

The visitor is part 2's, unchanged. The constructor is the part the sketch
omitted:

```rust
impl<'a> ScopeScanner<'a> {
    pub fn new(root: TextRange) -> Self {
        Self {
            root,
            defs: Vec::new(),
            calls: Vec::new(),
        }
    }
}
```

And the two entry points plus the shared tail:

```rust
/// Scan ONE scope. Returns its direct children, nested scopes recursed into.
pub fn scan_scope<'a>(ctx: &Ctx<'_>, root: AnyNodeRef<'a>) -> Vec<Node> {
    let mut scanner = ScopeScanner::new(root.range());
    walk_node(&mut scanner, root);          // ★ the NODE, not its body
    collect(ctx, scanner)
}

/// Scan a module: its body is not a node, so there is no root to enter.
pub fn scan_module<'a>(ctx: &Ctx<'_>, body: &'a [Stmt]) -> Vec<Node> {
    let mut scanner = ScopeScanner::new(TextRange::default());
    walk_body(&mut scanner, body);
    collect(ctx, scanner)
}

fn collect(ctx: &Ctx<'_>, scanner: ScopeScanner<'_>) -> Vec<Node> {
    let mut out = Vec::new();
    for def in scanner.defs {
        let children = scan_scope(ctx, def);              // ★★ the recursion
        out.push(node_for_definition(ctx, def, children));
    }
    for call in scanner.calls {
        out.push(node_for_call(ctx, call));
    }
    out.sort_by_key(|n| (n.position.line, n.position.column));
    out
}
```

Three things worth noticing:

- **`TextRange::default()` as the module's root** is `0..0`, which no real node
  has, so the root guard never fires spuriously. Cleaner than inventing the
  module's full range, which *could* equal a node's range in a one-expression
  file.
- **`collect` takes the scanner by value.** It is finished with; moving it lets
  `for def in scanner.defs` consume the `Vec` without a clone.
- **The recursion is in `collect`, not in `scan_scope`.** Both entry points get
  it for free. This is the line object 7's sketch promised in a comment
  (`// → a node each, then recurse`) and never wrote.

---

## 3. `node_for_definition` — objects 3 and 6, assembled

```rust
fn node_for_definition(ctx: &Ctx<'_>, def: AnyNodeRef<'_>, children: Vec<Node>) -> Node {
    let (name, kind, body, range) = match def {
        AnyNodeRef::StmtFunctionDef(d) => {
            (d.name.to_string(), NodeKind::Function, &*d.body, d.range)
        }
        AnyNodeRef::StmtClassDef(d) => (d.name.to_string(), NodeKind::Class, &*d.body, d.range),
        _ => unreachable!("scanner only pushes defs"),
    };

    Node {
        id: id_from_body(body).map(|uuid| match kind {
            NodeKind::Class => format!("ClassSchema/{uuid}"),
            _ => format!("FunctionSchema/{uuid}"),
        }),
        name,
        kind,
        position: ctx.position(def_position_range(ctx, range, def)),
        children,
        // exercise 08: needs semantic resolution, not the AST.
        base_classes: None,
    }
}
```

- **`id` is `Option<String>`, and `None` serialises as `"id": null`** — not
  omitted. That is quirk 4 for `parse_file`: a def with no `ID:` must still
  appear, because `id: null` is what triggers injection downstream. Object 6 has
  the table; `nodes.rs` has no `skip_serializing_if` on this field, which is the
  decision made visible.
- **`base_classes: None` is deliberate**, not a stub. It needs the MRO, which is
  semantic work (exercise 08), and `None` + `skip_serializing_if` means the key
  is absent for functions — which is what the wire shape wants.
- **`unreachable!` with a reason.** The scanner only ever pushes those two
  variants, and saying so in the message is better than a silent fallback that
  would emit a wrong node if the `enter_node` arm ever grew.

### ⚠ DECIDED — decorators are **inside** the range

This is the trap from object 3, and it is a decision, not a bug to fix. Two
defensible answers, and the code now holds both behind one named const:

| | `position` of `edges.py:decorated` | matches |
|---|---|---|
| `DECORATORS_IN_RANGE = true` ← **chosen** | `line: 11` — the `@functools.cache` | ruff's own range |
| `DECORATORS_IN_RANGE = false` | `line: 12` — the `def` keyword | parso / the Python driver |

```rust
/// DECIDED: a decorated def's `position` **starts at the `@`**, not at the
/// `def`/`async`/`class` keyword — i.e. the decorators are part of the range.
/// Flip this const to go back to parso's convention; `header_range` below is
/// the other half of the switch and stays compiled either way.
const DECORATORS_IN_RANGE: bool = true;

/// The range a definition's `position` is computed from.
fn def_position_range(ctx: &Ctx<'_>, range: TextRange, def: AnyNodeRef<'_>) -> TextRange {
    if DECORATORS_IN_RANGE {
        range // ruff's own range: already starts at `@` (and at `async`)
    } else {
        header_range(ctx, range, def)
    }
}
```

**What choosing `true` costs.** The M2 byte-identical gate
(`plan/04-build/02-milestones.md`) will differ on **every decorated def** — on
real code that is most methods (`@property`, `@staticmethod`, `@pytest.fixture`,
`@app.route`). That is the expected diff, not a regression, and it is the reason
the parity path stays compiled rather than being deleted: the gate can flip it
back with one word.

Note the asymmetry this settles. Ruff's range includes `async` (quirk 11 — which
you want, and which deletes a parso special case) *and* decorators. Taking the
range as-is now accepts both.

### The parity path: `header_range`, done properly

Object 3 offers "range from just after the last decorator" and shrugs. That is
not good enough, and running it shows why: `last.range().end()` is the offset of
the **newline** after the decorator, so the position reports the *decorator's*
line anyway — at its end column, which is neither answer in the table above.

Measured on `edges.py:decorated` (`@functools.cache` on line 11, `def` on
line 12):

```
naive:   "position": { "line": 11, "column": 16, … }   ← end of the decorator ✗
skipped: "position": { "line": 12, "column": 0,  … }   ← the `def` keyword ✓
```

So the parity branch is a trivia skip, and it is fifteen lines with no new
dependency:

```rust
fn header_range(ctx: &Ctx<'_>, range: TextRange, def: AnyNodeRef<'_>) -> TextRange {
    let last = match def {
        AnyNodeRef::StmtFunctionDef(d) => d.decorator_list.last().map(|d| d.range().end()),
        AnyNodeRef::StmtClassDef(d) => d.decorator_list.last().map(|d| d.range().end()),
        _ => None,
    };
    let Some(end) = last else { return range };
    TextRange::new(skip_trivia(ctx.code.text(), end), range.end())
}

/// First offset at or after `from` that is not whitespace or a comment.
fn skip_trivia(text: &str, from: TextSize) -> TextSize {
    let mut at = from.to_usize();
    let bytes = text.as_bytes();
    while at < bytes.len() {
        match bytes[at] {
            b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' => at += 1,
            b'#' => {
                // a comment runs to the end of its line
                at += bytes[at..]
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(bytes.len() - at, |n| n + 1);
            }
            _ => break,
        }
    }
    TextSize::try_from(at).expect("offset came from this text")
}
```

Handling `#` matters: `@deco  # why\ndef f():` is legal, and stopping at the
first non-whitespace would land you on the comment. `ruff_python_trivia`'s
`SimpleTokenizer` would also do this — but it is one more dependency for
something the parser has already bounded for you.

**Rust note — `let Some(end) = last else { return range };`.** `let`-`else` is
the early-return form for a pattern that may not match. It binds `end` for the
rest of the function with no nesting, which a `match` or `if let` would both
force on you.

### ⚠ Still open: the decorator's *contents*

The range decision says nothing about where a call **inside** a decorator goes,
and the two are independent. `visit_decorator` is currently an empty override
(part 2), which suppresses the subtree in *every* scope — so
`@functools.wraps(build)` in `edges.py` appears **nowhere** in the output. Three
answers, and this one is not settled:

| | where `functools.wraps(build)` lands |
|---|---|
| empty `visit_decorator` ← **today** | dropped entirely — matches neither implementation |
| delete the override | a child of `decorated_async`, consistent with the range decision |
| walk decorators in the *enclosing* scanner only | the enclosing scope — parso's answer |

Today's behaviour loses a call, which is the one answer nothing argues for. Pick
one against a golden before M2.

### `id_from_body` — object 6's scan, as code

```rust
fn id_from_body(body: &[Stmt]) -> Option<String> {
    let doc = docstring(body)?;
    extract_metadata(doc).get("ID").map(|s| s.to_string())
}

fn docstring(body: &[Stmt]) -> Option<&str> {
    match body.first()? {
        Stmt::Expr(e) => Some(e.value.as_string_literal_expr()?.value.to_str()),
        _ => None,
    }
}

/// `key: value` pairs, last-wins — mirrors `re.findall(r"(\S+)\s*:\s*(\S+)")`.
fn extract_metadata(doc: &str) -> FxHashMap<&str, &str> {
    let mut out = FxHashMap::default();
    for (i, _) in doc.char_indices().filter(|(_, c)| *c == ':') {
        let key = doc[..i].split_whitespace().next_back();
        let value = doc[i + 1..].split_whitespace().next();
        if let (Some(k), Some(v)) = (key, value) {
            if !k.contains(':') && !v.contains(':') {
                out.insert(k, v);
            }
        }
    }
    out
}
```

`next_back()` on a `SplitWhitespace` gives the **last** whitespace-delimited run
before the colon, which is `\S+` anchored at the colon. No regex, no lookbehind,
and the returned `&str`s borrow the docstring — only the one `ID` value is
allocated. Verified against `docstrings.py`: `no_docstring` and
`not_first_statement` come out `[no id]`, `id_like_text` and `multiple_keys`
resolve as object 6 describes.

---

## 4. `node_for_call` — one node per `ExprCall`

```rust
fn node_for_call(ctx: &Ctx<'_>, call: &ExprCall) -> Node {
    Node {
        id: None,                        // calls never have one
        name: call_name(ctx, call),
        kind: NodeKind::Call,
        position: ctx.position(call.range()),
        children: Vec::new(),
        base_classes: None,
    }
}

/// The called name, from the AST rather than from a source slice.
fn call_name(ctx: &Ctx<'_>, call: &ExprCall) -> String {
    match call.func.as_ref() {
        Expr::Name(n) => n.id.as_str().to_string(),          // build()
        Expr::Attribute(a) => a.attr.as_str().to_string(),   // .render()
        other => ctx.slice(other.range()).to_string(),       // handlers["x"]()
    }
}
```

That is the whole emitter. **`build().render().strip()` is three `ExprCall`s, so
three nodes:**

```
call build    1:0..1:7
call render   1:0..1:16
call strip    1:0..1:24
```

### The rule that makes it work

> **One output node per AST node.** Then the visitor's "each node is visited
> once" guarantee *is* your deduplication.

The earlier design broke that rule: the scanner counted **calls** and the
emitter counted **chains**, and two counts of the same thing drift. Every bug in
this section's history lived in that gap:

| was | now |
|---|---|
| each chain emitted once per call in it (so `deep_chain` gave 6 nodes, not 3) | impossible — one node per visit |
| `flatten_call_chain` stopping one wrapper early on `build()["key"].render()`, splitting one chain into two numbered from 0 | no chain to split |
| a `seen: FxHashSet<TextRange>` guard to suppress the duplicates | deleted; two `ExprCall`s cannot share a range, since an outer call always adds its own `(…)` |
| "siblings or nested?" — unspecified, needed a golden | no chains, no nesting question |
| `name` as a raw source slice, so `multiline_callee` was `"obj \\\n        . render"` | `render`, from `a.attr` |

### `position` is already right, for free

Ruff's `ExprCall.range()` covers its `func`, so `build().render()` **starts at
`build`**. The old code reconstructed that span by hand as "chain start → this
call's end"; `call.range()` is that exact span, already computed.

### `call_index` and `call_col_pos` are gone

They existed for Jedi, which resolves by cursor position: every call in a chain
starts at the same offset, so it needed an extra number to say which one you
meant. With one node per `ExprCall` the `position` is unique, so the range *is*
the handle — the call tree names a call site by its range (`SiteId`, `plan/03-call-tree/09`).

Dropping them is a wire-format change, so the two costs, stated plainly:

- **Any reader of those fields breaks.** Nothing in this repo reads them; v-noc
  downstream is the one to check.
- **The M2 byte-identical gate stops being meaningful for calls.** Already true
  for decorated defs (section 3). Make new goldens from the Rust output and
  review them by hand, once.

If something later needs "this call is made on the result of that call", it is
one `call.func` hop away — compute it where it is read, not in the output.

### The tie-break in `collect`

Calls nested in one expression share a start, so sorting on `(line, column)`
alone leaves their order to the sort's stability. Sort on all four:

```rust
out.sort_by_key(|n| {
    let p = n.position;
    (p.line, p.column, p.end_line, p.end_column)
});
```

The end breaks the tie, which puts `build`, `render`, `strip` in the order they
run.

### One rough edge: a call whose callee is a call

`build()()` gives the outer node the name `build()` — the `other` arm's source
slice, since `call.func` is itself an `ExprCall`. It is accurate (the callee
*is* `build()`), and `calls.py:call_of_call` is the fixture. If you would rather
it were empty or `<call>`, that is one more match arm.

---

## 5. `main` — the standalone CLI

`src/bin/scan.rs`. Parses the file directly, which is right for a fixture-driver
and wrong for the product (see the next one).

```rust
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or_else(|| anyhow::anyhow!("usage: scan <file> [--pretty]"))?;
    let pretty = args.any(|a| a == "--pretty");

    // 1. source → AST. One parse, kept alive for the whole scan.
    let source = std::fs::read_to_string(&path)?;
    let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY313);
    let parsed = parse_unchecked(&source, options)
        .try_into_module()
        .expect("Mode::Module always produces a module");

    // 2. the two borrows every helper needs, built once.
    let index = LineIndex::from_source_text(&source);
    let ctx = Ctx::new(&source, &index);

    // 3. scan the module body — not a node, so scan_module, not scan_scope.
    let nodes = scan_module(&ctx, &parsed.syntax().body);

    // 4. serialise. The JSON is the contract — including KEY ORDER.
    #[derive(serde::Serialize)]
    struct Output<'a> { nodes: &'a [Node] }
    let out = Output { nodes: &nodes };
    println!("{}", if pretty {
        serde_json::to_string_pretty(&out)?
    } else {
        serde_json::to_string(&out)?
    });
    Ok(())
}
```

### ⚠ Do not serialise through `serde_json::json!`

The first version of this `main` ended with `serde_json::json!({ "nodes": nodes })`
and the output came out **alphabetised**:

```jsonc
// via json!  — wrong                  // via a struct — right
{ "children": [],                      { "id": null,
  "id": null,                            "name": "build",
  "name": "build",                       "type": "call",
  "position": { … },                     "position": { … },
  "type": "call" }                       "children": [] }
```

`json!` builds a `Value`, whose `Map` is a `BTreeMap` unless serde_json is
compiled with `preserve_order` — so it **sorts your keys**. Serialising the
struct directly keeps declaration order, which is the order `nodes.rs` declares
and the order the wire shape documents. For a byte-identical-output gate this is
not cosmetic.

It hits nested structs too — `Position` came out
`column, end_column, end_line, line` under `json!` and `line, column, end_line,
end_column` through the struct. So there is no "only the top level matters"
escape: one `json!` anywhere in the output path re-sorts everything beneath it.

---

## 6. `main` — the real one, through the database

`src/bin/pylspt-dev.rs`. Object 1's rule is that you must not parse by hand: the
database owns the parse so salsa can cache it across files and invalidations.

```rust
fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().ok_or_else(|| anyhow::anyhow!("usage: pylspt-dev <dir> <file> [--pretty]"))?;
    let file_arg = args.next().ok_or_else(|| anyhow::anyhow!("usage: pylspt-dev <dir> <file> [--pretty]"))?;
    let pretty = args.any(|a| a == "--pretty");

    // ⚠ OsSystem::new asserts the project root is ABSOLUTE (os.rs:41), so a
    //   relative argument must be canonicalised or it panics, not errors.
    let dir = std::fs::canonicalize(&dir)?;
    let dir = SystemPath::from_std_path(&dir)
        .ok_or_else(|| anyhow::anyhow!("project root is not valid UTF-8: {dir:?}"))?;
    let db = open_project(dir)?;

    // the file path must be absolute too, and for the same reason: a relative
    // one silently resolves against the system root and yields an empty file.
    let file_path = std::fs::canonicalize(&file_arg)?;
    let file_path = SystemPath::from_std_path(&file_path)
        .ok_or_else(|| anyhow::anyhow!("file path is not valid UTF-8: {file_path:?}"))?;
    let (file, program_file) = open(&db, file_path)?;

    let parsed = parsed_module(&db, program_file.python_file(&db)).load(&db);

    // the source text must outlive the Ctx, so bind it before building one.
    let source = source_text(&db, file);
    let index = LineIndex::from_source_text(&source);
    let ctx = Ctx::new(&source, &index);

    let nodes = scan_module(&ctx, &parsed.syntax().body);
    // …same Output struct and println! as above
}
```

### ⚠ Two absolute-path traps, one of which is silent

Both were hit while writing this page:

| input | what happens |
|---|---|
| relative `dir` | **panic**: `assertion failed: cwd.as_utf8_path().is_absolute()` at `ruff_db/src/system/os.rs:41` |
| relative `file` | **`{"nodes":[]}`** — no error, no warning, empty output |

The first is loud. The second is the dangerous one: `system_path_to_file`
resolves the relative path against the system root, finds nothing useful, and
you get a well-formed empty answer. Canonicalise both, and note that
`SystemPath::from_std_path` returns `Option` because a `SystemPath` must be UTF-8
— that is the error case worth a real message rather than an `unwrap`.

**Rust note — `let source = source_text(&db, file);` on its own line.** `Ctx`
borrows the text, so the `SourceText` must live in a binding that outlives it.
Inlining it as `Ctx::new(&source_text(&db, file), &index)` creates a temporary
that is dropped at the end of the statement, and the borrow checker rejects it.

---

## Checking it

Both binaries must produce byte-identical output — same scanner, different file
loading:

```console
$ for f in calls nested edges docstrings helpers; do
    diff <(cargo run -q --bin scan -- experience/02-parse-and-ast/python/$f.py) \
         <(cargo run -q --bin pylspt-dev -- experience/02-parse-and-ast/python \
                                            experience/02-parse-and-ast/python/$f.py) \
      && echo "$f.py identical"
  done
calls.py identical
nested.py identical
edges.py identical
docstrings.py identical
helpers.py identical
```

And the tree for `nested.py`, which is exercise C's checklist as output:

```
func outer                  L9
  call build                  L14 idx=0 col=9
  func inner                  L16
    call log                    L18 idx=0 col=11     ← under inner, not outer ✓
  call inner                  L20 idx=0 col=9
clas Container              L23
  call build                  L29 idx=0 col=20
  func method                 L31
    call log                    L33 idx=0 col=11     ← under method ✓
    clas Inner                  L35
      func deep                   L38
        call build                  L40 idx=0 col=21 ← three levels down ✓
func with_blocks            L43
  func conditional            L50                    ← def inside an `if`, found ✓
    call log                    L52 idx=0 col=15
  call conditional            L54 idx=0 col=19
```

---

## What is still open

> **Two of these three are closed by the staging split**
> ([file 8](../08-staged-pipeline.md)), and not by a golden:
>
> - **item 0** — once stage 3 finds calls by evaluating a body, a decorator
>   expression is evaluated in the scope its `def` statement sits in. That is
>   parso's answer, reached with no special case and nothing dropped. The empty
>   `visit_decorator` survives only here, in the `parse_file` tree.
> - **item 2** — stage 1 needs no sort at all: `StatementVisitor`'s `walk_stmt`
>   visits every arm in written order, and defs are the only thing it pushes, so
>   there are no two lists to merge. The tie-break below is a property of
>   emitting defs and calls from one scanner.
>
> Item 1 (`base_classes`) is untouched: it still needs exercise 08.

Three things this code does **not** settle, all flagged in the source:

0. **Decorator *contents*.** Settled for the *range* (`DECORATORS_IN_RANGE =
   true`), open for the subtree: the empty `visit_decorator` drops a decorator's
   calls from every scope, so `edges.py`'s `functools.wraps(build)` is in no
   node's children. The section above has the three candidates. **This is the
   only open item that currently loses data**, so it ranks above the rest.
1. **`base_classes`** is `None`. Needs the MRO — exercise 08, and
   `plan-api-claims-wrong-at-ac201b8` warns that the public route gives direct
   bases, not a C3 linearisation.
2. **Definition-before-call emission order**, then a positional sort. The sort
   makes it right; whether the Python driver would ever disagree on a tie is a
   golden question.

Each of those is a comment in `src/scan.rs` as well as a row here. A known gap
with a `⚠` next to it is a decision; the same gap undocumented is a bug waiting
to be found during parity testing.
