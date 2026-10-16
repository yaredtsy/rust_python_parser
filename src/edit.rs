//! Stage 2's pure half: where an `ID:` goes (`DocSlot`), and the edit that
//! puts it there (`Edit`, `plan_ids`, `apply`). See
//! `experience/10-lossless-edits/02-object-slot.md` and `03-object-edit.md`.
//!
//! One walker, still. `plan_ids` does not re-derive "which defs exist" — that
//! was already decided by `DefScanner` (`crate::defs`, object 2's stage 1).
//! It takes stage 1's `DefNode`s as given and, for each one, does a single
//! range-to-node lookup (`covering_node`) to get back the `&[Stmt]` body a
//! slot is computed from. That lookup is not a second decision about what
//! counts as a def — `DefNode.range` already says exactly which statement
//! this is — so there is still exactly one rule for which defs exist.
//!
//! `experience/10-lossless-edits/01-object-def.md`'s `child_defs`/`Def` is
//! the design this superseded: it existed to give collection and injection a
//! second, *shared* walk so they could not disagree. `08-staged-pipeline.md`
//! closed that gap a different way — stage 2 reads stage 1's output instead
//! of walking the AST on its own — which makes `child_defs` unnecessary. Do
//! not resurrect it alongside `DefScanner`; that would be two walkers again.

use ruff_python_ast::find_node::covering_node;
use ruff_python_ast::{AnyNodeRef, Stmt, StringFlags};
use ruff_text_size::{Ranged, TextSize};
use ty_python_core::definition::docstring_from_body;

use crate::defs::DefNode;

/// Where to insert, and what shape to insert.
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
    /// A docstring exists: `at` is immediately before its closing quotes.
    InLiteral { triple_quoted: bool },
    /// None exists: a whole new docstring statement goes in at `at`.
    NewStatement,
}

