//! `attrs <dir> <file>` — exercise 08 step 5: for every `x.y(...)` call whose
//! callee is an attribute access, print the receiver's type and what each of
//! the two attribute-resolution tools says `.y` resolves to:
//!
//! - the public, AST-shaped `static_member_type_for_attribute` (ty picks the
//!   receiver — the union over every assignment anywhere in the program)
//! - the fork's `vnoc::member` (the CALLER picks the receiver)
//!
//! For `attributes.py`'s `dispatch`, the two agree only once you hand
//! `vnoc::member` a concrete receiver — `LoudHandler`'s instance type, say —
//! which is the whole point: the wrapper answers "what is `.handle` on
//! *this* type", never "which type is `self.handler` on *this* path".
//!
//! See `experience/08-classes-and-mro/README.md`, step 5.

use pylspt::db::{open, open_project};
use pylspt::qname::qualified_name;
use ruff_db::parsed::parsed_module;
use ruff_db::system::SystemPath;
use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_body};
use ruff_python_ast::{AnyNodeRef, Expr, StmtClassDef, StmtFunctionDef};
use ty_python_semantic::types::ProgramEnvironment;
use ty_python_semantic::types::ide_support::static_member_type_for_attribute;
use ty_python_semantic::types::vnoc;
use ty_python_semantic::{HasDefinition, HasType, SemanticModel};

/// Every `x.y(...)` call, tagged with the name of the function it is in.
struct CallCollector<'a> {
    stack: Vec<&'a str>,
    calls: Vec<(&'a str, &'a ruff_python_ast::ExprAttribute)>,
}

impl<'a> SourceOrderVisitor<'a> for CallCollector<'a> {
    fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
        match node {
            AnyNodeRef::StmtFunctionDef(StmtFunctionDef { name, .. }) => {
                self.stack.push(name.as_str());
            }
            AnyNodeRef::ExprCall(call) => {
                if let Expr::Attribute(attribute) = call.func.as_ref() {
                    let function = self.stack.last().copied().unwrap_or("<module>");
                    self.calls.push((function, attribute));
                }
            }
            _ => {}
        }
        TraversalSignal::Traverse
    }

    fn leave_node(&mut self, node: AnyNodeRef<'a>) {
        if matches!(node, AnyNodeRef::StmtFunctionDef(_)) {
            self.stack.pop();
        }
    }
}

/// Every `class` in the module, for looking a name up by hand (step 5's
/// "call the wrapper with `LoudHandler`'s type as the receiver").
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
    let dir = args.next().ok_or_else(|| anyhow::anyhow!("usage: attrs <dir> <file>"))?;
    let file_arg = args.next().ok_or_else(|| anyhow::anyhow!("usage: attrs <dir> <file>"))?;

    let dir = std::fs::canonicalize(&dir)?;
    let dir = SystemPath::from_std_path(&dir)
        .ok_or_else(|| anyhow::anyhow!("project root is not valid UTF-8: {dir:?}"))?;
    let db = open_project(dir)?;

    let file_path = std::fs::canonicalize(&file_arg)?;
    let file_path = SystemPath::from_std_path(&file_path)
        .ok_or_else(|| anyhow::anyhow!("file path is not valid UTF-8: {file_path:?}"))?;
    let (_file, program_file) = open(&db, file_path)?;
    let parsed = parsed_module(&db, program_file.python_file(&db)).load(&db);

    let model = SemanticModel::new(&db, program_file);
    let env = ProgramEnvironment::from_program(program_file.program(&db));

    // The two tools being compared.
    let mut calls = CallCollector { stack: Vec::new(), calls: Vec::new() };
    walk_body(&mut calls, &parsed.syntax().body);

    for &(function, attribute) in &calls.calls {
        let Some(receiver_ty) = attribute.value.inferred_type(&model) else {
            continue;
        };
        let name = attribute.attr.as_str();

        let static_result = static_member_type_for_attribute(&model, attribute);
        let wrapper_result = vnoc::member(&db, &env, receiver_ty, name);

        println!("{function}: self.{name} — receiver = {}", receiver_ty.display(&db, &env));
        println!("  static_member_type_for_attribute: {:?}", static_result.map(|t| t.display(&db, &env).to_string()));
        println!("  vnoc::member (same receiver):     {:?}", wrapper_result.map(|t| t.display(&db, &env).to_string()));
    }

    // Step 5's punchline: pick a CONCRETE receiver by hand and watch the
    // wrapper answer the path-specific question `static_member_...` cannot.
    let mut classes = ClassCollector { classes: Vec::new() };
    walk_body(&mut classes, &parsed.syntax().body);

    for class_name in ["Handler", "LoudHandler"] {
        let Some(class_def) = classes.classes.iter().find(|c| c.name.as_str() == class_name) else {
            continue;
        };
        let definition = class_def.definition(&model);
        let qname = qualified_name(&db, definition, &parsed).unwrap_or_else(|| class_name.to_string());
        let Some(class_ty) = class_def.inferred_type(&model) else {
            continue;
        };
        let Some(instance_ty) = vnoc::instance_of(&db, &env, class_ty) else {
            continue;
        };
        let handle = vnoc::member(&db, &env, instance_ty, "handle");
        println!(
            "dispatch, receiver forced to {qname}: .handle = {:?}",
            handle.map(|t| t.display(&db, &env).to_string())
        );
    }

    Ok(())
}
