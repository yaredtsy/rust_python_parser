use ruff_db::files::File;
use ruff_db::parsed::{ParsedModuleRef, parsed_module};
use ruff_python_ast::{ExprRef, Stmt, StmtFunctionDef};

use ty_project::glob::IncludeResult;
use ty_project::{Db as _, ProjectDatabase};
use ty_python_core::ProgramFile;
use ty_python_semantic::{Db as _, HasDefinition, HasType};

pub use ruff_python_ast::name::Name;
pub use ty_python_core::definition::{Definition, DefinitionKind};
pub use ty_python_semantic::types::{Type, TypeDefinition};
pub use ty_python_semantic::{ProgramEnvironment, SemanticModel};

pub struct Sem<'db> {
    pub db: &'db ProjectDatabase,
}

pub enum Shape<'db> {
    Function(Definition<'db>), // a `def`, as ty sees it
    Union(Vec<Type<'db>>),     // keep the alternatives
    Unknown,                   // Any, Unknown, Never: nothing to go on
    Other,                     // keep as `Ty`
}
impl<'db> Sem<'db> {
    pub fn new(db: &'db ProjectDatabase) -> Self {
        Sem { db }
    }
    pub fn model(&self, file: ProgramFile<'db>) -> SemanticModel<'db> {
        SemanticModel::new(self.db, file)
    }
    /// ty's type for an expression of `model`'s file. Unknown if ty has none.
    pub fn type_of(&self, model: &SemanticModel<'db>, expr: ExprRef<'_>) -> Type<'db> {
        expr.inferred_type(model).unwrap_or(Type::unknown())
    }
    pub fn shape(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Shape<'db> {
        match ty {
            Type::Union(u) => Shape::Union(u.elements(self.db).to_vec()), // [check] accessor
            Type::Dynamic(_) | Type::Never => Shape::Unknown,
            Type::FunctionLiteral(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::Function(d)) => Shape::Function(d),
                    _ => Shape::Other,
                }
            }
            _ => Shape::Other,
        }
    }

    pub fn is_project(&self, def: Definition<'db>) -> bool {
        let file = def.file(self.db);
        let Some(path) = file.path(self.db).as_system_path() else {
            return false;
        };
        matches!(
            self.db.project().is_file_included(self.db, path),
            IncludeResult::Included { .. }
        )
    }
    pub fn parsed(&self, def: Definition<'db>) -> ParsedModuleRef {
        parsed_module(self.db, def.python_file(self.db)).load(self.db)
    }
    /// The `def` statement of a function. `parsed` must be `self.parsed(def)`.
    pub fn function_node<'m>(
        &self,
        def: Definition<'db>,
        parsed: &'m ParsedModuleRef,
    ) -> Option<&'m StmtFunctionDef> {
        match def.kind(self.db) {
            DefinitionKind::Function(f) => Some(f.node(parsed)),
            _ => None,
        }
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
