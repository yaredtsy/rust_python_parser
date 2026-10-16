# 08 — Splitting the scanner by stage

One traversal that collects everything, replaced by three traversals that each
have one job.

---

## Why the one-pass scanner existed

Object 5's `ScopeScanner` collects **defs and calls in the same walk**, because
object 7's output is one tree with both kinds of node in it. That is
`parse_file`'s `nodes` array, and it is a faithful port of `parser.py`.

The *pipeline* — inject IDs, then answer "what does X call?" — does not need
that tree. It needs three different things at three different times, and
collecting calls up front gets in the way of all of them:

| | what it actually needs | what the one-pass scanner gives it |
|---|---|---|
| injection | every def, and where its docstring is | defs **and** every call |
| after injection | the same defs, at their **new** offsets | ranges from before the write |
| the call tree | the calls on the path the interpreter actually takes | every call in the file, statically |

The third row is the important one. The interpreter evaluates a body; it reaches
an `ExprCall` *because it got there*. A pre-collected list of every call in the
scope is a different object — it has no path, no environment, and no order
beyond source position. You would build it, then ignore it, then find the calls
again by evaluating.

So: **do not collect calls in the first traversal.**

---

## The three stages

```
             ┌──────────────────────────────────────────────────────────┐
  stage 1    │ definitions(db, file) -> &Vec<DefNode>                   │
             │ StatementVisitor · defs only · owned data · salsa-cached │
             └───────────┬──────────────────────────────────┬───────────┘
                         │                                  │
                         │ any id == None?                  └──► id_index:
                         ▼                                       ID -> [(File, TextRange)]
             ┌──────────────────────────────────────────┐
  stage 2    │ build inserts, bottom-up · write once    │
             │ File::sync(&mut db) · stage 1 reruns     │
             └───────────┬──────────────────────────────┘
                         │  fresh ranges
                         ▼
             ┌──────────────────────────────────────────┐
  stage 3    │ interpreter: eval a body, and when eval  │
             │ reaches an ExprCall, resolve it          │
             └──────────────────────────────────────────┘
```

Each stage owns a boundary the next one depends on:

1. **Stage 1** is the only thing that reads the AST for structure. Everything
   downstream sees `DefNode`, not `&Stmt`.
2. **Stage 2** is the only thing that writes. It invalidates every range stage 1
   produced, which is why it ends by re-running stage 1 rather than patching.
3. **Stage 3** never sees a list of calls. It sees a body and an environment.

---

## Stage 1 — defs only, owned, in a salsa query

### Why `StatementVisitor` and not `SourceOrderVisitor`

A `def` and a `class` are **statements**. `ruff_python_ast` ships a visitor that
walks statements and nothing else:

```rust
// ruff_python_ast/src/statement_visitor.rs:6   [verified]
pub trait StatementVisitor<'a> {
    fn visit_body(&mut self, body: &'a [Stmt]) { walk_body(self, body) }      // :7
    fn visit_stmt(&mut self, stmt: &'a Stmt)   { walk_stmt(self, stmt) }      // :10
    fn visit_except_handler(&mut self, h: &'a ExceptHandler) { … }
    fn visit_elif_else_clause(&mut self, c: &'a ElifElseClause) { … }
    fn visit_match_case(&mut self, c: &'a MatchCase) { … }
}
```

Five methods instead of object 5's thirty-three, and `walk_stmt` has **eight
arms** — `FunctionDef`, `ClassDef`, `For`, `While`, `If`, `With`, `Match`,
`Try` — plus `_ => {}` **[verified]** (`statement_visitor.rs:30`–`:78`). Those
eight are exactly the statements that can hold a body; `Stmt` has 25 variants
(`generated.rs:125`) and the other 17 have none, so the `_` arm is complete
rather than a shortcut.

It is exported as `pub mod statement_visitor` (`lib.rs:29`), same crate, no new
dependency.

### ★ Statements-only is complete, not an approximation

The reason this is safe is worth stating as a rule, because it is what lets you
throw away four of object 5's five mechanisms:

> **No `def` or `class` can live inside an expression.** Python has exactly one
> definition form that is an expression — `lambda` — and a lambda cannot hold a
> docstring, so it can never hold an ID.

So an expression-blind walk does not miss a single ID-bearing definition. Object
5 needed `enter_node`, `Skip`, the root guard and the lambda arm because it was
walking *expressions* to find calls; stage 1 is not walking expressions at all.

Check the claim against the fixtures: `edges.py`'s `has_lambda` and
`comprehensions`, `nested.py`'s `with_blocks` (a def inside an `if`, reached
through `walk_stmt`'s `If` arm), and `calls.py`'s `default_args` (whose two
calls are in the signature — invisible here, and correctly so).

### `DefNode` — owned, because the parse does not outlive the query

```rust
use ruff_text_size::{TextRange, TextSize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefNode {
    /// The raw UUID from `ID:`, or `None` if this def has no ID yet.
    pub id: Option<String>,
    pub name: String,
    pub kind: DefKind,
    /// `stmt.range()` — decorators included (object 3, `DECORATORS_IN_RANGE`).
    pub range: TextRange,
    /// Where stage 2 writes. See "one insert per def" below.
    pub doc: Option<DocSlot>,
    pub children: Vec<DefNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefKind { Function, Class }

/// Where stage 2 inserts, and what it inserts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocSlot {
    /// The offset the text goes at.
    pub at: TextSize,
    /// `body[0]`'s start: the indentation anchor, and the one-liner tell.
    /// Equal to `at` when there is no docstring yet.
    pub stmt_start: TextSize,
    pub kind: SlotKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    /// A docstring exists: `at` is just before its closing quotes.
    InLiteral { triple_quoted: bool },
    /// None exists: a whole new docstring statement goes in at `at`.
    NewStatement,
}
```

