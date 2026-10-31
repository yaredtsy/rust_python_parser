use ruff_db::files::File;
use ruff_db::parsed::{ParsedModuleRef, parsed_module};
use ruff_python_ast::{Alias, AnyNodeRef, ExprName, ExprRef, Stmt, StmtFunctionDef};
use ruff_text_size::{Ranged, TextSize};

use ty_project::glob::IncludeResult;
use ty_project::{Db as _, ProjectDatabase};
use ty_python_core::ProgramFile;
use ty_python_core::scope::{FileScopeId, NodeWithScopeRef, ScopeKind};
use ty_python_core::semantic_index;
use ty_python_semantic::types::vnoc;
use ty_python_semantic::{
    Db as _, HasDefinition, HasType, ImportAliasResolution, ResolvedDefinition,
    definitions_for_name,
};

pub use ruff_python_ast::name::Name;
pub use ty_python_core::definition::{Definition, DefinitionKind};
pub use ty_python_semantic::types::{Type, TypeDefinition};
pub use ty_python_semantic::{ProgramEnvironment, SemanticModel};

/// Decorators that keep the wrapped function's signature.
pub const SIGNATURE_KEEPING: &[&str] = &[
    "functools.wraps",
    "functools.lru_cache",
    "functools.cache",
    "builtins.classmethod",
    "builtins.staticmethod",
];

