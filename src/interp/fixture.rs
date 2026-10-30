// src/interp/fixture.rs
use crate::db::{open, open_project};
use crate::interp::bind::param_names;
use crate::interp::relevant::relevant_params;
use crate::interp::summary::{PlainVal, SVal};
use crate::interp::{Mode, Options, Stats, call_tree, solve, to_text};
use crate::sem::Sem;
use ruff_db::system::SystemPath;
use std::collections::BTreeSet;

pub struct Expect {
    pub entry: String,
    pub tree: String,
    pub mode: Option<Mode>, // None: `# expect:`, every mode
}
/// Every `# expect:` block in `source`.
pub fn expectations(source: &str) -> Vec<Expect> {
    let mut out = Vec::new();
    let mut lines = source.lines().peekable();
    while let Some(line) = lines.next() {
        let mode = match line.trim_end() {
            "# expect:" => None,
            "# expect inline:" => Some(Mode::Inline),
            "# expect summary:" => Some(Mode::Summary),
            _ => continue,
        };
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
        out.push(Expect { entry, tree, mode });
    }
    out
}

/// The functions `func` can return (qualified names), over all its contexts,
/// solved in summary mode from `entry`.
pub fn returns_of(fixture: &str, entry: &str, func: &str) -> BTreeSet<String> {
    let path = std::fs::canonicalize(format!("experience/12-call-tree/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let module = path.file_stem().unwrap().to_str().unwrap();

    let (s, _) = solve(&db, file, entry, &summary()).unwrap();
    let want = format!("{module}.{func}");
    let mut out = BTreeSet::new();
    for (ctx, c) in s.contexts.iter() {
        if s.sem.qname(c.func) != want {
            continue;
        }
        let Some(res) = &s.slots[ctx].result else {
            continue;
        };
        for v in &res.returns {
            if let SVal::Plain(PlainVal::Function(d)) = v {
                out.insert(s.sem.qname(*d));
            }
        }
    }
    out
}

pub fn relevant_names(fixture: &str, func: &str) -> Vec<String> {
    let path = std::fs::canonicalize(format!("experience/12-call-tree/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let sem = Sem::new(&db);
    let def = sem.entry(file, func).unwrap();
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed).unwrap();
    param_names(node)
        .zip(relevant_params(&db, def))
        .filter(|(_, r)| **r)
        .map(|(n, _)| n.to_string())
        .collect()
}
/// The stats of one entry in `fixture`, opened the way `check` does.
pub fn stats_for(fixture: &str, entry: &str, opts: &Options) -> Stats {
    let path = std::fs::canonicalize(format!("experience/12-call-tree/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    call_tree(&db, file, entry, opts).unwrap().1
}

pub fn inline() -> Options {
    Options {
        mode: Mode::Inline,
        ..Options::default()
    }
}
pub fn summary() -> Options {
    Options {
        mode: Mode::Summary,
        ..Options::default()
    }
}

/// Inline mode only: for a fixture summary mode can't run yet.
pub fn check(fixture: &str) {
    check_with(fixture, &[inline()]);
}

/// Every mode. The same expected trees must come out of each.
pub fn check_both(fixture: &str) {
    check_with(
        fixture,
        &[
            inline(),
            summary(),
            Options {
                reuse: false,
                ..summary()
            },
        ],
    );
}

/// One run per `Options`. Every failing block of every run goes in one report.
fn check_with(fixture: &str, runs: &[Options]) {
    let report: String = runs.iter().map(|opts| failures(fixture, opts)).collect();
    assert!(report.is_empty(), "\n{report}");
}

fn failures(fixture: &str, opts: &Options) -> String {
    let path = std::fs::canonicalize(format!("experience/12-call-tree/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let source = std::fs::read_to_string(&path).unwrap();
    let module = path.file_stem().unwrap().to_str().unwrap();

    let blocks = expectations(&source);
    assert!(!blocks.is_empty(), "{fixture}: no `# expect:` block");
    let mut report = String::new();
    for e in blocks
        .iter()
        .filter(|e| e.mode.is_none_or(|m| m == opts.mode))
    {
        let got = call_tree(&db, file, &e.entry, opts)
            .map(|(t, _)| to_text(&t, module))
            .unwrap_or_else(|| format!("(no function `{}`)\n", e.entry));
        println!("--- {fixture}: {} ({:?})\n{got}", e.entry, opts.mode); // --show-output
        if got != e.tree {
            report += &format!(
                "--- {fixture}: {} {opts:?}\nexpected:\n{}got:\n{}\n",
                e.entry, e.tree, got
            );
        }
    }
    report
}

#[test]
fn reads_blocks() {
    let src = "def f(): pass\n# expect:\n# main\n#   a [recursive]\n# expect:\n# other\n";
    let b = expectations(src);
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].entry, "main");
    assert_eq!(b[0].tree, "main\n  a [recursive]\n");
    assert_eq!(b[1].entry, "other");

    let b = expectations("# expect inline:\n# f\n#   f [recursive]\n# expect summary:\n# f\n");
    assert_eq!(
        (b[0].mode, b[1].mode),
        (Some(Mode::Inline), Some(Mode::Summary))
    );
    assert_eq!(b[1].entry, "f");
}

#[test]
fn p01_01_harness() {
    check_both("01-functions/python/harness.py");
}

#[test]
fn p01_02_one_call() {
    check_both("01-functions/python/one_call.py");
}

#[test]
fn p01_03_locals() {
    check_both("01-functions/python/locals.py");
}

#[test]
fn p01_04_imports() {
    check_both("01-functions/python/imports.py");
    check_both("01-functions/python/imports_partial.py");
}

#[test]
fn p01_05_follow() {
    check_both("01-functions/python/follow.py");
}

#[test]
fn p01_06_recursion() {
    check_both("01-functions/python/recursion.py");
}

#[test]
fn p01_07_arguments() {
    check_both("01-functions/python/arguments.py");
}

#[test]
fn p01_08_varargs() {
    check_both("01-functions/python/varargs.py");
}

#[test]
fn p01_09_branches() {
    check_both("01-functions/python/branches.py");
}

#[test]
fn p01_10_returns() {
    check_both("01-functions/python/returns.py");
}

#[test]
fn p01_11_closures() {
    check("01-functions/python/closures.py");
}

#[test]
fn p01_12_callbacks() {
    check_both("01-functions/python/callbacks.py");
}
#[test]
fn p01_13_wall() {
    check_both("01-functions/python/wall.py");
    let stats = stats_for("01-functions/python/wall.py", "main", &Options::default());
    assert_eq!(stats.walks, 15);
    assert_eq!(stats.nodes, 14);
    assert_eq!(stats.contexts, 7);
}

#[test]
fn p02_01_keys() {
    check("02-function-summaries/python/keys.py");
    let stats = stats_for(
        "02-function-summaries/python/keys.py",
        "main",
        &Options::default(),
    );
    assert_eq!(stats.walks, 19);
    assert_eq!(stats.contexts, 11); // 12 before step 02.04: `note`'s `msg` is irrelevant
}

#[test]
fn p02_02_summaries() {
    check_both("02-function-summaries/python/summaries.py");
}

#[test]
fn p02_03_solver() {
    check_both("02-function-summaries/python/graph.py");
    let st = stats_for("02-function-summaries/python/graph.py", "main", &summary());
    assert_eq!((st.walks, st.reuse_hits), (8, 2));

    let st = stats_for("02-function-summaries/python/keys.py", "main", &summary());
    assert_eq!((st.walks, st.reuse_hits), (11, 6)); // (12, 5) before step 02.04

    let st = stats_for("01-functions/python/wall.py", "main", &summary());
    assert_eq!(st.walks, 7);
}

#[test]
fn p02_04_relevant() {
    check_both("02-function-summaries/python/relevant.py");
    let st = stats_for(
        "02-function-summaries/python/relevant.py",
        "main",
        &summary(),
    );
    assert_eq!(st.contexts, 9);
}

#[test]
fn p02_04_relevance_cases() {
    let fixture = "02-function-summaries/python/relevance_cases.py";
    let cases: &[(&str, &[&str])] = &[
        ("cond_only", &["b"]),
        ("bare_condition", &[]),
        ("attr_in_condition", &["cb"]),
        ("loop_order", &["cb"]),
        ("aug", &["cb"]),
        ("attr_store", &["cb", "obj"]),
        ("matched", &["cb"]),
        ("nested_class", &["cb"]),
        ("global_write", &["cb"]),
        ("splats", &["args"]),
    ];
    let mut wrong = String::new();
    for (func, want) in cases {
        let got = relevant_names(fixture, func);
        if got != *want {
            wrong += &format!("{func}: expected {want:?}, got {got:?}\n");
        }
    }
    assert!(wrong.is_empty(), "\n{wrong}");
}

#[test]
fn p02_05_pending() {
    check_both("02-function-summaries/python/pending.py");
}

#[test]
fn p02_06_groups() {
    check_both("02-function-summaries/python/groups.py");
}

#[test]
fn p02_06_entry_independence() {
    // The same contexts, solved from two entries: the group's members are
    // walked in a different order, and must settle on the same returns.
    let f = "02-function-summaries/python/groups.py";
    let from_expr = returns_of(f, "parse_expr", "parse_expr");
    assert_eq!(
        from_expr,
        BTreeSet::from(["groups.binop", "groups.num"].map(String::from))
    );
    for func in ["parse_expr", "parse_atom", "combine"] {
        assert_eq!(
            returns_of(f, "parse_expr", func),
            returns_of(f, "parse_atom", func),
            "{func}"
        );
    }
}

#[test]
fn p02_06_capped_group_is_incomplete() {
    let opts = Options {
        max_rounds: 1,
        ..summary()
    };
    let st = stats_for("02-function-summaries/python/groups.py", "ping_entry", &opts);
    assert_eq!(st.rounds_capped, 1);
    let st = stats_for("02-function-summaries/python/groups.py", "ping_entry", &summary());
    assert_eq!(st.rounds_capped, 0);
}
