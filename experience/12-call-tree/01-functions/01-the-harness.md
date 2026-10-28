# 01.01 — The harness

**You build:** `src/interp/` with a tree type, a text form of it, and a test
that reads expected trees out of a fixture and compares. The analysis itself
returns only the entry's node.
**Fixture:** `python/harness.py`
**Plan:** `plan/04-build/00-dev-cli.md` (the inner loop), `plan/03-call-tree/15`
§ "Expected-edge fixtures"

---

## Why a harness first

Every later step is "make this fixture's tree come out". If checking a tree
means reading JSON by eye, you will skip it. If it is one `cargo test` that
prints the expected and actual trees side by side, you will run it after every
edit. Build that loop before there is anything to check.

## The fixture

```python
# python/harness.py
def main():
    pass

def other():
    x = 1

# expect:
# main
```

## Predict

1. `main` calls nothing. What is the smallest tree the analysis can return for
   it?
2. Which module name must be dropped from `harness.main` so the test can say
   `main`?

---

## Build

### Decision 1: the expected tree lives in the fixture, as text

```
# expect:
# main
#   helper
#     leaf [recursive]
```

**Why in the fixture:** when a test fails, the Python and the tree it should
produce are on one screen. A separate golden file splits them, and they drift.
**Why text:** a failing `assert_eq!` on two structs prints a wall of `Node {
qname: …, children: [Node { … }] }`. Two indented trees can be compared at a
glance.

Golden JSON files do have a place: the Jedi parity suite in part 05, where the
other side is a program, not you.

### Decision 2: one door into ty

Every call into ty goes through one file, `src/sem.rs`. The interpreter never
imports `ty_python_semantic` or `ty_python_core` itself.

**Why:** ty's API moves on every rebase, and several accessors you need are
reached through the `vnoc` wrappers. When one breaks, you fix `sem.rs` and
nothing else. The plan's module list starts with this rule
(`plan/03-call-tree/03-design.md` § "Modules").

```rust
// src/sem.rs
pub use ruff_python_ast::name::Name;
pub use ty_python_core::definition::{Definition, DefinitionKind};
pub use ty_python_semantic::types::Type;
pub use ty_python_semantic::SemanticModel;

/// The only door into ty. Everything else in pylspt's interpreter asks this.
pub struct Sem<'db> {
    pub db: &'db ProjectDatabase,
}
```

Two methods in this step:

- `entry(file, "main" | "Pipeline.run") -> Option<Definition>`: find the `def`
  in the module's AST, then ask ty for its `Definition` (`HasDefinition` on
  `StmtFunctionDef`).
- `qname(def) -> String`: the existing `crate::qname::qualified_name`.

**Why `Definition<'db>` as a function's identity:** it is public, a 4-byte
salsa id, and everything later starts from it: the qualified name, the body,
the docstring `ID:`, whether it is project code.

### Decision 3: the tree type is the output, not the walker's state

```rust
// src/interp/tree.rs
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub qname: String,
    pub site: Option<TextRange>,   // the call expression's range. None for the root
    pub marks: Vec<Mark>,
    pub children: Vec<Node>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark { Recursive, Cut, Incomplete }
```

**Why `site` now:** one node per call site and function is the rule the whole
output follows (step 05). Without the site in the node you can't tell two
calls to `emit` from one call that reached it twice.

`to_text(&node, module)` prints one line per node, two spaces per level,
marks in brackets, with `module.` dropped from the front of each name.

### Decision 4: report every mismatch, not the first

`check(fixture)` runs every `# expect:` block in the file, collects every
mismatch, and fails once with all of them. One run tells you everything that
is wrong.

### Decision 5: the project root is the fixture's folder

The test opens the database with the fixture's `python/` folder as the project
root. So `util.py` beside a fixture is project code, and `print` (typeshed) is
not. That is the same split the real server makes, with no special case for
tests.

### Yours

- `pub mod interp;` in `lib.rs`.
- `interp/mod.rs`: `Mode` (only `Inline` for now), `Options`,
  `call_tree(db, file, entry, &opts) -> Option<Node>` returning just the root.
