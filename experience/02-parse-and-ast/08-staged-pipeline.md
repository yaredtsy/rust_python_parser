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
through `walk_stmt`'s `If` arm), and `edges.py`'s `default_args` (whose two
calls are in the signature — invisible here, and correctly so).

### `DefNode` — owned, because the parse does not outlive the query

```rust
use ruff_text_size::TextRange;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefNode {
    /// The raw UUID from `ID:`, or `None` if this def has no ID yet.
    pub id: Option<String>,
    pub name: String,
    pub kind: DefKind,
    /// `stmt.range()` — decorators included (object 3, `DECORATORS_IN_RANGE`).
    pub range: TextRange,
    pub children: Vec<DefNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefKind { Function, Class }
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
- **★ No docstring-slot fields.** An earlier version of this page carried the
  insert offset, the quote style and the indentation anchor here, so stage 2
  could read them off the cached outline. Do not: those fields make every cached
  `DefNode` bigger *and* they join salsa's `old == new`, so reflowing a docstring
  — which changes nothing about the outline — would invalidate the outline and
  everything under it. Stage 2 derives them in its own pass, from the same
  cached parse. [Exercise 10, object 4](../10-lossless-edits/04-object-purity.md)
  §3 is the full argument.

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

### ★ The def-finding rule is extracted, because stage 2 needs the same one

`DefScanner` above pushes a `DefNode` in the same `match` arm that finds the
def. That is one rule in one place, which is fine until stage 2 needs it too —
and stage 2 does, because it has to visit exactly the defs stage 1 reported.

Two walks that answer "which defs exist" separately can disagree, and the
failure is silent and permanent: a def collection sees and injection misses
sits in the outline with `id: null` forever. So pull the walk out and let both
passes call it:

```rust
pub fn child_defs<'a>(body: &'a [Stmt]) -> Vec<Def<'a>>;   // the ONE rule
```

Then stage 1 is a map over it, and stage 2 is a different map over it:

```rust
fn outline(body: &[Stmt]) -> Vec<DefNode> {
    child_defs(body)
        .into_iter()
        .map(|d| DefNode {
            id: id_from_body(d.body()),     // object 6
            name: d.name(),
            kind: d.kind(),
            range: d.range(),
            children: outline(d.body()),    // same rule, one level down
        })
        .collect()
}
```

`Def` is a two-variant enum (`Function`/`Class`) rather than an `AnyNodeRef`,
which is what makes `d.body()` and `d.range()` total functions instead of
matches with an `unreachable!()` arm — object 5 part 4's
`node_for_definition` has that arm, and this is where it goes away.

[Exercise 10, object 1](../10-lossless-edits/01-object-def.md) is the full
chapter: the `Def` impl, the sixteen-def agreement fixture, and why the *reader*
(`id_from_body`) has to be shared as well as the walk.

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
edit that *shifts bytes* changes the value — and an edit near the top of a file
shifts every def below it. It fires in two narrow cases: an edit **after the
last def** (a trailing comment, a newline at EOF), and an edit that keeps every
byte count identical (`x = 1` → `x = 2`). A changed import is **not** one of
them: if the line's length changes, every def range below it moves and
`old != new`. The reliable win is the per-file cache — edit one file in a
200-file project and only that file is re-walked.

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

> ★ **Superseded by the fork.** `pylspt` is now a member of the fork's
> workspace (`plan/01-crates/04`, rule 6), so the whole paragraph above
> collapses to one line:
>
> ```toml
> salsa = { workspace = true }
> ```
>
> You inherit ruff's entry — same version, same features, one build, nothing to
> keep in sync. Rule 6 exists for exactly this.

The reasoning is kept because it is what you need if you ever consume ty as a
plain git dependency again. The danger is two *different* salsa crates in one
graph: their types are unrelated, so `#[salsa::tracked]` over ty's `Db` will
not compile against them. At `ac201b8` ruff takes salsa from **crates.io**
(`Cargo.toml:165`) with **no `[patch]` section** **[verified]**, so a matching
crates.io dependency unifies to one build — and if a later rev pins salsa by
git rev, the version string stops being enough and you must point at the same
source.

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

