# Object 1 — `parsed_module`, `ParsedModule`, `ParsedModuleRef`, `ModModule`

Getting the tree. Three types for one job, and there is a reason.

---

## What it is

```rust
use ruff_db::parsed::parsed_module;

let parsed = parsed_module(&db, program_file.python_file(&db));   // ParsedModule
let module = parsed.load(&db);                                    // ParsedModuleRef
let ast: &ModModule = module.syntax();                            // the tree
```

Three steps, three types:

| type | what it is |
|---|---|
| `ParsedModule` | a **handle** to the parse. May not hold the tree right now. |
| `ParsedModuleRef` | the tree, **materialised**, for as long as you hold this. |
| `ModModule` | the root AST node. Its `body` is a `Vec<Stmt>`. |

**Rust note.** `ParsedModuleRef` implements `Deref<Target = Parsed<ModModule>>`
**[verified, `ruff_db/src/parsed.rs:208`]**, so `.syntax()`, `.tokens()` and
`.errors()` work on it directly — you do not need `.module()` or any unwrapping.

---

## Why `.load()` exists

Because **ty can throw the AST away**.

Under memory pressure, a `ParsedModule` drops its tree and re-parses when asked
again (`ParsedModule::clear()` is public **[verified]** — that is the mechanism).
If `parsed_module` handed you a `&ModModule` directly, that borrow would pin the
tree in memory for as long as it lived, and the optimisation would be
impossible.

So the design is: the handle is cheap and long-lived; `.load(db)` materialises
the tree and keeps it alive **only while you hold the `ParsedModuleRef`**.

Two practical rules follow:

1. **Call `.load()` where you need the tree, and let the ref drop.** Do not stash
   a `ParsedModuleRef` in a struct — same reasoning as `ProgramFile<'db>` in
   exercise 00, object 6.
2. **Do not call `.load()` in a tight loop** either. Load once per analysis, walk
   the whole tree, drop it.

---

## Why `parsed_module` takes a `PythonFile`

Recall exercise 00, object 6: parsing depends on the file **and the Python
version**, and nothing else. `PythonFile` is exactly that pair — so two projects
targeting 3.12 share one parse of the same file.

```rust
parsed_module(&db, program_file.python_file(&db))
//                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ not `file`, not `program_file`
```

Getting this wrong is a compile error, not a silent bug, so it costs you thirty
seconds once.

---

## ⚠ Never parse a database file yourself

```rust
// ✗ NO
let parsed = ruff_python_parser::parse_module(source)?;

// ✓ YES
let parsed = parsed_module(&db, program_file.python_file(&db)).load(&db);
```

Three separate reasons, each sufficient on its own:

1. **You bypass the cache.** `parsed_module` is `#[salsa::tracked]`; your call is
   not. You would re-parse on every request.
2. **You get the wrong Python version.** `parse_module` silently targets **3.10**
   **[verified]** — `PythonVersion::default()`. Exercise 00, object 7.
3. **The semantic layer will reject your nodes.** `AstNodeRef` is keyed on
   `(file, python_version)`, and there is a test literally named
   `rejects_module_parsed_for_different_python_version` **[verified,
   `ty_python_core/src/ast_node_ref.rs:155`]**.

Reason 3 is the one that would waste a day: you would hand ty a node from your
own parse and get either a panic or a wrong answer, with nothing pointing at the
cause.

**The one legitimate exception** is the `parse_file(file_path, content)` RPC,
where the editor sent unsaved text that has no `File`. Then you parse directly —
and you pass the version explicitly:

```rust
use ruff_python_parser::{parse_unchecked, Mode, ParseOptions};

let version = program_file.python_version(&db);
let parsed = parse_unchecked(
    &content,
    ParseOptions::from(Mode::Module).with_target_version(version),
);
```

`parse_unchecked` never returns `Err` — it recovers and reports errors
separately, which is what you want (see below).

---

## What you can do with it

### `parsed_module` and `ParsedModule` **[verified]**

