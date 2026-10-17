//! `parse_file`'s wire-shape mapping: `DefNode` (stage 1, `crate::defs`) →
//! `Node` (object 5/7's output shape).
//!
//! See `experience/02-parse-and-ast/05-object-visitor/04-wiring-it-up.md` and
//! `experience/02-parse-and-ast/08-staged-pipeline.md`.
//!
//! RETIRED since: `ScopeScanner`, the `SourceOrderVisitor`-based walker this
//! file used to define. It did two things — find defs, and collect every
//! `ExprCall` in a scope (quirk 8's `ExprLambda => Skip`, `visit_expr`'s
//! `self.calls.push`) — and both are gone:
//!
//! - Calls: superseded by stage 3, which resolves a call where `eval` reaches
//!   it rather than collecting it unresolved first. Nothing collects calls
//!   anywhere now.
//! - Defs: once calls were gone, this walker found exactly what
//!   `crate::defs::DefScanner` already finds — same rule, over the same
//!   `StatementVisitor`, producing `DefNode` instead of `Node`. Keeping a
//!   second, independent `SourceOrderVisitor` walk for the identical "which
//!   defs exist" question is exactly the two-walkers risk object 1 of
//!   exercise 10 warns about (one disagreement away from a def that is
//!   silently missing from one side). So this file no longer walks the AST
//!   at all — it maps `defs::scan_defs`'s output to the wire shape.
//! `ruff_python_ast::visitor::source_order::SourceOrderVisitor` is no longer
//! a dependency of this crate for that reason.

use ruff_source_file::{LineIndex, SourceCode};
use ruff_text_size::TextRange;
use rustc_hash::FxHashMap;

use crate::defs::{DefKind, DefNode, scan_defs};
use crate::nodes::{Node, NodeKind};
use crate::positions::{Position, to_position_with};

/// Everything the emit helpers need. Borrows the source; never owns it.
pub struct Ctx<'src> {
    pub code: SourceCode<'src, 'src>,
}

impl<'src> Ctx<'src> {
    pub fn new(source: &'src str, index: &'src LineIndex) -> Self {
        Self { code: SourceCode::new(source, index) }
    }

    pub(crate) fn position(&self, range: TextRange) -> Position {
        to_position_with(&self.code, range)
    }
}

// ----------------------------------------------------------------- the driver

/// A module's (or a scope's) defs, as the wire shape. `body` works for either
/// — `DefScanner` already recurses into nested defs' bodies on its own, so
/// there is nothing left here that needs "scope" and "module" to differ.
pub fn scan_module(ctx: &Ctx<'_>, body: &[ruff_python_ast::Stmt]) -> Vec<Node> {
    scan_defs(body).iter().map(|def| node_for_definition(ctx, def)).collect()
}

// ---------------------------------------------------------------- definitions

fn node_for_definition(ctx: &Ctx<'_>, def: &DefNode) -> Node {
    let kind = match def.kind {
        DefKind::Function => NodeKind::Function,
        DefKind::Class => NodeKind::Class,
    };

    Node {
        id: def.id.as_deref().map(|uuid| match kind {
            NodeKind::Class => format!("ClassSchema/{uuid}"),
            NodeKind::Function => format!("FunctionSchema/{uuid}"),
        }),
        name: def.name.clone(),
        kind,
        position: ctx.position(def.range),
        children: def.children.iter().map(|child| node_for_definition(ctx, child)).collect(),
        // exercise 08: needs semantic resolution, not the AST.
        base_classes: None,
    }
}

// A decorated def's `position` starts at the `@`, not at the
// `def`/`async`/`class` keyword — `DefNode.range` is `stmt.range()`, ruff's
// own range, decorators included.
//
// There used to be a `DECORATORS_IN_RANGE` const here plus `header_range` and
// `skip_trivia`, which stripped decorators back off for parso's convention
// (object 3's trap,
// `experience/02-parse-and-ast/03-object-functiondef-classdef.md`) — kept
// around for a byte-identical M2 parity gate against the Python driver.
// Deleted: that gate's other half, `call_index`/`call_col_pos`, is already
// gone (object 5 part 4), and once `node_for_definition` stopped taking an
// `AnyNodeRef` (this file's retirement of `ScopeScanner`), re-wiring the
// toggle needs a `covering_node(root, def.range)` lookup rather than a
// one-word flip — so the const no longer paid for its keep. Recover it from
// git history if the parity gate comes back.

/// The `ID:` value from the scope's docstring, if any (object 6).
///
/// `pub(crate)`: `defs::DefScanner` reads the same docstring the same way, on
/// purpose — one reader shared, so there is nothing to disagree about which
/// defs have an `ID:` and which do not.
pub(crate) fn id_from_body(body: &[ruff_python_ast::Stmt]) -> Option<String> {
    let doc = docstring(body)?;
    extract_metadata(doc).get("ID").map(|s| s.to_string())
}

fn docstring(body: &[ruff_python_ast::Stmt]) -> Option<&str> {
    match body.first()? {
        ruff_python_ast::Stmt::Expr(e) => Some(e.value.as_string_literal_expr()?.value.to_str()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ruff_python_ast::PythonVersion;
    use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};

    /// Scan a module and return `(name, start_line, start_column)` for every
    /// definition node, outermost first.
    fn defs(source: &str) -> Vec<(String, usize, usize)> {
        fn flatten(nodes: &[Node], out: &mut Vec<(String, usize, usize)>) {
            for n in nodes {
                out.push((n.name.clone(), n.position.line, n.position.column));
                flatten(&n.children, out);
            }
        }
        let mut out = Vec::new();
        flatten(&scan(source), &mut out);
        out
    }

    /// Parse and scan a module, source in, nodes out.
    fn scan(source: &str) -> Vec<Node> {
        let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY313);
        let parsed = parse_unchecked(source, options)
            .try_into_module()
            .expect("Mode::Module always produces a module");
        let index = LineIndex::from_source_text(source);
        let ctx = Ctx::new(source, &index);
        scan_module(&ctx, &parsed.syntax().body)
    }

    #[test]
    fn decorators_are_inside_the_range() {
        // `DefNode.range` is `stmt.range()`: a decorated def reports the
        // DECORATOR's line, not `def`'s.
        let src = "\
@cache
def one():
    pass


@cache
@wraps(f)
async def two():
    pass


class Plain:
    @property
    def prop(self):
        pass
";
        assert_eq!(
            defs(src),
            vec![
                ("one".to_string(), 1, 0),   // `@cache`, not `def` on line 2
                ("two".to_string(), 6, 0),   // the FIRST of two decorators
                ("Plain".to_string(), 12, 0), // undecorated: unchanged
                ("prop".to_string(), 13, 4),  // `@property`, not `def` on line 14
            ]
        );
    }

    #[test]
    fn undecorated_defs_are_untouched() {
        let src = "def f():\n    pass\n\n\nasync def g():\n    pass\n";
        assert_eq!(
            defs(src),
            vec![("f".to_string(), 1, 0), ("g".to_string(), 5, 0)]
        );
    }
}
