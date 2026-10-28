use ruff_db::files::File;
use ruff_db::parsed::parsed_module;
use ruff_python_ast::Stmt;
use ty_project::ProjectDatabase;
use ty_python_semantic::{Db as _, HasDefinition};

pub use ruff_python_ast::name::Name;
pub use ty_python_core::definition::{Definition, DefinitionKind};
pub use ty_python_semantic::SemanticModel;
pub use ty_python_semantic::types::Type;

pub struct Sem<'db> {
    pub db: &'db ProjectDatabase,
}

impl<'db> Sem<'db> {
    pub fn new(db: &'db ProjectDatabase) -> Self {
        Sem { db }
    }

    pub fn entry(&self, file: File, dotted: &str) -> Option<Definition<'db>> {
        let program_file = self.db.program_file(file);
        let parsed = parsed_module(self.db, program_file.python_file(self.db)).load(self.db);
        let model = SemanticModel::new(self.db, program_file);

        let mut body: &[Stmt] = &parsed.syntax().body;
        let mut parts = dotted.split('.').peekable();

        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            let mut next = None;
            for stmt in body {
                match stmt {
                    Stmt::FunctionDef(f) if last && f.name.as_str() == part => {
                        return Some(f.definition(&model));
                    }
                    Stmt::ClassDef(c) if !last && c.name.as_str() == part => {
                        next = Some(&c.body[..])
                    }
                    _ => {}
                }
            }
            body = next?;
        }
        None
    }

    pub fn qname(&self, def: Definition<'db>) -> String {
        let parsed = parsed_module(self.db, def.python_file(self.db)).load(self.db);
        crate::qname::qualified_name(self.db, def, &parsed).unwrap_or_else(|| "<unknown>".into())
    }
}