| item | notes |
|---|---|
| `parsed_module(db, python_file)` | ★ the cached query |
| `.load(db)` | ★ → `ParsedModuleRef` |
| `.file()` | the `File` it came from |
| `.python_version()` | the version it was parsed at |
| `.clear()` | drop the tree (ty's memory management; you will not call it) |

### `Parsed<ModModule>` — via `Deref` on the ref **[verified]**

| method | returns | notes |
|---|---|---|
| `.syntax()` | `&ModModule` | ★★ the tree |
| `.tokens()` | `&Tokens` | ★ needed by `Stylist` in exercise 10 |
| `.errors()` | `&[ParseError]` | real syntax errors |
| `.unsupported_syntax_errors()` | `&[UnsupportedSyntaxError]` | ★ "this needs 3.12" |
| `.has_valid_syntax()` / `.has_invalid_syntax()` | `bool` | |
| `.has_syntax_errors()` | `bool` | includes unsupported-syntax ones |
| `.as_result()` | `Result<&Parsed<T>, &[ParseError]>` | if you prefer a `Result` |

### `ModModule`

```rust
pub struct ModModule {
    pub range: TextRange,
    pub body: Vec<Stmt>,     // ★ the top-level statements
}
```

That is it. The root node is a list of statements — object 2.

---

## Error recovery: the property that makes your driver possible

Ruff's parser is **error-recovering**. A file with a syntax error still produces
a usable tree, plus a list of errors.

```
def broken(:          ← syntax error
    return 1

def fine():           ← still parsed, still in the tree
    return 2
```

This matters because of quirk 13 (`plan/00-orientation/01`): *failures are
swallowed everywhere; a partial tree is a valid result*. Your driver must never
hard-fail on a broken file, and ruff's parser gives you that for free.

But it has a consequence people miss:

> **"Errors were reported" and "the tree is unusable" are different things.**

Check what you actually got before concluding a file is unparseable. Usually you
got most of it.

### Two kinds of error

| method | meaning | what you do |
|---|---|---|
| `.errors()` | genuine syntax errors | log at debug, keep the partial tree |
| `.unsupported_syntax_errors()` | valid Python, wrong target version | ★ log, then think about exercise 04 |

The second kind is your version-misconfiguration alarm. `type X = int` in a
project declaring `requires-python = ">=3.9"` lands here. Discarding these is
correct policy for an analyser; discarding them **without logging** is how you
lose a day to "why does that file have no functions".

---

## Example 1 — get the tree and count statements

```rust
use pylspt::db::{open, open_project};
use ruff_db::parsed::parsed_module;
use ruff_db::system::SystemPath;
use ty_python_semantic::Db as _;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: prog <dir> <file>");
    let file_arg = std::env::args().nth(2).expect("usage: prog <dir> <file>");

    let db = open_project(SystemPath::new(&dir))?;
    let (_file, program_file) = open(&db, SystemPath::new(&file_arg))?;

    let parsed = parsed_module(&db, program_file.python_file(&db)).load(&db);
    let ast = parsed.syntax();

    println!("top-level statements: {}", ast.body.len());
    println!("range:                {:?}", ast.range);
    println!("tokens:               {}", parsed.tokens().len());
    println!("syntax errors:        {}", parsed.errors().len());
    println!("unsupported syntax:   {}", parsed.unsupported_syntax_errors().len());

    for err in parsed.unsupported_syntax_errors() {
        println!("  ⚠ {err:?}");
    }

    Ok(())
}
```

Run it on:

```bash
# 4 top-level statements (docstring, import, and two defs — count them yourself)
cargo run --bin pylspt-dev -- ast experience/02-parse-and-ast/python \
                                  experience/02-parse-and-ast/python/nested.py

# the version fixture: same file, two projects, different outcomes
cargo run --bin pylspt-dev -- ast experience/04-python-version/python/proj-requires39 \
                                  experience/04-python-version/python/proj-requires39/app.py
cargo run --bin pylspt-dev -- ast experience/04-python-version/python/proj-tytoml313 \
                                  experience/04-python-version/python/proj-tytoml313/app.py
```

The last two are the **same bytes** in two projects. One resolves to 3.9 and
reports unsupported-syntax errors; the other resolves to 3.13 and does not.
Predict which, and predict whether the 3.9 run gets *no* tree or a *partial*
one.

---

## Example 2 — the dump command you will use in every later exercise

```rust
let parsed = parsed_module(&db, program_file.python_file(&db)).load(&db);
println!("{:#?}", parsed.syntax());
```

Every ruff AST node derives `Debug`, and `{:#?}` pretty-prints it.

```
ModModule {
    range: 0..1043,
    body: [
        Expr(
            StmtExpr {
                range: 0..104,
                value: StringLiteral( … ),
            },
        ),
        FunctionDef(
            StmtFunctionDef {
                range: 141..319,
                is_async: false,
                decorator_list: [],
                name: Identifier { id: "outer", range: 145..150 },
                …
```

**Add this as a real command now** (`pylspt-dev dump <file>`). It is the single
most useful debugging tool in this whole port: when your walk produces the wrong
tree, you look at the real one instead of guessing.

Pipe it through `head -100` — a real file's dump is thousands of lines.

---

## Exercise

**A.** Write the `ast` command from example 1. Run it on all five fixtures in
`experience/02-parse-and-ast/python/`. Record `body.len()`, token count, and both
error counts for each.

**B.** Write the `dump` command from example 2. Then use it to answer a question
you cannot answer any other way: in `python/edges.py`, find the
`StmtFunctionDef` for `decorated` and read its `range`. Does the range start at
the `@` or at the `def`? (Compute the byte offsets from the file to check.)

Write the answer down. It is object 3's headline trap, and finding it in the
dump yourself is worth more than being told.

**C.** Run the two version fixtures from example 1 and record the
unsupported-syntax errors. Did the 3.9 run produce an empty tree or a partial
one? How many of its four defs survived?

**D.** Try the wrong thing on purpose:

```rust
let parsed = ruff_python_parser::parse_module(source)?;
```

on `experience/04-python-version/python/pep695.py`. What errors do you get, and
why? Then explain — in one sentence — why this would have been much harder to
diagnose if you had hit it accidentally in exercise 07.

**E.** Print `parsed.tokens().len()` for `ascii.py` and for `unicode.py`. Then
add `--tokens` to your dump command and look at the first ten tokens of
`unicode.py`. You will not need tokens often, but exercise 10 needs them for
`Stylist`, so knowing they are one call away is useful.

---

## Exam

**1.** Name the three types in `parsed_module(db, f).load(db).syntax()` and what
each represents.

**2.** Why does `.load()` exist? What would be impossible without it?

**3.** What must you *not* do with a `ParsedModuleRef`, and which earlier object
follows the same rule?

**4.** Why does `parsed_module` take a `PythonFile` rather than a `File` or a
`ProgramFile`?

**5.** Give the three reasons never to call `ruff_python_parser::parse_module` on
a database file. Which one is hardest to diagnose?

**6.** What is the one legitimate case for parsing directly, and what must you
pass?

**7.** What does "error-recovering parser" mean, and which quirk of your Python
driver does it support?

**8.** Distinguish `.errors()` from `.unsupported_syntax_errors()`. What does your
driver do with each?

**9.** A file reports three unsupported-syntax errors. Can you still build a node
tree from it? How would you find out?

**10.** What is in `ModModule`?

---

## Answers

**1.** `ParsedModule` — a handle to the parse, which may not currently hold the
tree. `ParsedModuleRef` — the tree materialised, alive while you hold it.
`ModModule` — the root node, whose `body` is the top-level statements.

**2.** Because ty may **drop the AST under memory pressure** and re-parse later.
Handing out a `&ModModule` would pin the tree for the life of the borrow, making
that impossible. `.load()` separates "I have a handle" from "I need the tree
right now".

**3.** Do not store it in a long-lived struct — it keeps the tree alive and is
tied to one borrow of one revision. Same rule as `ProgramFile<'db>` (exercise
00, object 6) and `SourceCode` (exercise 01, object 4): **build it where you use
it, store owned keys.**

**4.** Because parsing depends on exactly two things: the file and the target
Python version. `PythonFile` is that pair, so two projects with the same version
share one parse. A `ProgramFile` key would needlessly prevent that sharing; a
`File` key would ignore the version, which changes the tree.

**5.** (1) You bypass the salsa cache and re-parse every request. (2) You
silently get Python **3.10**, because `parse_module` uses
`PythonVersion::default()`. (3) The semantic layer rejects nodes from a
differently-versioned parse — `AstNodeRef` is keyed on `(file, python_version)`,
with a test named `rejects_module_parsed_for_different_python_version`.

Hardest to diagnose is **(3)**, because the failure appears far from the cause —
in the type layer, on a node that looks perfectly valid.

**6.** The `parse_file(file_path, content)` RPC, where the editor sent unsaved
text with no `File`. Pass the version explicitly:
`ParseOptions::from(Mode::Module).with_target_version(version)` — never let
`Default` decide.

**7.** The parser produces a usable tree even when the source has syntax errors,
reporting the errors separately instead of giving up. It supports **quirk 13**:
failures are swallowed and a partial result is a valid result. Your driver must
never die on one weird file.

**8.** `.errors()` are genuine syntax errors — malformed code.
`.unsupported_syntax_errors()` are valid Python that is too new for the target
version (`type X = int` at 3.9).

Your driver logs both at debug level and keeps the partial tree. It reports
neither to the client — you are an analyser, not a linter. But the second kind is
your **version-misconfiguration alarm**, so discarding it silently is how a
version bug becomes invisible.

**9.** **Yes, usually** — the parser recovers, so most nodes survive. Find out by
walking the tree and counting what you got, not by checking whether errors were
reported. That distinction is the point: "errors exist" and "the tree is
unusable" are independent.

**10.** `range: TextRange` and `body: Vec<Stmt>`. The root of a Python file is
just a list of statements.
