//! `mro <dir> <file>` — the exercise 08 driver: for every class in the file,
//! print the `Type` variant it comes back as, the direct bases
//! (`type_hierarchy_supertypes`, one level, DAG) next to the linearised
//! `base_classes` (`vnoc::class_mro`, via `pylspt::mro`), and for
//! `attributes.py`-shaped files, the two attribute-resolution tools side by
//! side.
//!
//! See `experience/08-classes-and-mro/README.md`.

use pylspt::db::{open, open_project};
use pylspt::mro::base_classes;
use pylspt::qname::qualified_name;
use ruff_db::parsed::parsed_module;
use ruff_db::system::SystemPath;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_body};
use ruff_python_ast::{AnyNodeRef, StmtClassDef};
use ty_python_semantic::types::ProgramEnvironment;
use ty_python_semantic::types::ide_support::type_hierarchy_supertypes;
use ty_python_semantic::{HasDefinition, HasType, SemanticModel};

/// Every `StmtClassDef` in the module, nested ones included — unlike
/// `ScopeScanner` (object 5) this does not stop at the first def; exercise 08
/// needs `Outer.Inner` too.
struct ClassCollector<'a> {
    classes: Vec<&'a StmtClassDef>,
}

impl<'a> SourceOrderVisitor<'a> for ClassCollector<'a> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        if let AnyNodeRef::StmtClassDef(class_def) = node {
            self.classes.push(class_def);
        }
        TraversalSignal::Traverse
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().ok_or_else(|| anyhow::anyhow!("usage: mro <dir> <file>"))?;
    let file_arg = args.next().ok_or_else(|| anyhow::anyhow!("usage: mro <dir> <file>"))?;

    let dir = std::fs::canonicalize(&dir)?;
    let dir = SystemPath::from_std_path(&dir)
        .ok_or_else(|| anyhow::anyhow!("project root is not valid UTF-8: {dir:?}"))?;
    let db = open_project(dir)?;

    let file_path = std::fs::canonicalize(&file_arg)?;
    let file_path = SystemPath::from_std_path(&file_path)
        .ok_or_else(|| anyhow::anyhow!("file path is not valid UTF-8: {file_path:?}"))?;
    let (_file, program_file) = open(&db, file_path)?;
    let parsed = parsed_module(&db, program_file.python_file(&db)).load(&db);

    let mut collector = ClassCollector { classes: Vec::new() };
    walk_body(&mut collector, &parsed.syntax().body);

    let model = SemanticModel::new(&db, program_file);
    let env = ProgramEnvironment::from_program(program_file.program(&db));

    for class_def in collector.classes {
        let definition = class_def.definition(&model);
        let qname = qualified_name(&db, definition, &parsed)
            .unwrap_or_else(|| format!("<unresolved>.{}", class_def.name));

        let Some(ty) = class_def.inferred_type(&model) else {
            println!("{qname}: <no inferred type>");
            continue;
        };

        // Step 1: which `Type` variant did we actually get?
        let variant = match ty {
            ty_python_semantic::types::Type::ClassLiteral(_) => "ClassLiteral",
            ty_python_semantic::types::Type::GenericAlias(_) => "GenericAlias",
            _ => "<other>",
        };

        // Step 2: the direct bases (one level, DAG) next to the
        // linearisation ty's MRO actually computed from them.
        let direct: Vec<String> = type_hierarchy_supertypes(&db, &env, ty)
            .iter()
            .map(|base| base.name.to_string())
            .collect();
        let linearised = base_classes(&db, ty);

        println!("{qname} [{variant}]");
        println!("  direct bases (DAG):   {direct:?}");
        println!("  base_classes (C3):    {linearised:?}");
    }

    Ok(())
}