> **This stage has its own exercise.**
> [`experience/10-lossless-edits`](../10-lossless-edits/README.md) is four
> objects on it: the shared def rule, the docstring slot, the edits, and why it
> cannot be a salsa query. What follows is the summary the pipeline needs.

### The docstring is the storage

There is no ID table. `read_or_inject_file_id`, the injector and the call tree
all agree on one source of truth: the `ID:` line in the def's own docstring.
Writing the ID into the source **is** the save, and `plan/00-orientation/01`
§"Side effects" says `parse_file` is expected to rewrite the file when a key is
missing.

### It is a separate pass, and it must share one thing with stage 1

Injection cannot live inside `definitions`: it writes files and it mints
**random** UUIDs, so salsa's `old == new` is never true and the same def could
be handed two IDs on two reads. And a query can be **cancelled mid-flight** by
another thread taking `&mut db` — survivable for a pure query, not for a half-
written file.

What it must share is **"which defs exist"**. Give the two passes separate walks
and they can disagree: a def collection sees and injection does not gets
`id: null` forever, never becomes a graph node, and reports nothing. So the walk
is extracted into one function both call —
[`child_defs`](../10-lossless-edits/01-object-def.md), with
`python/shared_rule.py`'s sixteen defs as the agreement test. The reader
(`id_from_body`) is the other half of that rule, and is shared too.

### Plan, then apply, bottom-up

```rust
let edits = plan_ids(&module.syntax().body, &source, &mut || Uuid::new_v4().to_string());
let new_source = apply(&source, edits);     // descending offsets, or a builder
```

`Edit { offset, text }` is **owned**, which is what lets it outlive the AST
borrow — and the write needs that, because `File::sync` takes `&mut db`. Each
def contributes exactly one insert, located just before its docstring's closing
quotes or before `body[0]` when there is no docstring. Inserting before a closer
can never corrupt an escape, because Python forbids a literal from ending in an
odd number of backslashes. Descending offsets keep every un-used offset valid,
and put children before parents for free.
[Objects 2 and 3](../10-lossless-edits/02-object-slot.md) have the eleven
shapes, the one-liner case, and the measured tables.

### The four-step order, and the borrow that enforces it

```
1. parse (cached)   2. plan_ids   3. apply → write → File::sync(&mut db)   4. definitions(file)
```

Step 4 is why a pre-write range never reaches the graph: the outline is always
computed from the post-write text. And it is not a convention —
`definitions` is `returns(ref)` so its result borrows the db, while
`File::sync` takes `&mut dyn Db` (`ruff_db/src/files.rs:478` **[verified]**), so
**holding an outline across the write does not compile**. The loophole to avoid
is cloning it first: that compiles, and the clone's ranges are stale.

Skip the sync and every later query serves the pre-injection source: IDs stay
`None`, injection looks like a no-op, and the file on disk is visibly correct.
Batch the writes and sync once — `&mut db` cancels in-flight queries.

### ⚠ This diverges from `plan/02-mapping/02`

That chapter designs injection around the `libcst` crate and says to *reproduce*
two pre-existing bugs (a dropped `r` prefix, a broken rebuild when the docstring
contains `"""`). Insertion fixes both by never rebuilding the literal, which is
the right call for output that is **the user's source file** — and it means those
two inputs are no longer byte-comparable against the Python driver.
[Exercise 10, object 5](../10-lossless-edits/05-putting-it-together.md) has the
comparison table and the two fixtures that mark the gap.


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
| default args belong to the function | a default expression is evaluated in the **enclosing** env, once, when the `def` runs — so its calls land in the enclosing scope, **not** in the function. ⚠ This is a divergence from parso, which nests defaults inside the funcdef |
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

### ★ DECIDED: defs only