Three decisions in that struct, each with an alternative worth knowing:

- **`DefKind`, not `NodeKind`.** `NodeKind::Call` is unrepresentable in stage 1
  output, so a type with two variants beats a type with three and a dead one.
  Map `DefKind` → `NodeKind` at the serde boundary if `parse_file` keeps its
  wire shape.
- **`id` holds the raw UUID**, not `"FunctionSchema/<uuid>"`. The prefix is
  derivable from `kind`, and storing it twice invites drift. Prefix at the wire
  boundary; strip at the RPC edge. A bonus: a request for
  `ClassSchema/<uuid>` that resolves to a `DefKind::Function` is then a
  *detectable* mismatch rather than a silent miss.
- **`doc` is `Option<DocSlot>`** — `None` means "this def has no place to write
  an ID", which happens on broken source (an empty body, an unclosed string).
  Quirk 13 says swallow it, so model it instead of panicking.

**Owned data, not `&'a Stmt`.** The query's return value outlives the
`ParsedModuleRef` it was built from — salsa may even clear the AST
(`ParsedModule::clear`, `parsed.rs:160`) and re-parse on the next `load`. Same
rule as object 5 part 3's answer 15, now enforced by the signature: a tracked
function's output must be `'static` or a `SalsaValue`
(`salsa/src/salsa_value.rs:55` — `T: 'static` is accepted directly, which
`Vec<DefNode>` is) **[verified]**.

### `DefScanner`

```rust
use ruff_python_ast::statement_visitor::{StatementVisitor, walk_body, walk_stmt};
use ruff_python_ast::Stmt;
use ruff_text_size::Ranged;

#[derive(Default)]
struct DefScanner {
    out: Vec<DefNode>,
}

impl<'a> StatementVisitor<'a> for DefScanner {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        let (name, kind, body) = match stmt {
            Stmt::FunctionDef(f) => (&f.name, DefKind::Function, &f.body),
            Stmt::ClassDef(c) => (&c.name, DefKind::Class, &c.body),
            _ => {
                walk_stmt(self, stmt);   // if / for / while / with / try / match
                return;                  // — defs can sit inside any of them
            }
        };

        let mut inner = DefScanner::default();
        walk_body(&mut inner, body);     // ← a FRESH scanner: one per scope

        self.out.push(DefNode {
            id: id_from_body(body),      // object 6
            name: name.as_str().to_string(),
            kind,
            range: stmt.range(),
            doc: doc_slot(body),
            children: inner.out,
        });
    }
}
```

### ⚠ The def arm must *not* call `walk_stmt`

`StatementVisitor`'s `walk_stmt` descends into a `FunctionDef`'s and
`ClassDef`'s body on its own (`statement_visitor.rs:32` and `:38`). Call it in
the def arm *as well as* `walk_body(&mut inner, body)` and every nested def is
recorded twice — once as a child of the def, once as a sibling in the enclosing
scope.

Note the shape: object 5 needed `TraversalSignal::Skip` to stop at a nested def.
Here, **not calling the walk is the skip**. The two visitors arrive at "one
scanner per scope" from opposite directions, which is why the nesting in `out`
comes out the same.

**Source order falls out for free.** `walk_stmt` visits `body` before `orelse`,
`body` → `handlers` → `orelse` → `finalbody` for a `Try`, and `body` before the
`elif_else_clauses` — written order in every arm, so `out` needs no sort. That
is a property of this walk, not of the AST; object 5's `collect` sorts because
it merges two lists.

**Rust note — `body` is a `ThinVec`, not a `Vec`.** `StmtFunctionDef.body` and
`StmtClassDef.body` are both `thin_vec::ThinVec<Stmt>` (`generated.rs:9310`,
`:9323`; `pub type Suite = ThinVec<Stmt>` at `nodes.rs:3734`) **[verified]**.
`&f.body` coerces to `&[Stmt]` at a call site through `Deref`, so the code above
compiles — but a *signature* written as `fn f(body: &Vec<Stmt>)` will not. Take
`&[Stmt]` everywhere, as object 6's helpers do.

### `doc_slot` — the insert point, computed where the AST is still in hand

```rust
use ruff_python_ast::{ExprStringLiteral, StringFlags};
use ty_python_core::definition::docstring_from_body;   // object 6, verified public

fn doc_slot(body: &[Stmt]) -> Option<DocSlot> {
    let stmt_start = body.first()?.start();   // broken source can give an empty body
    match docstring_from_body(body) {
        // Insert just before the closing quotes of the LAST part of the literal.
        Some(doc) => {
            let last = doc.value.as_slice().last()?;   // implicit concat: the last part closes it
            let flags = last.flags;
            if flags.is_unclosed() {
                return None;                           // does not parse; write nothing
            }
            Some(DocSlot {
                at: last.range().end() - flags.closer_len(),
                stmt_start,
                kind: SlotKind::InLiteral { triple_quoted: flags.is_triple_quoted() },
            })
        }
        None => Some(DocSlot { at: stmt_start, stmt_start, kind: SlotKind::NewStatement }),
    }
}
```

