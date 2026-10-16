//! `base_classes`: direct bases via the public `type_hierarchy_supertypes`,
//! linearised with C3 to match Jedi's `py__mro__()`.
//!
//! See `experience/08-classes-and-mro/README.md`. The private MRO the plan
//! assumed (`ClassLiteral::iter_mro`) does not compile under Option B — this
//! is the public-API replacement, and the DAG-vs-C3 gap it leaves behind.

use ruff_db::parsed::parsed_module;
use ruff_python_ast::AnyNodeRef;
use ruff_python_ast::find_node::covering_node;
use ty_python_core::semantic_index;
use ty_python_semantic::types::ProgramEnvironment;
use ty_python_semantic::types::Type;
use ty_python_semantic::types::ide_support::{TypeHierarchyClass, type_hierarchy_supertypes};
use ty_python_semantic::{Db, HasType, SemanticModel};

use crate::qname::qualified_name;

/// `base_classes` for a class, in C3 order, with `builtins.object` last — the
/// class itself is not included (matching the `BaseNode` wire shape:
/// `"base_classes": ["pkg.mod.Base", "builtins.object"]`).
pub fn base_classes<'db>(db: &'db dyn Db, env: &ProgramEnvironment<'db>, ty: Type<'db>) -> Vec<String> {
    let bases = direct_bases(db, env, ty);

    // C3: `L[C] = C + merge(L[B1], …, L[Bn], [B1, …, Bn])`. This computes the
    // merge (everything but the leading `C`), so callers get just the bases.
    let mut sequences: Vec<Vec<String>> = bases
        .iter()
        .map(|(qname, base_ty)| {
            let mut linearisation = base_classes(db, env, *base_ty);
            linearisation.insert(0, qname.clone());
            linearisation
        })
        .collect();
    sequences.push(bases.into_iter().map(|(qname, _)| qname).collect());

    c3_merge(sequences)
}

/// One level of bases, each resolved back to a `(qualified_name, Type)` pair
/// so the caller can recurse into it.
fn direct_bases<'db>(db: &'db dyn Db, env: &ProgramEnvironment<'db>, ty: Type<'db>) -> Vec<(String, Type<'db>)> {
    type_hierarchy_supertypes(db, env, ty)
        .iter()
        .filter_map(|base| resolve(db, base))
        .collect()
}

/// `type_hierarchy_supertypes` hands back a `(name, file, range)` — not a
/// `Type` — because it is a tree-view IDE API, not an inference one. To
/// recurse (or to name the base precisely, nested classes included) we have
/// to go back through the AST: find the `StmtClassDef` the range covers, then
/// use the SAME `HasDefinition`/`HasType` path exercise 06 and the class's own
/// definition use, rather than any private accessor on `ClassLiteral`.
fn resolve<'db>(db: &'db dyn Db, base: &TypeHierarchyClass<'db>) -> Option<(String, Type<'db>)> {
    let file = base.file.file(db);
    let program_file = db.program_file(file);
    let parsed = parsed_module(db, program_file.python_file(db)).load(db);

    // `selection_range` is the class NAME's range — a leaf, so `covering_node`
    // always lands exactly on it, unambiguously. `full_range` is only the
    // HEADER (`ClassLiteral::header_range`, ide_support.rs), which is not any
    // single AST node's own range, so looking for `StmtClassDef` AT that node
    // (rather than walking up from it) silently misses every time.
    let root = AnyNodeRef::from(parsed.syntax());
    let class_def = covering_node(root, base.selection_range)
        .ancestors()
        .find_map(|node| match node {
            AnyNodeRef::StmtClassDef(class_def) => Some(class_def),
            _ => None,
        })?;

    let index = semantic_index(db, program_file);
    let definition = *index.definitions(class_def).first()?;
    let qname = qualified_name(db, definition, &parsed)?;

    let model = SemanticModel::new(db, program_file);
    let ty = class_def.inferred_type(&model)?;
    Some((qname, ty))
}

/// Python's own C3 algorithm: repeatedly take the first head that appears in
/// no other list's tail, consuming it from every list it leads.
///
/// Degrades to "take the first remaining head anyway" on an inconsistent
/// hierarchy rather than erroring — the driver must not die on pathological
/// input (quirk 13), and an MRO conflict is exactly that.
fn c3_merge(mut sequences: Vec<Vec<String>>) -> Vec<String> {
    let mut result = Vec::new();
    loop {
        sequences.retain(|s| !s.is_empty());
        if sequences.is_empty() {
            return result;
        }

        let head = sequences
            .iter()
            .map(|s| &s[0])
            .find(|candidate| sequences.iter().all(|s| !s[1..].contains(candidate)))
            .cloned()
            .unwrap_or_else(|| sequences[0][0].clone());

        for s in &mut sequences {
            if s.first() == Some(&head) {
                s.remove(0);
            }
        }
        result.push(head);
    }
}