This driver is new orchestration, not a drop-in replacement for the current
Python backend's `parse_file` consumers. Calls come from the call tree
(stage 3), not from `parse_file` — `parse_file` emits defs only.

That matters because it is not a free choice against the *current* backend.
The old chain — `body_parser.py`'s `_traverse_and_collect` reads `type ==
"call"` nodes out of `parse_file`'s array, `call_graph/builder.py`'s
`resolve_call_hierarchy` skips `resolve_calls` entirely when that list is
empty, and `diff_calulator.py` then diffs an empty tree against whatever call
nodes are already stored — would read an empty call array as "every call was
deleted" and remove every stored call node from the graph on the first sync.
That is the real cost of "defs only" against that backend, and it is why the
table above called this a decision to check against v-noc rather than a
cleanup.

It is not a problem here because nothing routes this driver's `parse_file`
output through that chain. If that ever changes — this driver serving the
current backend before stage 3 exists — `parse_file` needs its calls back,
and the way to do it **without** reopening the two-walkers risk
(`experience/10-lossless-edits/01-object-def.md`) is a call finder scoped to
one def's body at a time: given a body `DefScanner` (object 8) already found,
walk its expressions, collect `ExprCall`s, stop descending at a nested
`FunctionDef`/`ClassDef`/`Lambda`, and never record a def itself. It finds
calls inside defs `DefScanner` already decided exist; it does not re-decide
which defs exist.

`src/scan.rs`'s `ScopeScanner` — the thing that used to put call nodes into
`parse_file` — is deleted, not merely unused. Recover it from git history if
this decision is revisited.

---

## Exercise

**A.** Write `DefScanner` and run it on all five fixtures. Compare its def list
against `ScopeScanner`'s, filtering the latter to non-call nodes. They must
match exactly — same names, same nesting, same ranges. Any difference is a bug
in one of them; say which.

> Non-call is the only filter you need: `ScopeScanner` never emits a lambda
> node. Quirk 8 `Skip`s `ExprLambda` **and records nothing**, so there is no
> `<lambda>` row to exclude (`src/scan.rs`, object 5 part 2). If you ever make
> lambdas visible for some other reason, this check needs a second filter.

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

**G.** Extract `child_defs` and make *both* `definitions` and `plan_ids` call it.
Then diff the two def lists on every fixture in this exercise and in
`../10-lossless-edits/python/`. They must be identical; a difference is the
silent-`id: null` bug waiting to happen.

**H.** Try to hold `definitions`' result across `file.sync(&mut db)`. Paste the
borrow-checker error into your notes; it is the clearest statement of the
stale-range rule you will get. Then defeat it with `.clone()` and print the
cloned ranges against the fresh ones.

**I.** Run stage 2 over `docstrings.py` — twelve docstring forms, several
without keys — then run it again. The second run must write nothing. If it
writes, your reader cannot read what your writer wrote, which is object 6's
"inject an ID it cannot read back" failure.

> **J.** The rest of stage 2 — every insert shape, the one-liner, the ordering,
> the guards — is [exercise 10](../10-lossless-edits/README.md), which has ten
> fixtures and four objects' worth of exercises for it. Do those there rather
> than here.

**K.** Build `id_index` over the fixture directory. Then copy a function —
docstring and all — into a second file, rebuild, and confirm the ID has two
entries.

---

## Exam

Questions 9–11 and 16 are stage-2 questions; [exercise
10](../10-lossless-edits/exam.md) asks them again with ten fixtures behind
them. Answer them here from the pipeline's point of view.

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
answer, with no special case. A default expression is evaluated the same way and
in the same place, so **its calls land in the enclosing scope too**.

That last one is a change of answer, not a restatement: the AST scanner
attributes a default-arg call to the function whose signature it is in (object 5
part 2, matching parso), and the interpreter cannot, because the only frame in
existence when the `def` statement runs is the enclosing one. Defaults and
decorators end up in the same place for the same reason — both are evaluated
*at* the `def`, not inside it. Record it as a parity divergence alongside the
decorator one.

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