Everything that function needs is on `StringFlags` (`nodes.rs:799`
**[verified]**): `closer_len()` (`:847` — `quote_len()`, or zero when the string
is unclosed), `is_triple_quoted()` (`:811`), `quote_str()`, `prefix()`.
`StringLiteralValue::as_slice()` is `nodes.rs:1420`.

⚠ **An empty body is reachable.** `parse_body` returns `Suite::new()` after an
error — `ruff_python_parser/src/parser/statement.rs:3117` **[verified]** — so
`def f():` at end of file gives a def with *no* statements. `body.first()?` is
not defensive programming; it is the quirk-13 path.

### The query

```rust
use ruff_db::files::File;
use ruff_db::parsed::parsed_module;
use ty_project::Db;                         // same Db ty_ide's own queries take

#[salsa::tracked(returns(ref))]
pub fn definitions(db: &dyn Db, file: File) -> Vec<DefNode> {
    let python_file = db.program_file(file).python_file(db);   // ★ see below
    let parsed = parsed_module(db, python_file);
    let module = parsed.load(db);

    let mut scanner = DefScanner::default();
    walk_body(&mut scanner, &module.syntax().body);
    scanner.out
}
```

### ⚠ `parsed_module` does not take a `File`

```rust
// ruff_db/src/parsed.rs:34-35   [verified]
#[salsa::tracked(returns(ref), no_eq, heap_size=…, lru=200)]
pub fn parsed_module(db: &dyn Db, file: PythonFile<'_>) -> ParsedModule
```

`PythonFile` is an interned `(File, PythonVersion)` pair (`ruff_db/src/lib.rs:34`
**[verified]**), and its doc comment says why: the same file may be parsed for
two Python versions in one revision without sharing an incompatible AST. So
`parsed_module(db, file)` with a `File` does not compile. The two hops are
`db.program_file(file)` (`ty_python_semantic/src/db.rs:14`) then
`.python_file(db)` (`ty_python_core/src/program_file.rs:61`) — which is what
`src/bin/pylspt-dev.rs` already does.

That chain is also why keying `definitions` on `File` is still correct across
versions: the query *depends* on `program_file`, so a changed target version
invalidates it. ty's own convention is to key on `ProgramFile<'_>` instead
(`ty_ide/src/symbols.rs:395`) and let the caller do the hop; either works, and
`File` keeps the call site short while the index below needs a `File` back
anyway.

### The salsa attributes, one at a time

`parsed_module`'s attribute list is not a template to copy wholesale:

| attribute | copy it? | why |
|---|---|---|
| `returns(ref)` | **yes** | hands out `&Vec<DefNode>`; the alternative clones the whole file's defs on every read |
| `no_eq` | **no** | `parsed_module` uses it because a `ParsedModule` is not comparable. Your `Vec<DefNode>` is, and comparison is what lets salsa *backdate* an unchanged result |
| `heap_size=…` | optional | the function it names lives in `ruff_memory_usage`, which is not one of your dependencies. Memory reporting only |
| `lru=200` | **no** | an LRU on a cheap owned value buys nothing; `parsed_module` has one because ASTs are large |

**`#[derive(PartialEq)]` on `DefNode` is load-bearing.** Salsa's default
comparison is literally `old_value == new_value`
(`salsa-macros/src/tracked_fn.rs:103-115` **[verified]**), so without it the
macro will not compile — and with `no_eq` it would compile and silently stop
backdating.

Be honest about how much backdating buys here: `DefNode` carries `range`, so any
edit that shifts bytes inside a def changes the value. It fires for edits
*outside* every def (a trailing comment, a changed import), and for a re-sync
that did not actually change the file. The reliable win is the per-file cache —
edit one file in a 200-file project and only that file is re-walked.

**The dependency you must add.** `salsa` is a transitive dependency today; a
`#[salsa::tracked]` of your own needs it as a direct one, and at the *same*
build. Copy ruff's line verbatim rather than writing `salsa = "0.28"`:

```toml
salsa = { version = "0.28.2", default-features = false, features = [
    "compact_str", "macros", "salsa_unstable", "inventory",
] }
```

Defaults would add `rayon` and `accumulator`; cargo's feature unification is
additive, so they would switch on for ruff's copy too. Same version, same
features, one build.

**This works across crates — verified, not assumed.** `ty_ide` defines
`#[salsa::tracked] fn symbols_for_file(db: &dyn Db, file: ProgramFile<'_>) ->
FlatSymbols` over `ty_project::Db`, a trait from another crate
(`ty_ide/src/symbols.rs:394-402`) **[verified]**. That is the same shape as
`definitions`: a tracked query in a downstream crate, over a foreign `Db`,
returning owned data built from `parsed_module`.

> **It is almost the query you want, already written.** `symbols_for_file` is
> `pub(crate)`, but `ty_ide::document_symbols(db, program_file) -> &FlatSymbols`
> is public (`ty_ide/src/document_symbols.rs:6` **[verified]**) and wraps it.
> It gives you nested symbols with `full_range` and a `SymbolKind` to filter on
> — enough to prototype stage 2 before you write a visitor. What it does not
> give you is the docstring slot, the `ID:`, or a shape you control, and it
> indexes imports, assignments and parameters you would have to filter out every
> time. Prototype with it; ship your own.

### What stage 1 deletes, and what survives