pub struct Sem<'db> {
    pub db: &'db ProjectDatabase,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BodyScope<'db> {
    pub file: ProgramFile<'db>,
    pub scope: FileScopeId,
}
/// What a bound method is bound to.
pub enum BoundTo {
    Object,
    Class,
    Other,
}
pub enum Shape<'db> {
    Function(Definition<'db>), // a `def`, as ty sees it
    Class(Definition<'db>),
    Instance(Definition<'db>), // an object of this class, whichever one
    BoundMethod {
        func: Definition<'db>,
        receiver: Option<Type<'db>>, // what ty bound it to
    },
    SubclassOf(Option<Definition<'db>>), // `type[X]`; `None` for `type[Any]`, `type[T]`
    Alias(Type<'db>),                    // a type alias: what it stands for
    Union(Vec<Type<'db>>), // keep the alternatives
    Module,
    Unknown, // Any, Unknown, Never: nothing to go on
    Other,   // keep as `Ty`
}
impl<'db> Sem<'db> {
    pub fn definition_of(
        &self,
        model: &SemanticModel<'db>,
        d: &StmtFunctionDef,
    ) -> Definition<'db> {
        d.definition(model)
    }
    /// The scope of a function's body. [check] the accessor name.
    pub fn body_scope(&self, def: Definition<'db>, node: &StmtFunctionDef) -> BodyScope<'db> {
        let file = def.program_file(self.db);
        let index = semantic_index(self.db, file);
        BodyScope {
            file,
            scope: index.node_scope(NodeWithScopeRef::Function(node)),
        }
    }
    pub fn class_type(&self, class: Definition<'db>) -> Type<'db> {
        vnoc::type_of_definition(self.db, class)
    }
    /// An instance of the class, for looking up attributes on an object.
    pub fn instance_type(&self, class: Definition<'db>) -> Type<'db> {
        let env = ProgramEnvironment::from_file(class.program_file(self.db));
        vnoc::instance_of(self.db, &env, self.class_type(class)).unwrap_or(Type::unknown())
    }

    /// What ty bound a method to, by kind only. Matches the variant of `__self__`,
    /// never compares it with a type built here: `Box[int]` and `Box` must both count.
    pub fn bound_to(&self, method: Type<'db>) -> BoundTo {
        match vnoc::bound_method_self(self.db, method) {
            Some(Type::NominalInstance(_)) => BoundTo::Object,
            Some(Type::ClassLiteral(_) | Type::GenericAlias(_) | Type::SubclassOf(_)) => {
                BoundTo::Class
            }
            _ => BoundTo::Other,
        }
    }

    /// `__init__` as found through the MRO, if it is a `def` somewhere.
    pub fn init_of(&self, class: Definition<'db>) -> Option<Definition<'db>> {
        let file = class.program_file(self.db);
        let t = self.member(self.class_type(class), "__init__", file)?;
        match self.shape(t, file) {
            Shape::Function(d) => Some(d),
            _ => None,
        }
    }
    /// If `name` is a free variable of `scope`, the enclosing FUNCTION scope
    /// that binds it. `None` for a local, a global or a builtin.
    pub fn free_variable_scope(&self, scope: BodyScope<'db>, name: &str) -> Option<BodyScope<'db>> {
        let index = semantic_index(self.db, scope.file);
        let mut ancestors = index.ancestor_scopes(scope.scope);
        ancestors.next(); // the scope itself
        if index
            .place_table(scope.scope)
            .symbol_by_name(name)
            .is_some_and(|s| s.is_bound())
        {
            return None; // bound here: a local, even if not assigned yet on this path
        }
        for (id, s) in ancestors {
            match s.kind() {
                ScopeKind::Class | ScopeKind::TypeParams => continue, // skipped by name resolution
                ScopeKind::Module => return None,                     // a global
                _ => {
                    if index
                        .place_table(id)
                        .symbol_by_name(name)
                        .is_some_and(|s| s.is_bound())
                    {
                        return Some(BodyScope {
                            file: scope.file,
                            scope: id,
                        });
                    }
                }
            }
        }
        None
    }
    /// Names `func` (or a function nested in it) reads that are bound in an
    /// enclosing FUNCTION scope outside `func`. Sorted.
    pub fn free_variables(&self, func: Definition<'db>) -> Vec<Name> {
        let parsed = self.parsed(func);
        let Some(node) = self.function_node(func, &parsed) else {
            return Vec::new();
        };
        let own = self.body_scope(func, node);
        let index = semantic_index(self.db, own.file);
        let mut scopes = vec![own.scope];
        let mut i = 0;
        while i < scopes.len() {
            let kids: Vec<_> = index.child_scopes(scopes[i]).map(|(id, _)| id).collect();
            scopes.extend(kids);
            i += 1;
        }
        let mut out = Vec::new();
        for scope in scopes {
            for symbol in index.place_table(scope).symbols() {
                if !symbol.is_used() {
                    continue;
                }
                let at = BodyScope {
                    file: own.file,
                    scope,
                };
                if let Some(bound) = self.free_variable_scope(at, symbol.name().as_str()) {
                    // free in `scope`, and bound outside `func`
                    if !index
                        .ancestor_scopes(bound.scope)
                        .any(|(id, _)| id == own.scope)
                    {
                        out.push(symbol.name().clone());
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
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
    pub fn member(&self, ty: Type<'db>, name: &str, file: ProgramFile<'db>) -> Option<Type<'db>> {
        let env = ProgramEnvironment::from_file(file);
        vnoc::member(self.db, &env, ty, name)
    }
    /// A literal's class (`Literal[3]` is `int`); anything else unchanged.
    pub fn widen(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Type<'db> {
        let env = ProgramEnvironment::from_file(file);
        vnoc::widen(self.db, &env, ty)
    }
    pub fn type_of_import(&self, model: &SemanticModel<'db>, alias: &Alias) -> Type<'db> {
        alias.inferred_type(model).unwrap_or(Type::unknown())
    }
    pub fn shape(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Shape<'db> {
        match ty {
            Type::Union(u) => Shape::Union(u.elements(self.db).to_vec()), // [check] accessor
            Type::Dynamic(_) | Type::Never => Shape::Unknown,
            Type::ModuleLiteral(_) => Shape::Module,
            Type::FunctionLiteral(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::Function(d)) => Shape::Function(d),
                    _ => Shape::Other,
                }
            }
            Type::BoundMethod(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::Function(d)) => Shape::BoundMethod {
                        func: d,
                        receiver: vnoc::bound_method_self(self.db, ty),
                    },
                    _ => Shape::Other,
                }
            }
            Type::NominalInstance(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::StaticClass(d)) => Shape::Instance(d),
                    _ => Shape::Other,
                }
            }
            Type::SubclassOf(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::StaticClass(d)) => Shape::SubclassOf(Some(d)),
                    _ => Shape::SubclassOf(None),
                }
            }
            Type::TypeAlias(alias) => Shape::Alias(alias.value_type(self.db)),
            Type::ClassLiteral(_) | Type::GenericAlias(_) => {
                let env = ProgramEnvironment::from_file(file);
                match ty.definition(self.db, &env) {
                    Some(TypeDefinition::StaticClass(d)) => Shape::Class(d),
                    _ => Shape::Other,
                }
            }
            _ => Shape::Other,
        }
    }

    pub fn none_type(&self, file: ProgramFile<'db>) -> Type<'db> {
        Type::none(self.db, &ProgramEnvironment::from_file(file))
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

    /// If `name` refers to `def` statements with decorators, those defs.
    pub fn decorated_defs(
        &self,
        model: &SemanticModel<'db>,
        name: &ExprName,
    ) -> Option<Vec<Definition<'db>>> {
        let found = definitions_for_name(
            model,
            name.id.as_str(),
            AnyNodeRef::ExprName(name),
            ImportAliasResolution::ResolveAliases,
        );
        let mut defs = Vec::new();
        for r in found {
            let ResolvedDefinition::Definition(def) = r else {
                return None;
            };
            let parsed = self.parsed(def);
            let node = self.function_node(def, &parsed)?; // not a def: not this rule
            if node.decorator_list.is_empty() {
                return None; // undecorated: ty's answer is the def anyway
            }
            defs.push(def);
        }
        (!defs.is_empty()).then_some(defs)
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

    /// Where the definition lives: its file path, then the offset of its `def`.
    /// A stable order for nodes that the walk can reach in any order.
    pub fn def_position(&self, def: Definition<'db>) -> (String, TextSize) {
        let path = def.file(self.db).path(self.db).to_string();
        let parsed = self.parsed(def);
        let start = self
            .function_node(def, &parsed)
            .map_or(TextSize::default(), |f| f.range().start());
        (path, start)
    }

    pub fn qname(&self, def: Definition<'db>) -> String {
        let parsed = parsed_module(self.db, def.python_file(self.db)).load(self.db);
        crate::qname::qualified_name(self.db, def, &parsed).unwrap_or_else(|| "<unknown>".into())
    }
}
