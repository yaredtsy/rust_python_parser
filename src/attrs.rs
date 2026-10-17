//! Tests for the `vnoc::member` and `vnoc::instance_of` wrappers — see
//! `experience/08-classes-and-mro/README.md`, step 5, and `src/bin/attrs.rs`
//! for the driver that prints them side by side with the public
//! `static_member_type_for_attribute`.
//!
//! No production code of its own: `vnoc` IS the production surface. This
//! module exists so those wrappers have a test in `pylspt` rather than in
//! the fork (object 8's rule — the compiler catches a changed signature on
//! rebase, only a test catches a changed *answer*).

#[cfg(test)]
mod tests {
    use crate::db::{open, open_project};
    use crate::qname::qualified_name;
    use ruff_db::parsed::{ParsedModuleRef, parsed_module};
    use ruff_db::system::SystemPath;
    use ruff_python_ast::visitor::source_order::{SourceOrderVisitor, TraversalSignal, walk_body};
    use ruff_python_ast::{AnyNodeRef, StmtClassDef};
    use ty_project::ProjectDatabase;
    use ty_python_core::ProgramFile;
    use ty_python_semantic::types::ProgramEnvironment;
    use ty_python_semantic::types::vnoc;
    use ty_python_semantic::{HasDefinition, HasType, SemanticModel};

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

    /// Opens `attributes.py` in a project rooted at the exercise dir, and
    /// leaks both the db and the parsed module to `'static` — only ever
    /// called from a `#[test]`, which is itself short-lived, so there is
    /// nothing to free.
    fn open_attributes() -> (&'static ProjectDatabase, ProgramFile<'static>, &'static ParsedModuleRef) {
        let dir = std::fs::canonicalize("experience/08-classes-and-mro").unwrap();
        let dir = SystemPath::from_std_path(&dir).unwrap();
        let db: &'static ProjectDatabase = Box::leak(Box::new(open_project(dir).unwrap()));

        let file_path = std::fs::canonicalize("experience/08-classes-and-mro/python/attributes.py").unwrap();
        let file_path = SystemPath::from_std_path(&file_path).unwrap();
        let (_file, program_file) = open(db, file_path).unwrap();

        let parsed: &'static ParsedModuleRef =
            Box::leak(Box::new(parsed_module(db, program_file.python_file(db)).load(db)));

        (db, program_file, parsed)
    }

    /// `(qualified_name, class's own Type)` for every class in `attributes.py`.
    fn classes_by_name<'db>(
        db: &'db ProjectDatabase,
        program_file: ProgramFile<'db>,
        parsed: &'db ParsedModuleRef,
    ) -> Vec<(String, ty_python_semantic::types::Type<'db>)> {
        let mut collector = ClassCollector { classes: Vec::new() };
        walk_body(&mut collector, &parsed.syntax().body);

        let model = SemanticModel::new(db, program_file);
        collector
            .classes
            .into_iter()
            .filter_map(|class_def| {
                let definition = class_def.definition(&model);
                let qname = qualified_name(db, definition, parsed)?;
                let ty = class_def.inferred_type(&model)?;
                Some((qname, ty))
            })
            .collect()
    }

    /// `vnoc::member` on `Handler`'s instance resolves `.handle` — the
    /// baseline both tools agree on (`attributes.py`'s `safe`/`inherited`
    /// shape).
    #[test]
    fn member_resolves_a_method_on_an_instance() {
        let (db, program_file, parsed) = open_attributes();
        let env = ProgramEnvironment::from_program(program_file.program(db));
        let classes = classes_by_name(db, program_file, parsed);

        let (_, handler_ty) = classes.iter().find(|(name, _)| name.ends_with(".Handler")).unwrap();
        let instance = vnoc::instance_of(db, &env, *handler_ty).expect("Handler is a class literal");
        let handle = vnoc::member(db, &env, instance, "handle");
        assert!(handle.is_some(), "Handler instance should resolve .handle");
    }

    /// Step 5's punchline: the same wrapper, given `LoudHandler`'s instance
    /// instead of `Handler`'s, resolves `.handle` to a DIFFERENT method —
    /// the receiver picks the answer, and the caller picks the receiver.
    /// This is what `static_member_type_for_attribute` cannot do for
    /// `dispatch`: it infers the receiver from syntax, so it can only ever
    /// report the union over every construction site.
    #[test]
    fn member_answer_depends_on_which_instance_you_pass() {
        let (db, program_file, parsed) = open_attributes();
        let env = ProgramEnvironment::from_program(program_file.program(db));
        let classes = classes_by_name(db, program_file, parsed);

        let handler_ty = classes.iter().find(|(name, _)| name.ends_with(".Handler")).unwrap().1;
        let loud_ty = classes.iter().find(|(name, _)| name.ends_with(".LoudHandler")).unwrap().1;

        let handler_instance = vnoc::instance_of(db, &env, handler_ty).unwrap();
        let loud_instance = vnoc::instance_of(db, &env, loud_ty).unwrap();

        let handler_handle = vnoc::member(db, &env, handler_instance, "handle").unwrap();
        let loud_handle = vnoc::member(db, &env, loud_instance, "handle").unwrap();

        // Different receivers, different bound methods — displaying to
        // different text is the cheapest behaviour check available without
        // reaching into `FunctionType` internals the fork keeps private.
        assert_ne!(
            handler_handle.display(db, &env).to_string(),
            loud_handle.display(db, &env).to_string(),
        );
    }
}
