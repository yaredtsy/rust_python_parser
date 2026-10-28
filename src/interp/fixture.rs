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
        let entry = tree
            .lines()
            .next()
            .unwrap_or("")
            .split(" [")
            .next()
            .unwrap()
            .to_string();
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
        println!("--- {fixture}: \n{got} {}\n", e.tree);

        if got != e.tree {
            failures += &format!(
                "--- {fixture}: {}\nexpected:\n{}got:\n{}\n",
                e.entry, e.tree, got
            );
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

#[test]
fn p01_02_one_call() {
    check("01-functions/python/one_call.py");
}

#[test]
fn p01_03_locals() {
    check("01-functions/python/locals.py");
}

#[test]
fn p01_04_imports() {
    check("01-functions/python/imports.py");
    check("01-functions/python/imports_partial.py");
}

#[test]
fn p01_05_follow() {
    check("01-functions/python/follow.py");
}

#[test]
fn p01_06_recursion() {
    check("01-functions/python/recursion.py");
}

#[test]
fn p01_07_arguments() {
    check("01-functions/python/arguments.py");
}

#[test]
fn p01_08_varargs() {
    check("01-functions/python/varargs.py");
}

#[test]
fn p01_09_branches() {
    check("01-functions/python/branches.py");
}
