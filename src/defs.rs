//! Stage 1 of the pipeline: defs only, found by a `StatementVisitor` instead
//! of object 5's `SourceOrderVisitor`. See
//! `experience/02-parse-and-ast/08-staged-pipeline.md`.
//!
//! One walker. `DefScanner` is the only thing that answers "which defs exist"
//! — stage 2 reads its `DefNode` output rather than re-deriving the answer
//! with a second walk, so the two stages cannot disagree about what a def is.

use ruff_db::files::File;
use ruff_db::parsed::parsed_module;
use ruff_python_ast::Stmt;
use ruff_python_ast::statement_visitor::{StatementVisitor, walk_body, walk_stmt};
use ruff_text_size::{Ranged, TextRange};
use ty_python_semantic::Db;

use crate::scan::id_from_body;

/// Owned, not `&'a Stmt` — the query's return value must outlive the
/// `ParsedModuleRef` it was built from (salsa may clear and re-parse).
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
pub enum DefKind {
    Function,
    Class,
}

/// Statements only — `def`/`class` is the only definition form that can hold
/// a docstring (a `lambda`'s body is an expression, so it never has one), so
/// a walk that never enters an expression still finds every ID-bearing def.
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
                walk_stmt(self, stmt); // if/for/while/with/try/match — defs can sit inside any of them
                return;
            }
        };

        // A FRESH scanner: not calling `walk_stmt` here is what stops this
        // walk at the def, the same way object 5's `TraversalSignal::Skip`
        // did — just from the opposite direction.
        let mut inner = DefScanner::default();
        walk_body(&mut inner, body);

        self.out.push(DefNode {
            id: id_from_body(body),
            name: name.as_str().to_string(),
            kind,
            range: stmt.range(),
            children: inner.out,
        });
    }
}

/// Scan one body for its direct and nested defs. Source order falls out of
/// `walk_stmt`'s own visiting order — nothing here sorts.
pub fn scan_defs(body: &[Stmt]) -> Vec<DefNode> {
    let mut scanner = DefScanner::default();
    walk_body(&mut scanner, body);
    scanner.out
}

#[cfg(test)]
static WALK_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Every def in `file`, salsa-cached per file.
#[salsa::tracked(returns(ref))]
pub fn definitions(db: &dyn Db, file: File) -> Vec<DefNode> {
    #[cfg(test)]
    WALK_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let python_file = db.program_file(file).python_file(db);
    let module = parsed_module(db, python_file).load(db);
    scan_defs(&module.syntax().body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{open, open_project};
    use crate::nodes::{Node, NodeKind};
    use crate::scan::{Ctx, scan_module};
    use ruff_db::system::SystemPath;
    use ruff_python_ast::PythonVersion;
    use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
    use ruff_source_file::LineIndex;
    use std::sync::atomic::Ordering;

    /// `(name, kind, start_line, start_col, end_line, end_col)` — comparable
    /// across `DefNode` and `Node`, at the same granularity object 5's own
    /// tests use.
    type Row = (String, &'static str, usize, usize, usize, usize);

    fn flatten_defs(ctx: &Ctx<'_>, defs: &[DefNode], out: &mut Vec<Row>) {
        for d in defs {
            let p = ctx.position(d.range);
            let kind = match d.kind {
                DefKind::Function => "function",
                DefKind::Class => "class",
            };
            out.push((d.name.clone(), kind, p.line, p.column, p.end_line, p.end_column));
            flatten_defs(ctx, &d.children, out);
        }
    }

    /// Same shape as `flatten_defs`, over `scan_module`'s wire-shape tree.
    fn flatten_nodes(nodes: &[Node], out: &mut Vec<Row>) {
        for n in nodes {
            let kind = match n.kind {
                NodeKind::Function => "function",
                NodeKind::Class => "class",
            };
            out.push((n.name.clone(), kind, n.position.line, n.position.column, n.position.end_line, n.position.end_column));
            flatten_nodes(&n.children, out);
        }
    }

    /// `scan_module` (`crate::scan`) is now a pure map over this module's
    /// `scan_defs` — no second walk — so this checks that the map is
    /// faithful: same names, same nesting, same ranges, on every fixture.
    fn assert_matches_scope_scanner(source: &str) {
        let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY313);
        let parsed = parse_unchecked(source, options)
            .try_into_module()
            .expect("Mode::Module always produces a module");
        let index = LineIndex::from_source_text(source);
        let ctx = Ctx::new(source, &index);

        let defs = scan_defs(&parsed.syntax().body);
        let nodes = scan_module(&ctx, &parsed.syntax().body);

        let mut from_defs = Vec::new();
        flatten_defs(&ctx, &defs, &mut from_defs);
        let mut from_nodes = Vec::new();
        flatten_nodes(&nodes, &mut from_nodes);

        assert_eq!(from_defs, from_nodes);
    }

    #[test]
    fn matches_scope_scanner_on_every_fixture() {
        for path in [
            "experience/02-parse-and-ast/python/calls.py",
            "experience/02-parse-and-ast/python/docstrings.py",
            "experience/02-parse-and-ast/python/edges.py",
            "experience/02-parse-and-ast/python/helpers.py",
            "experience/02-parse-and-ast/python/nested.py",
        ] {
            let source = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert_matches_scope_scanner(&source);
        }
    }

    /// Exercise E: a second `definitions()` call on an unchanged file must
    /// not re-walk — salsa should serve the cached value.
    #[test]
    fn definitions_does_not_rewalk_an_unchanged_file() {
        WALK_COUNT.store(0, Ordering::SeqCst);

        let dir = std::fs::canonicalize("experience/02-parse-and-ast").unwrap();
        let dir = SystemPath::from_std_path(&dir).unwrap();
        let db = open_project(dir).unwrap();

        let file_path = std::fs::canonicalize("experience/02-parse-and-ast/python/calls.py").unwrap();
        let file_path = SystemPath::from_std_path(&file_path).unwrap();
        let (file, _program_file) = open(&db, file_path).unwrap();

        let first = definitions(&db, file);
        assert_eq!(WALK_COUNT.load(Ordering::SeqCst), 1);

        let second = definitions(&db, file);
        assert_eq!(WALK_COUNT.load(Ordering::SeqCst), 1, "second call re-walked");
        assert_eq!(first, second);
    }
}