| object 5 mechanism | in stage 1 |
|---|---|
| `enter_node` / `TraversalSignal::Skip` | **gone** — not calling the walk is the skip |
| the root guard (`node.range() == self.root`) | **gone** — no visitor ever starts at its own def |
| `ExprLambda => Skip` (quirk 8) | **gone** — expressions are never entered |
| empty `visit_decorator` | **gone** — decorators are not statements |
| position dedup (quirk 10) | **gone** — one push per `visit_stmt` |
| `walk_node` vs `walk_body` (the default-args trap) | **gone** — there are no calls to lose |
| `DECORATORS_IN_RANGE` (object 3) | **survives** — `stmt.range()` still starts at the `@` |
| `id_from_body` (object 6) | **survives**, unchanged |
| source order | **survives**, for free |

Six of the nine rows are gone because they all served one goal: finding calls
without falling into a nested scope. Object 5 is not wasted — stage 3 inherits
every one of those rules as evaluation semantics, and the *standalone*
`parse_file` tree still uses `ScopeScanner` as written. But the pipeline's first
traversal no longer carries any of it.

---

## Stage 2 — generate IDs, write them, rescan

### The docstring is the storage

There is no ID table. `read_or_inject_file_id`, the injector and the call tree
all agree on one source of truth: the `ID:` line in the def's own docstring.
Writing the ID into the source **is** the save, and
`plan/00-orientation/01` §"Side effects" says `parse_file` is expected to
rewrite the file when one is missing.

So stage 2 is: for every `DefNode` with `id == None`, mint a UUID, build the
text, insert it at `doc`.

### One insert per def, two shapes

Both shapes are **pure insertions** — one offset, one string, no byte outside
the insert touched.

```rust
// The indentation to write at, and whether `body[0]` even starts its own line.
let indent = line_prefix(source, slot.stmt_start);

let text = match (slot.kind, indent) {
    // 1. a docstring exists: extend it in place
    (SlotKind::InLiteral { triple_quoted: true }, Some(ind)) =>
        format!("\n\n{ind}ID: {uuid}\n{ind}"),
    (SlotKind::InLiteral { triple_quoted: false }, _) =>
        format!(" ID: {uuid}"),                 // one line: a newline would not parse
    (SlotKind::InLiteral { triple_quoted: true }, None) =>
        format!(" ID: {uuid}"),                 // `def f(): """doc"""` — stay on the line

    // 2. no docstring: a whole new statement before body[0]
    (SlotKind::NewStatement, Some(ind)) =>
        format!("\"\"\"ID: {uuid}\"\"\"\n{ind}"),
    (SlotKind::NewStatement, None) =>
        format!("\n{one_in}\"\"\"ID: {uuid}\"\"\"\n{one_in}"),   // the one-liner
};
```

```rust
/// The whitespace between `at`'s line start and `at` — the indentation to reuse
/// **verbatim**, tabs and all. `None` when `at` is not the first thing on its
/// line, which is the `def f(): body` one-liner and the only case that has to
/// synthesise an indent (`one_in` above: the def's own indent, plus one unit).
fn line_prefix(source: &str, at: TextSize) -> Option<&str> {
    let at = at.to_usize();
    let line_start = source[..at].rfind('\n').map_or(0, |nl| nl + 1);
    let prefix = &source[line_start..at];
    prefix.bytes().all(|b| b == b' ' || b == b'\t').then_some(prefix)
}
```

### ★ Why inserting before the closing quote is always safe