- `interp/tree.rs`: `Node`, `Mark`, `to_text`.
- `interp/fixture.rs` (`#[cfg(test)]`): `expectations(source)` and
  `check(fixture)`.
- `sem.rs` with `entry` and `qname`.
- Optional: `src/bin/calls.rs`, printing `to_text` for `<dir> <file> <entry>`,
  for poking at files outside the tests.

<details><summary>Reference: this step's code</summary>

```rust
// src/sem.rs
use ruff_db::files::File;
use ruff_db::parsed::parsed_module;
use ruff_python_ast::Stmt;
use ty_project::ProjectDatabase;
use ty_python_semantic::{Db as _, HasDefinition};

pub use ruff_python_ast::name::Name;
pub use ty_python_core::definition::{Definition, DefinitionKind};
pub use ty_python_semantic::SemanticModel;
pub use ty_python_semantic::types::Type;

pub struct Sem<'db> {
    pub db: &'db ProjectDatabase,
}

impl<'db> Sem<'db> {
    pub fn new(db: &'db ProjectDatabase) -> Self {
        Sem { db }
    }

    /// `"main"` or `"Pipeline.run"`: a function at module level or in a class.
    pub fn entry(&self, file: File, dotted: &str) -> Option<Definition<'db>> {
        let program_file = self.db.program_file(file);
        let parsed = parsed_module(self.db, program_file.python_file(self.db)).load(self.db);
        let model = SemanticModel::new(self.db, program_file);

        let mut body: &[Stmt] = &parsed.syntax().body;
        let mut parts = dotted.split('.').peekable();
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            let mut next = None;
            for stmt in body {
                match stmt {
                    Stmt::FunctionDef(f) if last && f.name.as_str() == part => {
                        return Some(f.definition(&model));
                    }
                    Stmt::ClassDef(c) if !last && c.name.as_str() == part => next = Some(&c.body[..]),
                    _ => {}
                }
            }
            body = next?;
        }
        None
    }

    pub fn qname(&self, def: Definition<'db>) -> String {
        let parsed = parsed_module(self.db, def.python_file(self.db)).load(self.db);
        crate::qname::qualified_name(self.db, def, &parsed).unwrap_or_else(|| "<unknown>".into())
    }
}
```

```rust
// src/interp/mod.rs
//! The call tree analysis. Built step by step in experience/12-call-tree.

mod tree;
#[cfg(test)]
mod fixture;

pub use tree::{Mark, Node, to_text};

use ruff_db::files::File;
use ty_project::ProjectDatabase;

use crate::sem::Sem;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Follow every call into the callee's body (parts 01 and 03).
    Inline,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub mode: Mode,
}

impl Default for Options {
    fn default() -> Self {
        Options { mode: Mode::Inline }
    }
}

/// The call tree under `entry`, a function in `file`. `None` when there is no
/// such function.
pub fn call_tree(db: &ProjectDatabase, file: File, entry: &str, _opts: &Options) -> Option<Node> {
    let sem = Sem::new(db);
    let def = sem.entry(file, entry)?;
    Some(Node::root(sem.qname(def)))
}
```

```rust
// src/interp/tree.rs
use ruff_text_size::TextRange;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub qname: String,
    pub site: Option<TextRange>,
    pub marks: Vec<Mark>,
    pub children: Vec<Node>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Recursive,
    Cut,
    Incomplete,
}

impl Node {
    pub fn root(qname: String) -> Self {
        Node { qname, site: None, marks: Vec::new(), children: Vec::new() }
    }
}

/// One line per node, two spaces per level, `module.` dropped from names.
pub fn to_text(root: &Node, module: &str) -> String {
    fn go(n: &Node, depth: usize, module: &str, out: &mut String) {
        let short = n
            .qname
            .strip_prefix(module)
            .and_then(|rest| rest.strip_prefix('.'))
            .unwrap_or(&n.qname);
        out.push_str(&"  ".repeat(depth));
        out.push_str(short);
        for m in &n.marks {
            out.push_str(match m {
                Mark::Recursive => " [recursive]",
                Mark::Cut => " [cut]",
                Mark::Incomplete => " [incomplete]",
            });
        }
        out.push('\n');
        for c in &n.children {
            go(c, depth + 1, module, out);
        }
    }
    let mut out = String::new();
    go(root, 0, module, &mut out);
    out
}
```

