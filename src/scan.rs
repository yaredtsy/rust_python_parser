//! Scope scanning: the `ScopeScanner` visitor and the `scan_scope` driver.
//!
//! See `experience/02-parse-and-ast/05-object-visitor/04-wiring-it-up.md`.

use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal, walk_body, walk_expr, walk_node,
};
use ruff_python_ast::{AnyNodeRef, Decorator, Expr, ExprCall, Stmt};
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

    fn position(&self, range: TextRange) -> Position {
        to_position_with(&self.code, range)
    }

    fn slice(&self, range: TextRange) -> &'src str {
        &self.code.text()[range]
    }
}

// ---------------------------------------------------------------- the visitor

pub struct ScopeScanner<'a> {
    root: TextRange,
    /// Definitions found in this scope — recursed into separately.
    pub defs: Vec<AnyNodeRef<'a>>,
    /// Every call found in this scope, in source order. One entry per
    /// `ExprCall` — no chains, so no dedup set and nothing to collapse.
    pub calls: Vec<&'a ExprCall>,
}

impl<'a> ScopeScanner<'a> {
    pub fn new(root: TextRange) -> Self {
        Self { root, defs: Vec::new(), calls: Vec::new() }
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
            AnyNodeRef::ExprLambda(_) => TraversalSignal::Skip, // quirk 8
            _ => TraversalSignal::Traverse,
        }
    }

    fn visit_decorator(&mut self, _d: &'a Decorator) {
        // PARITY: parso puts decorators outside the funcdef, so calls inside
        // them belong to the enclosing scope. Deliberately does NOT call
        // walk_decorator.
    }

    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            // Every call, once. No guard: the walk visits each `ExprCall`
            // exactly once, and one output node per `ExprCall` means there is
            // no second counting of chains to disagree with it.
            self.calls.push(call);
        }
        walk_expr(self, expr); // ★★ KEEP GOING
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
    let mut out = Vec::new();
    for def in scanner.defs {
        let children = scan_scope(ctx, def); // ★★ the recursion
        out.push(node_for_definition(ctx, def, children));
    }
    for call in scanner.calls {
        out.push(node_for_call(ctx, call));
    }
    // Calls nested in one expression share a start, so the END breaks the tie —
    // which puts `build`, `render`, `strip` in the order they actually run.
    out.sort_by_key(|n| {
        let p = n.position;
        (p.line, p.column, p.end_line, p.end_column)
    });
    out
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

// --------------------------------------------------------------------- calls

/// One node per `ExprCall`. No chain flattening, no `call_index`.
///
/// `call.range()` already covers the callee, so `build().render()` starts at
/// `build` — the span the old chain code reconstructed by hand is the range
/// ruff hands you.
fn node_for_call(ctx: &Ctx<'_>, call: &ExprCall) -> Node {
    Node {
        id: None, // calls never have one
        name: call_name(ctx, call),
        kind: NodeKind::Call,
        position: ctx.position(call.range()),
        children: Vec::new(),
        base_classes: None,
    }
}

/// The called name, from the AST rather than from a source slice.
///
/// Taking `a.attr` instead of slicing `a` means `calls.py:multiline_callee` —
/// a callee split over two lines with a backslash — is `render`, not the raw
/// text with the backslash and the indentation still in it. The receiver is not
/// part of the name; if something later needs "called on the result of what",
/// that is one `call.func` hop away, computed where it is read.
fn call_name(ctx: &Ctx<'_>, call: &ExprCall) -> String {
    match call.func.as_ref() {
        Expr::Name(n) => n.id.as_str().to_string(), // build()
        Expr::Attribute(a) => a.attr.as_str().to_string(), // .render()
        other => ctx.slice(other.range()).to_string(), // handlers["x"]()
    }
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
                if !matches!(n.kind, NodeKind::Call) {
                    out.push((n.name.clone(), n.position.line, n.position.column));
                }
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

    /// Every call node in the module, flattened, as
    /// `(name, start_line, start_col, end_line, end_col)`.
    fn calls(source: &str) -> Vec<(String, usize, usize, usize, usize)> {
        fn flatten(nodes: &[Node], out: &mut Vec<(String, usize, usize, usize, usize)>) {
            for n in nodes {
                if matches!(n.kind, NodeKind::Call) {
                    let p = n.position;
                    out.push((n.name.clone(), p.line, p.column, p.end_line, p.end_column));
                }
                flatten(&n.children, out);
            }
        }
        let mut out = Vec::new();
        flatten(&scan(source), &mut out);
        out
    }

    #[test]
    fn one_node_per_call_in_a_chain() {
        // `build().render().strip()` is three calls, so three nodes — no chain
        // expansion, no `call_index`. They share a start, and the END ordering
        // is the order they run in.
        let got = calls("build().render().strip()\n");
        assert_eq!(
            got,
            vec![
                ("build".to_string(), 1, 0, 1, 7),
                ("render".to_string(), 1, 0, 1, 16),
                ("strip".to_string(), 1, 0, 1, 24),
            ]
        );
    }

    #[test]
    fn subscript_between_calls_is_not_special() {
        // The old chain walk had to hop Attribute-over-Subscript to avoid
        // splitting this into two chains numbered from 0. One node per call
        // removes the question.
        let got = calls("build()[\"key\"].render()\n");
        assert_eq!(
            got,
            vec![
                ("build".to_string(), 1, 0, 1, 7),
                ("render".to_string(), 1, 0, 1, 23),
            ]
        );
    }

    #[test]
    fn no_call_is_emitted_twice() {
        // The `seen` set is gone; this is what used to need it.
        let src = "wrap(build(), key=build())\nbuild()()\n";
        let got = calls(src);
        assert_eq!(got.len(), 5, "{got:#?}");
        // and every node has a distinct span, which is what makes the range a
        // usable handle for the interpreter.
        let mut spans: Vec<_> = got.iter().map(|c| (c.1, c.2, c.3, c.4)).collect();
        spans.sort();
        let before = spans.len();
        spans.dedup();
        assert_eq!(spans.len(), before, "two call nodes share a span");
    }

    #[test]
    fn multiline_callee_name_comes_from_the_ast() {
        // Not the raw slice: `obj \\\n        . render (value=1)` would have put
        // a backslash and two lines of indentation in `name`.
        let got = calls("obj \\\n    . render (value=1)\n");
        assert_eq!(got.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["render"]);
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