The obvious worry is escapes: insert after a trailing `\` and you change what
the string means. **That input cannot exist.** Python forbids a literal from
ending in an odd number of backslashes — the backslash would escape the closing
quote. Measured on CPython 3.12.2:

| source | `__doc__` before | after inserting before the closer |
|---|---|---|
| `"""abc\\"""` (escaped backslash) | `'abc\\'` | `'abc\\ ID: u1'` ✓ |
| `r"""abc\"""` (raw, trailing `\`) | **SyntaxError: unterminated** | input is impossible |
| `"abc"` (single-quoted docstring) | `'abc'` | `'abc ID: u1'` ✓ |
| `"ab" "cd"` (implicit concat) | `'abcd'` | `'abcd ID: u1'` ✓ |

The single-quoted row is why `SlotKind` carries `triple_quoted`: a newline
inside a single-quoted string is a syntax error, so that shape appends on the
same line. `_extract_metadata`'s `(\S+)\s*:\s*(\S+)` reads `ID: u1` from the
middle of a line perfectly well (object 6), so nothing downstream notices.
`docstrings.py:single_quotes` is the fixture.

The implicit-concat row is why `in_literal` takes `value.as_slice().last()` and
not the first part: the last part is the one holding the closing quote.
`docstrings.py:implicit_concat` is the fixture, and note it splits its two parts
across a line continuation — the AST does not care, and neither does the insert,
because the offset comes from the last part's own range.

### ⚠ When `line_prefix` returns `None` — the one-liner

`edges.py:one_liner` is the fixture (`def one_liner(): build()`), and it is the
only case where the insert has to add a line break:

```python
def one_liner(): return build()
```

`body[0]` starts on the *same line* as `def`, so inserting a docstring statement
before it without a newline produces `def f(): """ID: u1"""` followed by an
orphaned indented line. Insert `\n{indent}` first and it parses, with the
original statement riding along behind the docstring:

```python
def one_liner(): 
    """ID: u1"""
    return build()
```

Measured: `__doc__ == 'ID: u1'`, and the function still returns. A semicolon
body (`def g(): a = 1; return a`) works the same way — the whole compound line
moves down as one. The trailing space after the colon is cosmetic; a formatter
removes it on the next commit.

Both questions — "is this a one-liner?" and "what indentation do I write?" —
are the same lookup, which is why `line_prefix` answers them together:

- **`Some(prefix)`** — `body[0]` begins its line, and `prefix` is the exact
  bytes to reuse. Reusing them **verbatim** preserves tabs, spaces and width
  with no style detection at all.
- **`None`** — something precedes `body[0]` on its line, so this is the
  one-liner, and the insert must open a new line. This is the only case that has
  to *synthesise* an indent (the def's own indent plus one unit), and the single
  place `ruff_python_codegen::Stylist` would earn a new dependency.

Deriving both in stage 2 rather than storing them in `DocSlot` keeps stage 1
source-free: `doc_slot(body)` needs the AST and nothing else.

### ★ Bottom-up, by descending offset

An insert shifts every byte after it. Two rules, and the second follows from the
first:

**Within one file, apply inserts in descending offset order.** Each insert only
moves text *below* itself, so every offset you have not used yet is still valid.
No offset arithmetic, no fixups.

That order also puts **children before parents**, for free: a def's docstring
slot is at the top of its body, which is strictly before any nested def inside
it. A parent's insert shifts text its children's inserts already wrote — which
is fine, because those are bytes now, not ranges.

**Build the whole new file in memory, write once.** One `String`, one
`fs::write`, one `File::sync`. Writing per-def means N syncs, N revision bumps,
and N re-parses of a file you are still editing.

### The borrow checker enforces the rescan

> **Never carry a pre-write range into stage 3.** After the write, update the
> file in the db and call `definitions(db, file)` again.

In Rust you do not have to remember this, because the types collide:

```rust
let defs = definitions(db, file);     // &Vec<DefNode> — borrows db immutably
let new_source = apply_inserts(&source, defs);
std::fs::write(path.as_std_path(), &new_source)?;
file.sync(&mut db);                   // ← needs &mut: the shared borrow must be dead
let defs = definitions(&db, file);    // fresh ranges, recomputed by salsa
```

`File::sync(self, db: &mut dyn Db)` (`ruff_db/src/files.rs:478` **[verified]**)
takes `&mut`, and `returns(ref)` ties `defs` to the shared borrow. Hold the old
`defs` across the write and it does not compile.

The loophole to *not* take: cloning the defs out first. That compiles, and the
clone's ranges are stale. If you need the names across the write, clone the
names — never the ranges.

`plan/02-mapping/02` §"The salsa write ordering" has the other half: skip the
sync and every later query serves the pre-injection source, so IDs stay `None`
forever and nothing looks broken. Because `&mut db` cancels in-flight queries,
batch the whole request's writes before syncing once.

### ⚠ DECISION — insertion fixes three bugs `plan/02-mapping/02` says to preserve

That page's design is libcst: parse to a CST, insert a docstring node, codegen
the module back. It argues against "hand-rolled text edits" — and a pure
insertion *is* a hand-rolled text edit. The comparison is no longer the one that
page makes, though, because inserting at an offset is not a general edit:

| | libcst codegen | insert at an offset |
|---|---|---|
| bytes outside the docstring | re-emitted (preserved, but re-emitted) | **provably untouched** |
| `r"""…"""` prefix | `id_injector.py:70` emits `f'"""{content}"""'` — **drops the `r`**, changing the string's meaning | preserved; the prefix is never read |
| `"""` inside the content | rebuilt literal is **broken** | impossible; nothing is rebuilt |
| quote style, indent, line endings | from `Stylist`, or you mismatch the file | inherited from the bytes you did not touch |
| new dependency | `libcst` 1.8.4 (+ maybe `ruff_python_codegen`) | none, except `uuid` |
| byte-parity with the Python driver | reproduces both bugs | **diverges on exactly those inputs** |

The last two rows are the trade. `plan/02-mapping/02` says "reproduce them, do
not fix them" about the prefix loss and the broken-`"""` rebuild — a defensible
rule for *output* parity, and the wrong rule here, because this output is the
user's source file and the Python's version of it is lossy. Reproducing a bug
that corrupts `r"""..."""` is not parity, it is damage.

**Recommended:** take the insertion, and record the divergence where the parity
gate can see it — `plan/04-build/02-milestones.md`'s byte-identical test cannot
cover a def with an `r` prefix or a `"""` in its docstring, because the two
implementations disagree *by design*.

Give the divergence fixtures rather than a footnote. `docstrings.py:raw` is not
one: it already carries an `ID:`, so injection never touches it. You need the
*no-ID* variants — a `raw_no_id` with `r"""…"""` and no key, and a
`triple_in_content` whose docstring contains `\"\"\"`. Both are inputs the Python
driver damages and yours does not.

Still owed either way, and unchanged by this choice: `_build_docstring`'s
key-replacement scan (for a def that has *some* metadata but no `ID:`), the
`FileID:` / `FolderID:` module-level variants, `__init__.py` creation, and
idempotence — run twice, the second run writes nothing and reports
`modified: false`.

---

## Stage 3 — resolve calls by walking them, not collecting them

The interpreter evaluates a def's body. A call is found when `eval` reaches an
`ExprCall`. **There is no list.**

```rust
fn eval(&mut self, expr: &Expr, env: &Env<'db>, frame: &mut Frame<'db>) -> AbstractValue<'db> {
    match expr {
        Expr::Call(call) => self.resolve_call(call, env, frame),   // ← the whole "collection"
        Expr::Attribute(a) => { … }
        …
    }
}
```

