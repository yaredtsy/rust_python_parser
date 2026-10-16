//! Scope scanning: the `ScopeScanner` visitor and the `scan_scope` driver.
//!
//! See `experience/02-parse-and-ast/05-object-visitor/04-wiring-it-up.md`.
//! RETIRED since: call collection. `ScopeScanner` used to also collect every
//! `ExprCall` in a scope (quirk 8's `ExprLambda => Skip`, `visit_expr`'s
//! `self.calls.push`). Superseded by
//! `experience/02-parse-and-ast/08-staged-pipeline.md`'s stage 3, which
//! resolves a call where `eval` reaches it rather than collecting it
//! unresolved first — so there is nothing left for a pre-pass to collect.
//! This file now only finds defs.

use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_body, walk_node};
use ruff_python_ast::{AnyNodeRef, Expr, Stmt};
use ruff_source_file::{LineIndex, SourceCode};
use ruff_text_size::{Ranged, TextRange, TextSize};
use rustc_hash::FxHashMap;

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

// ---------------------------------------------------------------- the visitor

pub struct ScopeScanner<'a> {
    root: TextRange,
    /// Definitions found in this scope — recursed into separately.
    pub defs: Vec<AnyNodeRef<'a>>,
}

impl<'a> ScopeScanner<'a> {
    pub fn new(root: TextRange) -> Self {
        Self { root, defs: Vec::new() }
    }
}

impl<'a> SourceOrderVisitor<'a> for ScopeScanner<'a> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        if node.range() == self.root {
            return TraversalSignal::Traverse; // this is me; go in
        }
        match node {
            AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::StmtClassDef(_) => {
                self.defs.push(node); // remember it, recurse later
                TraversalSignal::Skip // parser.py:78-80
            }
            _ => TraversalSignal::Traverse,
        }
    }

    fn visit_expr(&mut self, _expr: &'a Expr) {
        // Nothing inside an expression matters any more: no def can live in
        // one (object 5/8), and calls are no longer collected here — see the
        // RETIRED note above. Not calling `walk_expr` is what stops the
        // descent; `enter_node` is therefore never reached with an `Expr`
        // node at all (every `Expr` path, `visit_annotation` included, funnels
        // through this method — `ruff_python_ast::visitor::source_order`,
        // `walk_annotation`/`walk_expr`), so quirk 8's `ExprLambda => Skip`
        // and the empty `visit_decorator` (parity for call attribution) are
        // both dead with it, not merely unused.
    }
}

// ----------------------------------------------------------------- the driver

/// Scan ONE scope. Returns its direct children, nested scopes recursed into.
pub fn scan_scope<'a>(ctx: &Ctx<'_>, root: AnyNodeRef<'a>) -> Vec<Node> {
    let mut scanner = ScopeScanner::new(root.range());
    walk_node(&mut scanner, root); // ★ the NODE, not its body
    collect(ctx, scanner)
}

/// Scan a module: its body is not a node, so there is no root to enter.
pub fn scan_module<'a>(ctx: &Ctx<'_>, body: &'a [Stmt]) -> Vec<Node> {
    let mut scanner = ScopeScanner::new(TextRange::default());
    walk_body(&mut scanner, body);
    collect(ctx, scanner)
}

fn collect(ctx: &Ctx<'_>, scanner: ScopeScanner<'_>) -> Vec<Node> {
    // No sort: `self.defs` was filled by `enter_node`, which — being a
    // SOURCE ORDER visitor — already visits siblings in source order. Only
    // mattered as an interleave once calls joined the same list; see RETIRED.
    scanner
        .defs
        .into_iter()
        .map(|def| {
            let children = scan_scope(ctx, def); // ★★ the recursion
            node_for_definition(ctx, def, children)
        })
        .collect()
}

// ---------------------------------------------------------------- definitions

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

/// DECIDED: a decorated def's `position` **starts at the `@`**, not at the
/// `def`/`async`/`class` keyword — i.e. the decorators are part of the range.
/// Flip this const to go back to parso's convention; `header_range` below is
/// the other half of the switch and stays compiled either way.
///
/// This is a deliberate divergence from the Python driver (object 3's trap,
/// `experience/02-parse-and-ast/03-object-functiondef-classdef.md`): parso's
/// `Function.start_pos` is at `def`, with decorators in a `decorated` wrapper
/// *outside* the funcdef. So the M2 byte-identical gate
/// (`plan/04-build/02-milestones.md`) will differ on **every decorated def** —
/// which on real code is most methods. That is expected, not a regression.
const DECORATORS_IN_RANGE: bool = true;

/// The range a definition's `position` is computed from.
fn def_position_range(ctx: &Ctx<'_>, range: TextRange, def: AnyNodeRef<'_>) -> TextRange {
    if DECORATORS_IN_RANGE {
        range // ruff's own range: already starts at `@` (and at `async`)
    } else {
        header_range(ctx, range, def)
    }
}

/// Range with the decorators removed — parso's convention. `decorator_list` is a
/// field of the def, so `def.range` starts at `@`; parso starts at `def`/`async`.
///
/// Stopping at `last_decorator.end()` is not enough — that offset is the
/// newline after the decorator, so the position reports the decorator's line.
/// Skip forward over trivia (whitespace and `# comments`) to land on the real
/// `def`/`async`/`class` keyword.
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

/// The `ID:` value from the scope's docstring, if any (object 6).
///
/// `pub(crate)`: `defs::DefScanner` (object 8) reads the same docstring the
/// same way, on purpose — one reader shared by both walkers, so they cannot
/// disagree about which defs have an `ID:` and which do not.
pub(crate) fn id_from_body(body: &[Stmt]) -> Option<String> {
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
        // The decision recorded on `DECORATORS_IN_RANGE`: `position` starts at
        // the `@`, so a decorated def reports the DECORATOR's line, not `def`'s.
        assert!(DECORATORS_IN_RANGE, "this module's tests encode `true`");

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

    /// The other half of the switch still works, so flipping the const back for
    /// the M2 parity gate is a one-word change and not a rewrite.
    #[test]
    fn header_range_still_strips_decorators() {
        let src = "@cache  # trailing comment\n@wraps(f)\ndef one():\n    pass\n";
        let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY313);
        let parsed = parse_unchecked(src, options)
            .try_into_module()
            .expect("Mode::Module always produces a module");
        let index = LineIndex::from_source_text(src);
        let ctx = Ctx::new(src, &index);

        let def = parsed.syntax().body.first().expect("one statement");
        let node = AnyNodeRef::from(def);
        let stripped = ctx.position(header_range(&ctx, node.range(), node));
        assert_eq!((stripped.line, stripped.column), (3, 0), "lands on `def`");

        let full = ctx.position(node.range());
        assert_eq!((full.line, full.column), (1, 0), "ruff's range: the `@`");
    }
}