```rust
// src/interp/fixture.rs
use ruff_db::system::SystemPath;

use crate::db::{open, open_project};
use crate::interp::{Options, call_tree, to_text};

pub struct Expect {
    pub entry: String,
    pub tree: String,
}

/// Every `# expect:` block in `source`.
pub fn expectations(source: &str) -> Vec<Expect> {
    let mut out = Vec::new();
    let mut lines = source.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim_end() != "# expect:" {
            continue;
        }
        let mut tree = String::new();
        while let Some(rest) = lines.peek().and_then(|l| l.strip_prefix("# ")) {
            if rest.starts_with("expect") {
                break; // the next block
            }
            tree.push_str(rest);
            tree.push('\n');
            lines.next();
        }
        let entry = tree.lines().next().unwrap_or("").split(" [").next().unwrap().to_string();
        out.push(Expect { entry, tree });
    }
    out
}

/// Run every block in `fixture` (relative to experience/12-call-tree).
pub fn check(fixture: &str) {
    let path = std::fs::canonicalize(format!("experience/12-call-tree/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let source = std::fs::read_to_string(&path).unwrap();
    let module = path.file_stem().unwrap().to_str().unwrap();

    let blocks = expectations(&source);
    assert!(!blocks.is_empty(), "{fixture}: no `# expect:` block");
    let mut failures = String::new();
    for e in blocks {
        let got = call_tree(&db, file, &e.entry, &Options::default())
            .map(|t| to_text(&t, module))
            .unwrap_or_else(|| format!("(no function `{}`)\n", e.entry));
        if got != e.tree {
            failures += &format!("--- {fixture}: {}\nexpected:\n{}got:\n{}\n", e.entry, e.tree, got);
        }
    }
    assert!(failures.is_empty(), "\n{failures}");
}

#[test]
fn reads_blocks() {
    let src = "def f(): pass\n# expect:\n# main\n#   a [recursive]\n# expect:\n# other\n";
    let b = expectations(src);
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].entry, "main");
    assert_eq!(b[0].tree, "main\n  a [recursive]\n");
    assert_eq!(b[1].entry, "other");
}

#[test]
fn p01_01_harness() {
    check("01-functions/python/harness.py");
}
```

</details>

---

## Run

```bash
cargo test interp::fixture
```

Both tests pass. Now break one on purpose: add `#   helper` under `# main`
in `harness.py` and run again.

```
--- 01-functions/python/harness.py: main
expected:
main
  helper
got:
main
```

That is the message you will read most in this tutorial. Put the line back.

---

## Where you are

```
call_tree(db, file, "main") → Node { qname: "harness.main", children: [] }
```

No walking yet. The next step walks `main`'s body.

---

## Exercise

1. Why does `check` use the fixture's folder as the project root, rather than
   the repository root?
2. `to_text` drops `harness.` from `harness.main`. What would it print for a
   function `helper` defined in `util.py` and called from `harness.py`? Why is
   that the right output?
3. Why is `site` a range and not just the start offset? (Hint: `f()()`.)

<details><summary>Answers</summary>

1. With the repository root, every Python file in the repo would be project
   code, including other exercises' fixtures. The fixture folder makes "project
   code" mean exactly the files the fixture can import, the same split the
   server makes for a real project.
2. `util.helper`. Only the fixture's own module is dropped. A function from
   another module keeps its module, so the tree shows where it came from.
3. `f()()` has two calls that start at the same offset: `f()` and the call of
   its result. Only the end offset tells them apart. The plan makes the same
   choice for `SiteId` (`plan/03-call-tree/09-summaries.md`).

</details>

---

→ Next: [`02-one-call.md`](02-one-call.md)