`plan/03-call-tree/03`'s `walk_body` currently reads
`for call in collect_calls_in_scope(func) { self.resolve_call(...) }`. That is
the line this chapter removes: a pre-collected list cannot carry the environment
the call was reached in, and the order it imposes is source order rather than
the order the interpreter actually gets there.

### Object 5's `Skip` rules become statement semantics

Every rule the scanner expressed as a traversal signal is a rule about what
*evaluating* a statement does. Nothing is lost; it moves:

| scanner rule | the same rule in `eval` |
|---|---|
| `StmtFunctionDef => Skip` | a `def` statement evaluates its **decorators and defaults now**, binds a function value, and leaves the body unevaluated |
| the body is walked later, by a fresh visitor | the body is evaluated when something **calls** that value — in a child frame, with a child env |
| `ExprLambda => Skip` | a `lambda` evaluates to a closure. Its body runs when the closure is called, in the closure's env |
| empty `visit_decorator` (parso's enclosing-scope rule) | a decorator expression is evaluated in the **enclosing** env, at the `def` statement — which is where it lexically is |
| default args belong to the function | a default expression is evaluated in the enclosing env too, once, when the `def` runs |
| nested class terminates the scan | a `class` statement evaluates its body in a new class scope, then binds the class value |

Two of those rows are *more* correct than the scanner's version, because
evaluation order is a real thing and traversal order was a proxy for it. The
decorator row in particular settles object 5 part 4's "still open" item: a call
inside a decorator belongs to the scope the `def` statement is in, which is
parso's answer, reached without a special case.

### Reading the target's ID

Resolution ends at a definition, and the ID comes out of that definition's own
docstring — the same scan, from whichever handle you are holding:

| you hold | use |
|---|---|
| a `&[Stmt]` body (you walked there) | `id_from_body(body)` — object 6 |
| a ty `Definition` (you resolved there) | `definition.docstring(db)` (`ty_python_core/src/definition.rs:157` **[verified]**), then the metadata scan |
| a `DefNode` from stage 1 | `node.id` — already extracted |

**The source is the lookup.** Quirk 4's "no ID → no node, and do not descend"
is then one `let Some(id) = … else { continue }`, exactly as
`plan/03-call-tree/03` step (d) has it.

---

## The one index you do need: ID → location

v-noc asks for the call tree of function X by **ID**. Stage 1 gives you
ID-from-location; the request needs location-from-ID.

```rust
pub type IdIndex = FxHashMap<String, Vec<(File, TextRange)>>;

/// Merge every file's `definitions` into one ID lookup.
pub fn id_index(db: &dyn Db) -> IdIndex {
    let mut out: IdIndex = FxHashMap::default();
    for file in db.project().files(db).iter() {
        collect(definitions(db, file), file, &mut out);   // the per-file query is cached
    }
    out
}

fn collect(defs: &[DefNode], file: File, out: &mut IdIndex) {
    for def in defs {
        if let Some(id) = &def.id {
            out.entry(id.clone()).or_default().push((file, def.range));
        }
        collect(&def.children, file, out);                // nested defs have IDs too
    }
}
```

### ★ The value is a `Vec`, not a single entry

Copying a function copies its docstring, so **two defs can carry the same ID**.
A `Vec` makes that visible; a `HashMap<String, (File, TextRange)>` silently
keeps whichever file was merged last — the same last-wins failure as the old
metadata regex (object 6's `multiple_keys`), one layer up.

What to do with a duplicate is a product question, not a data-structure one:
report it, pick the first, or refuse. All three need the second entry to exist.

### ⚠ The merge stays *outside* salsa — and ty agrees

The tempting version is `#[salsa::tracked] fn id_index(db) -> IdIndex` keyed on
`Project`. Two reasons not to, the second verified:

1. Its dependency set is *every file's* `definitions`, so it re-runs whenever
   any file changes — which is the normal case. It is only reused when every
   file backdates, and `DefNode.range` means edited files do not backdate.
2. `Project::files(db)` is not a query. It is a plain method over a lazily
   populated file index (`ty_project/src/lib.rs:754`), and ty keeps it out of
   tracked code: **`workspace_symbols` — ty's own "merge every file's symbols"
   — is a plain `pub fn`**, not a tracked query, iterating `project.files(db)`
   and calling the per-file tracked `symbols_for_file`
   (`ty_ide/src/workspace_symbols.rs:8`, `:19-24`) **[verified]**.

That is the pattern to copy exactly: **per-file tracked, cross-file merge
plain.** The expensive part — parse plus walk — is cached per file; the merge is
hash inserts over data already in memory. ty also parallelises it with
`ty_project::parallel::ParallelIteratorExt::map_with_db`
(`workspace_symbols.rs:1`, `:22-24`), which is the right move once the corpus is
200 files.

### Keep it in the analyser

The source is the truth; the graph is derived from it. An ID index in TerminusDB
is a second copy that can disagree with the files on disk, and the disagreement
surfaces as a call tree with a missing branch. Rebuild it from `definitions` —
that is what the per-file cache is for.

---

## What this does to the RPC surface

Stage 1 emits defs. The `parse_file` wire shape
(`plan/00-orientation/01`) emits defs **and calls**. Three ways to square that,
and this is a decision to make against v-noc, not from here:

| | `parse_file`'s `nodes` | cost |
|---|---|---|
| **keep calls** ← safest | stage 1 for defs; `ScopeScanner` (object 5, `src/scan.rs`) still runs for the call nodes | two traversals on a `parse_file`, only one of which the pipeline consumes |
| **defs only** | drop `type: "call"` from the array | a wire change on top of the `call_index` / `call_col_pos` removal (object 5 part 4 §4). Anything in v-noc that renders call sites from `parse_file` breaks |
| **defs only, calls on demand** | add an RPC that returns the calls in one scope | new surface, but each caller gets what it asks for |

Note that `resolve_calls(file_path, calls)` already takes a `calls` argument —
call sites identified positionally. Starting the call tree from an **ID**
instead (which is what the index above is for) changes that signature too.
Check both against v-noc before M2, and write the answer down here.

---

## Exercise

**A.** Write `DefScanner` and run it on all five fixtures. Compare its def list
against `ScopeScanner`'s, filtering the latter to non-call nodes. They must
match exactly — same names, same nesting, same ranges. Any difference is a bug
in one of them; say which.

**B.** Add `walk_stmt(self, stmt)` to the def arm, on purpose. Predict what
`nested.py` does before you run it, then run it.

**C.** Delete the `_ => { walk_stmt(self, stmt); return; }` arm — return without
walking. Which fixture loses a def, and which object-5 bug is this the same
shape as?

**D.** Prove the lambda claim. Write a Python file with a `def` you believe is
inside an expression. Then write `lambda: "ID: fake-uuid"` and check what
`__doc__` is on the resulting function object.

**E.** Make `definitions` a tracked query. Then call it twice on the same file
and confirm the second call does not re-parse (a `tracing` span or a counter in
the scanner both show it). Now `touch` the file, sync, and call again.

**F.** Backdating: add a comment to the *end* of a fixture, below every def,
sync, and check whether `definitions`'s value compares equal. Then add a comment
*inside* a function body and check again. Explain the difference in one
sentence.

**G.** Write stage 2 for `docstrings.py`, which has twelve docstring forms and no
IDs on several of them. Apply the inserts bottom-up, write to a copy, and diff.
Every def must gain exactly one `ID:` line, and `python3 -c "import docstrings"`
must still work.

**H.** Run stage 2 twice. The second run must write nothing. If it writes, your
`id_from_body` cannot read what your inserter wrote — which is object 6's
"inject an ID it cannot read back" failure.

**I.** Try to hold `definitions`' result across `file.sync(&mut db)`. Paste the
borrow-checker error into your notes; it is the clearest statement of the
stale-range rule you will get.

**J.** Apply the inserts **ascending** instead. On a file with two defs, measure
how far off the second insert lands, and relate the number to the length of the
first inserted string.

**K.** Build `id_index` over the fixture directory. Then copy a function —
docstring and all — into a second file, rebuild, and confirm the ID has two
entries.

---

## Exam

**1.** Why does the pipeline not need a list of calls from the first traversal?
Give the reason that is about the *interpreter*, not about efficiency.

**2.** `StatementVisitor` never enters an expression. Prove that this loses no
ID-bearing definition.

**3.** `walk_stmt` has eight arms and a `_ => {}`. Why is the `_` arm complete
rather than a gap?

**4.** In `DefScanner`, what plays the part of `TraversalSignal::Skip`?

**5.** Name the four object-5 mechanisms stage 1 deletes, and say what single
goal all four served.

**6.** Why must `DefNode` own its data? Name the salsa requirement and the
non-salsa reason.

**7.** `#[salsa::tracked(returns(ref), no_eq, lru=200)]` is `parsed_module`'s
attribute list. Which of the three do you copy, and what does each do?

**8.** `parsed_module(db, file)` where `file: File` does not compile. What does
it want, why does it want that, and what are the two hops?

**9.** Within one file, inserts are applied in descending offset order. What
does that guarantee, and what does it give you for free about parent/child defs?

**10.** Inserting before a docstring's closing quote cannot break an escape
sequence. Why not? Which input would you expect to break it, and what does
Python do with that input?

**11.** `edges.py:one_liner` needs a different insert from every other def.
What, and why?

**12.** You must not carry a pre-write range into stage 3. What makes that a
compile error rather than a convention, and what is the loophole?

**13.** The ID index maps to `Vec<(File, TextRange)>`. What breaks if it maps to
a single `(File, TextRange)`, and what in the real world produces that case?

**14.** Should the cross-file merge be a salsa query? Give the dependency-set
argument and the ty precedent.

**15.** Stage 3 drops `collect_calls_in_scope`. For each of lambdas, decorators
and default arguments, say what rule replaces the scanner's `Skip`.

**16.** Stage 2 diverges from the Python injector on two inputs. Name them, say
which implementation is wrong, and say what that costs the M2 gate.

---

## Answers

**1.** Because a call's meaning depends on the path taken to it. The interpreter
reaches an `ExprCall` with an environment — resolved parameter values, a bound
`self`, a frame chain for the cycle guard. A statically collected list has none
of that, so you would still have to evaluate the body to use it; the list is
built, ignored, and rediscovered.

**2.** Python has exactly one definition form that is an expression, `lambda`,
and a lambda's body is an expression, so it cannot contain a statement and
cannot hold a docstring (`lambda: "x"` *returns* the string;
`__doc__` is `None`). Every `def` and `class` is therefore reachable through
statements alone, and only ID-bearing definitions matter.

**3.** The eight arms are exactly the `Stmt` variants that own a body
(`FunctionDef`, `ClassDef`, `For`, `While`, `If`, `With`, `Match`, `Try`). `Stmt`
has 25 variants; the remaining 17 contain no statements, so there is nothing
below them to visit.

**4.** **Not calling `walk_stmt`.** The def arm walks the body with a *fresh*
scanner and returns, so the enclosing scanner never sees inside. Object 5 got
one-visitor-per-scope from `Skip`; stage 1 gets it from the default walk being
opt-in. Same structure, opposite direction.

**5.** `enter_node`/`Skip`, the root guard, the `ExprLambda` arm, the empty
`visit_decorator` (and position dedup, and the `walk_node`-vs-`walk_body`
choice). All of them served one goal: **finding calls without falling into a
nested scope.** Stage 1 is not looking for calls.

**6.** Salsa requires a tracked function's output to be `'static` or implement
`SalsaValue` (`salsa_value.rs`), and `Vec<DefNode>` qualifies only because every
field is owned. The non-salsa reason is the same one as object 5 part 3's answer
15: the value outlives the `ParsedModuleRef`, and salsa may clear and re-parse
the AST underneath you.

**7.** Copy `returns(ref)` — it hands out a reference instead of cloning the
whole def list on every read. Do **not** copy `no_eq`: it exists because a
`ParsedModule` cannot be compared, and switching off comparison switches off
backdating, which is the only reason to derive `PartialEq`. Do not copy
`lru=200` either; an LRU is for large values like ASTs.

**8.** It wants a `PythonFile<'_>`, an interned `(File, PythonVersion)` pair, so
one file parsed for two target versions in a single revision does not share an
incompatible AST. The hops are `db.program_file(file)` then `.python_file(db)`.

**9.** That every offset not yet used is still valid, because an insert only
shifts bytes after itself. For free, it orders children before parents: a def's
docstring slot sits at the top of its body, strictly before any nested def, so
descending offset processes the nested ones first.

**10.** Because a Python literal cannot end in an odd number of backslashes —
the backslash would escape the closing quote. The input you would fear is
`r"""abc\"""`, and Python rejects it with *unterminated triple-quoted string
literal*. `"""abc\\"""` is the legal version, and inserting after the escaped
pair is harmless.

**11.** Its `body[0]` is on the same line as `def`, so the inserted docstring
statement must be preceded by a newline and an indent. Without them you get
`def f(): """ID: u"""` with the original statement orphaned on an indented line
below. With them, the original statement moves down behind the docstring and the
function still runs.

**12.** `definitions` returns `&Vec<DefNode>` borrowed from `&db`, and
`File::sync` takes `&mut dyn Db`. The shared borrow must be dead before the
write, so holding it is rejected by the borrow checker. The loophole is cloning
the defs out first — which compiles, and whose ranges are stale. Clone names if
you must; never ranges.

**13.** A copied function brings its docstring, and therefore its ID, so two
defs legitimately share one. A single-valued map keeps whichever file merged
last and the other def silently disappears from every lookup — the same
last-wins bug as `_extract_metadata`'s repeated keys, one layer up. A `Vec`
makes the collision data instead of a coin flip.

**14.** No. Its dependency set is every file's `definitions`, so it re-runs on
any change, and `DefNode.range` stops edited files from backdating — there is
almost nothing to reuse. And `Project::files` is a lazily populated index, not a
query: ty's own cross-file merge, `workspace_symbols`, is a plain `pub fn` that
iterates `project.files(db)` and calls the per-file tracked query. Per-file
tracked, cross-file merge plain.

**15.** A `lambda` evaluates to a closure whose body runs only when the closure
is called. A decorator expression is evaluated in the **enclosing** environment
at the `def` statement — which lands its calls in the enclosing scope, parso's
answer, with no special case. A default expression is likewise evaluated in the
enclosing environment when the `def` runs, but it is part of the function's own
signature, so the call is attributed to the function.

**16.** A docstring with an `r` prefix, and a docstring containing `"""`. The
Python is wrong on both — it re-emits `f'"""{content}"""'`, dropping the prefix
(changing the string's meaning) and producing a broken literal in the second
case. Insertion is correct and therefore **differs**, so the M2 byte-identical
gate cannot cover those two inputs; add both to `docstrings.py` so the
divergence has a fixture.

---

## Done when

- [ ] `definitions(db, file)` is a tracked query returning owned `Vec<DefNode>`
- [ ] its def list matches `ScopeScanner`'s, filtered to non-calls, on all five
      fixtures
- [ ] no lambda, decorator or argument expression is reachable from stage 1
- [ ] every `DefNode` carries the offset stage 2 writes at, and `None` where
      there is nowhere to write
- [ ] stage 2 applies inserts descending, writes once, syncs once, and re-reads
      `definitions` afterwards
- [ ] stage 2 is idempotent, and the second run reports `modified: false`
- [ ] `docstrings.py` survives injection and still imports
- [ ] the `r"""` and `"""`-in-content divergences are written down with fixtures
- [ ] `id_index` is a plain function over the per-file query, and its value is a
      `Vec`
- [ ] the `parse_file` call-node decision is recorded, with v-noc checked
- [ ] `plan/03-call-tree/03`'s `collect_calls_in_scope` line is replaced by
      `eval` reaching `Expr::Call`

---

→ [`exam.md`](exam.md), then
[`../03-the-database/README.md`](../03-the-database/README.md)