/// `None` on a body a write must not touch: empty (a syntax error left
/// `parse_body` with `Suite::new()`) or an unclosed docstring. Both are
/// quirk 13 — swallow it, do not panic, and never rewrite a file that does
/// not parse.
pub fn doc_slot(body: &[Stmt]) -> Option<DocSlot> {
    let stmt_start = body.first()?.start();
    match docstring_from_body(body) {
        Some(doc) => {
            // The LAST part holds the closing quotes: `"a" "b"` is one literal.
            let last = doc.value.as_slice().last()?;
            let flags = last.flags;
            if flags.is_unclosed() {
                return None;
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

/// One insertion. Owned, so it outlives the AST it was derived from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub offset: TextSize,
    pub text: String,
}

/// The whitespace between `at`'s line start and `at` — the indentation to
/// reuse verbatim, tabs and all. `None` when `at` is not the first thing on
/// its line (a one-liner body, or `at` mid-docstring-line).
fn line_prefix(source: &str, at: TextSize) -> Option<&str> {
    let at = at.to_usize();
    let line_start = source[..at].rfind('\n').map_or(0, |nl| nl + 1);
    let prefix = &source[line_start..at];
    prefix.bytes().all(|b| b == b' ' || b == b'\t').then_some(prefix)
}

/// A one-liner has no `body[0]` indentation to copy, so synthesise one: the
/// `def`/`class` statement's own indentation, one unit deeper.
fn one_liner_indent(source: &str, def_start: TextSize) -> String {
    let own = line_prefix(source, def_start).unwrap_or("");
    format!("{own}    ")
}

/// Object 2's slot, plus the text to put in it.
fn id_edit(
    def: &DefNode,
    body: &[Stmt],
    source: &str,
    nl: &str,
    new_id: &mut impl FnMut() -> String,
) -> Option<Edit> {
    let slot = doc_slot(body)?;
    let uuid = new_id();
    let indent = line_prefix(source, slot.stmt_start);

    let (offset, text) = match (slot.kind, indent) {
        // a docstring exists: extend it in place
        (SlotKind::InLiteral { triple_quoted: true }, Some(ind)) => {
            match line_prefix(source, slot.at) {
                // the closer starts its own line: insert BEFORE its indent,
                // or that indent is left stranded on a line of its own
                Some(before) => {
                    (slot.at - TextSize::of(before), format!("{nl}{ind}ID: {uuid}{nl}"))
                }
                // the closer follows text, as in `"""Summary."""`
                None => (slot.at, format!("{nl}{nl}{ind}ID: {uuid}{nl}{ind}")),
            }
        }
        (SlotKind::InLiteral { triple_quoted: false }, _) => {
            (slot.at, format!(" ID: {uuid}")) // a newline would not parse
        }
        (SlotKind::InLiteral { triple_quoted: true }, None) => {
            (slot.at, format!(" ID: {uuid}")) // `def f(): """doc"""`
        }

        // none exists: a whole new statement before body[0]
        (SlotKind::NewStatement, Some(ind)) => {
            (slot.at, format!("\"\"\"ID: {uuid}\"\"\"{nl}{ind}"))
        }
        (SlotKind::NewStatement, None) => {
            let ind = one_liner_indent(source, def.range.start());
            (slot.at, format!("{nl}{ind}\"\"\"ID: {uuid}\"\"\"{nl}{ind}"))
        }
    };

    Some(Edit { offset, text })
}

/// `DefNode.range` already tells us exactly which statement this is: one AST
/// lookup by range, not a second "which defs exist" walk. `covering_node`
/// does not decide what counts as a def; `DefScanner` already did.
fn def_body<'a>(root: AnyNodeRef<'a>, def: &DefNode) -> Option<&'a [Stmt]> {
    match covering_node(root, def.range).node() {
        AnyNodeRef::StmtFunctionDef(f) => Some(&f.body),
        AnyNodeRef::StmtClassDef(c) => Some(&c.body),
        _ => None,
    }
}

/// Plan an `ID:` insert for every def below `defs` that has no key yet.
///
/// `defs` is stage 1's own output (`crate::defs::DefNode`), not a fresh walk
/// — `def.id` is already `id_from_body`'s answer, read once, by `DefScanner`.
/// `new_id` is a parameter so tests can pass a counter. Nothing in here
/// touches the filesystem or the database.
pub fn plan_ids(
    defs: &[DefNode],
    root: AnyNodeRef<'_>,
    source: &str,
    nl: &str,
    new_id: &mut impl FnMut() -> String,
) -> Vec<Edit> {
    let mut edits = Vec::new();
    for def in defs {
        if def.id.is_none() {
            if let Some(body) = def_body(root, def) {
                if let Some(edit) = id_edit(def, body, source, nl, new_id) {
                    edits.push(edit);
                }
            }
        }
        edits.extend(plan_ids(&def.children, root, source, nl, new_id));
    }
    edits
}

/// Bottom-up: an insert shifts every byte after it, so applying from the end
/// of the file backwards keeps every offset not yet reached valid.
pub fn apply(source: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.offset));
    debug_assert!(
        edits.windows(2).all(|w| w[0].offset != w[1].offset),
        "two edits at one offset: DefScanner produced a def twice",
    );

    let mut out = source.to_string();
    for e in edits {
        out.insert_str(e.offset.to_usize(), &e.text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruff_python_ast::PythonVersion;
    use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
    use ruff_source_file::find_newline;

    use crate::defs::scan_defs;

    /// Plan with a counter, apply, and return the result — reproducible,
    /// unlike a real UUID generator (object 3, "`new_id` is a parameter").
    fn inject(source: &str) -> String {
        let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY313);
        let parsed = parse_unchecked(source, options)
            .try_into_module()
            .expect("Mode::Module always produces a module");
        let root = AnyNodeRef::from(parsed.syntax());
        let defs = scan_defs(&parsed.syntax().body);
        let nl = find_newline(source).map_or("\n", |(_, ending)| ending.as_str());

        let mut n = 0;
        let edits = plan_ids(&defs, root, source, nl, &mut || {
            n += 1;
            format!("id-{n}")
        });
        apply(source, edits)
    }

    #[test]
    fn new_statement_before_a_one_liner_body() {
        let out = inject("def stub():\n    ...\n");
        assert_eq!(out, "def stub():\n    \"\"\"ID: id-1\"\"\"\n    ...\n");
    }

    #[test]
    fn new_statement_before_a_normal_body() {
        let out = inject("def f():\n    return 1\n");
        assert_eq!(out, "def f():\n    \"\"\"ID: id-1\"\"\"\n    return 1\n");
    }

    #[test]
    fn extends_a_single_line_triple_quoted_docstring() {
        let out = inject("def f():\n    \"\"\"Summary.\"\"\"\n    return 1\n");
        assert_eq!(out, "def f():\n    \"\"\"Summary.\n\n    ID: id-1\n    \"\"\"\n    return 1\n");
    }

    #[test]
    fn extends_a_multi_line_docstring_whose_closer_owns_its_line() {
        let out = inject("def f():\n    \"\"\"Summary.\n\n    Details.\n    \"\"\"\n    return 1\n");
        assert_eq!(
            out,
            "def f():\n    \"\"\"Summary.\n\n    Details.\n\n    ID: id-1\n    \"\"\"\n    return 1\n"
        );
    }

    #[test]
    fn extends_a_single_quoted_docstring_on_one_line() {
        let out = inject("def f():\n    'Summary.'\n    return 1\n");
        assert_eq!(out, "def f():\n    'Summary. ID: id-1'\n    return 1\n");
    }

    #[test]
    fn already_keyed_def_gets_no_edit() {
        let out = inject("def f():\n    \"\"\"ID: existing\"\"\"\n    return 1\n");
        assert_eq!(out, "def f():\n    \"\"\"ID: existing\"\"\"\n    return 1\n");
    }

    #[test]
    fn prose_mentioning_id_is_treated_as_already_keyed() {
        // Measured with the shared reader (object 1, exercise 02 object 6):
        // `id_from_body` finds `ID` -> `nothing-real,` here, so DefScanner
        // already recorded `id: Some(..)` and plan_ids must not add a second
        // key — consistent with collection, by construction.
        let src = "def f():\n    \"\"\"The text says ID: nothing-real, and nothing real below.\"\"\"\n    return 1\n";
        assert_eq!(inject(src), src);
    }

    #[test]
    fn nested_defs_get_keyed_bottom_up_with_no_collision() {
        let out = inject("class Outer:\n    def inner():\n        return 1\n");
        assert_eq!(
            out,
            "class Outer:\n    \"\"\"ID: id-1\"\"\"\n    def inner():\n        \"\"\"ID: id-2\"\"\"\n        return 1\n"
        );
    }

    #[test]
    fn a_second_run_makes_no_edit() {
        let once = inject("def f():\n    return 1\n");
        let twice = inject(&once);
        assert_eq!(once, twice);
    }
}
