use ruff_db::files::File;
use ruff_db::parsed::{ParsedModuleRef, parsed_module};
use ruff_db::source::{line_index, source_text};
use crate::stable::{StableDef, StableType};
use ty_module_resolver::file_to_module;
use ruff_python_ast::visitor::{Visitor, walk_stmt};
use ruff_python_ast::{Alias, AnyNodeRef, ExprName, ExprRef, Stmt, StmtFunctionDef};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;
use ruff_text_size::{Ranged, TextRange, TextSize};

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
    "builtins.property.setter",
];

pub struct Sem<'db> {
    pub db: &'db ProjectDatabase,
    /// Per file: a def's start offset to its `nth` among the defs with its qname.
    nth: RefCell<FxHashMap<File, Rc<FxHashMap<TextSize, u16>>>>,
    /// Per file: a def's start offset to its `__qualname__` and its 1-based place among the defs with that qualname.
    places: RefCell<FxHashMap<File, Rc<FxHashMap<TextSize, (String, u32)>>>>,
}

/// Where a def is, as the outline names it: the key the core finds a node by.
pub struct Place {
    /// An absolute path. `None` when the def has no file on disk (a builtin, a typeshed stub).
    pub file: Option<String>,
    /// `__qualname__`: a def inside a function gets `<locals>` after that function.
    pub qualname: String,
    /// 1-based, among the defs with this qualname in the file, in source order.
    pub nth: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BodyScope<'db> {
    pub file: ProgramFile<'db>,
    pub scope: FileScopeId,
}
/// A class or a function around an entry.
pub enum Scope<'db> {
    Class(Definition<'db>),
    Function(Definition<'db>),
}

/// What a function defined in a class is called on.
pub enum EntryKind {
    Method,
    ClassMethod,
    StaticMethod,
}

/// A property's project accessors. A library accessor is a stub: not walked.
pub struct PropertyParts<'db> {
    pub getter: Option<Definition<'db>>,
    pub setter: Option<Definition<'db>>,
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
    /// `None` when `name` isn't a property on `class`. Asked on the class: on an
    /// instance ty applies the descriptor and the property itself is gone.
    pub fn property_parts(&self, class: Definition<'db>, name: &str) -> Option<PropertyParts<'db>> {
        let file = class.program_file(self.db);
        let Type::PropertyInstance(p) = self.member(self.class_type(class), name, file)? else {
            return None;
        };
        // `property(get_h)` holds `get_h` as a `Callable`. Kept local: `shape` must not
        // turn every Callable that has a definition into a Function.
        let project = |t: Option<Type<'db>>| {
            let d = self.function_definition(t?, file)?;
            self.is_project(d).then_some(d)
        };
        Some(PropertyParts {
            getter: project(p.getter(self.db)),
            setter: project(p.setter(self.db)),
        })
    }

    /// An object of the class a type expression names (`Handler` → a `Handler`).
    /// Only a class-ish type; a union is the caller's to split (`shape`).
    pub fn instance_of(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Option<Type<'db>> {
        let env = ProgramEnvironment::from_file(file);
        vnoc::instance_of(self.db, &env, ty)
    }

    /// The classes and functions around `def`, outermost first, then `def` itself.
    pub fn scope_chain(&self, def: Definition<'db>) -> Vec<Scope<'db>> {
        let file = def.program_file(self.db);
        let index = semantic_index(self.db, file);
        let parsed = self.parsed(def);
        let model = self.model(file);
        let mut chain: Vec<Scope<'db>> = index
            .ancestor_scopes(def.scope(self.db).file_scope_id(self.db))
            .filter_map(|(_, scope)| match scope.node() {
                ty_python_core::scope::NodeWithScopeKind::Class(c) => {
                    Some(Scope::Class(c.node(&parsed).definition(&model)))
                }
                ty_python_core::scope::NodeWithScopeKind::Function(f) => {
                    Some(Scope::Function(f.node(&parsed).definition(&model)))
                }
                _ => None, // the module; type-parameter scopes hold nothing
            })
            .collect();
        chain.reverse();
        chain.push(Scope::Function(def));
        chain
    }

    /// A function in `class`, by what ty makes of it on an instance: the same order as `member`.
    pub fn entry_kind(&self, class: Definition<'db>, func: Definition<'db>) -> EntryKind {
        let name = self.function_name(func);
        if self.property_parts(class, &name).is_some() {
            return EntryKind::Method; // a property's getter runs on an object
        }
        let file = class.program_file(self.db);
        match self
            .member(self.instance_type(class), &name, file)
            .map(|t| (t, self.shape(t, file)))
        {
            Some((t, Shape::BoundMethod { .. })) => match self.bound_to(t) {
                BoundTo::Class => EntryKind::ClassMethod,
                _ => EntryKind::Method,
            },
            Some((_, Shape::Function(_))) => EntryKind::StaticMethod, // no receiver on an instance
            _ => EntryKind::Method, // ty can't tell (an unknown decorator): a method
        }
    }

    pub fn def_range(&self, def: Definition<'db>) -> TextRange {
        def.full_range(self.db, &self.parsed(def)).range()
    }

    pub fn function_name(&self, def: Definition<'db>) -> String {
        let parsed = self.parsed(def);
        self.function_node(def, &parsed)
            .map_or_else(String::new, |f| f.name.to_string())
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
            // `super().m` is bound to the synthesized `Self` type variable: an object
            Some(Type::NominalInstance(_) | Type::TypeVar(_)) => BoundTo::Object,
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
        Sem { db, nth: RefCell::default(), places: RefCell::default() }
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

    /// The `def` behind a function type: ty's own function, else the one a `Callable` kept.
    /// Kept apart from `callee_definition`: `shape` must not turn every Callable into a Function.
    fn function_definition(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Option<Definition<'db>> {
        match self.shape(ty, file) {
            Shape::Function(d) => Some(d),
            _ => vnoc::callable_definition(self.db, ty),
        }
    }

    /// The definition behind a callable type: its function, its class, or the
    /// function of its bound method.
    pub fn callee_definition(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Option<Definition<'db>> {
        match self.shape(ty, file) {
            Shape::Class(d) | Shape::BoundMethod { func: d, .. } => Some(d),
            _ => self.function_definition(ty, file),
        }
    }

    /// The docstring `ID:` of a project `def` or `class`. The one reader `scan` uses.
    pub fn docstring_id(&self, def: Definition<'db>) -> Option<String> {
        if !self.is_project(def) {
            return None;
        }
        let parsed = self.parsed(def);
        let body = match def.kind(self.db) {
            DefinitionKind::Function(f) => &f.node(&parsed).body,
            DefinitionKind::Class(c) => &c.node(&parsed).body,
            _ => return None,
        };
        crate::scan::id_from_body(body)
    }

    pub fn is_class(&self, def: Definition<'db>) -> bool {
        matches!(def.kind(self.db), DefinitionKind::Class(_))
    }

    /// The def's file, `__qualname__` and occurrence: the three values the outline gives it.
    pub fn place(&self, def: Definition<'db>) -> Place {
        let parsed = self.parsed(def);
        let start = match def.kind(self.db) {
            DefinitionKind::Function(f) => f.node(&parsed).range().start(),
            DefinitionKind::Class(c) => c.node(&parsed).range().start(),
            _ => self.def_position(def).1,
        };
        let file = def.file(self.db);
        let (qualname, nth) = self.places_of(file, &parsed).get(&start).cloned().unwrap_or_default();
        Place { file: file.path(self.db).as_system_path().map(|p| p.to_string()), qualname, nth }
    }

    /// The function in `file` whose `__qualname__` is `qualname`, the `nth` of them (1-based, source order). Any
    /// def can be named, wherever it sits: in a class, in a function (`outer.<locals>.inner`), under an `if`.
    pub fn entry_at(&self, file: File, qualname: &str, nth: u32) -> Option<Definition<'db>> {
        let program_file = self.db.program_file(file);
        let parsed = parsed_module(self.db, program_file.python_file(self.db)).load(self.db);
        let model = SemanticModel::new(self.db, program_file);
        let mut found = None;
        each_def(&parsed.syntax().body, &mut |stmt, q, n| {
            if let Stmt::FunctionDef(f) = stmt
                && found.is_none()
                && q == qualname
                && n == nth
            {
                found = Some(f.definition(&model));
            }
        });
        found
    }

    fn places_of(&self, file: File, parsed: &ParsedModuleRef) -> Rc<FxHashMap<TextSize, (String, u32)>> {
        if let Some(found) = self.places.borrow().get(&file) {
            return found.clone();
        }
        let mut out = FxHashMap::default();
        each_def(&parsed.syntax().body, &mut |stmt, q, n| {
            out.insert(stmt.range().start(), (q.to_string(), n));
        });
        let out = Rc::new(out);
        self.places.borrow_mut().insert(file, out.clone());
        out
    }

    /// A definition's identity outside this request: its docstring `ID:` when it
    /// is project code that has one, else where it is (module, qualified name, nth).
    pub fn stable_def(&self, def: Definition<'db>) -> StableDef {
        let parsed = self.parsed(def);
        let start = match def.kind(self.db) {
            DefinitionKind::Function(f) => f.node(&parsed).range().start(),
            DefinitionKind::Class(c) => c.node(&parsed).range().start(),
            _ => self.def_position(def).1,
        };
        if let Some(id) = self.docstring_id(def) {
            return StableDef::Node(id);
        }
        let module = file_to_module(self.db, def.program_file(self.db).resolver_file(self.db))
            .map(|m| m.name(self.db).to_string())
            .unwrap_or_default();
        let full = self.qname(def);
        let qname = full.strip_prefix(&format!("{module}.")).unwrap_or(&full).to_string();
        let nth = self.nth_of(def.file(self.db), &parsed).get(&start).copied().unwrap_or(0);
        StableDef::Path { module, qname, nth }
    }

    /// `nth` of every `def` and `class` in a file, by start offset: its place
    /// among the defs with the same qualified name, in source order.
    fn nth_of(&self, file: File, parsed: &ParsedModuleRef) -> Rc<FxHashMap<TextSize, u16>> {
        if let Some(found) = self.nth.borrow().get(&file) {
            return found.clone();
        }
        struct Walker {
            path: Vec<String>,
            seen: FxHashMap<String, u16>,
            out: FxHashMap<TextSize, u16>,
        }
        impl<'a> Visitor<'a> for Walker {
            fn visit_stmt(&mut self, stmt: &'a Stmt) {
                let name = match stmt {
                    Stmt::FunctionDef(f) => Some(f.name.to_string()),
                    Stmt::ClassDef(c) => Some(c.name.to_string()),
                    _ => None,
                };
                let Some(name) = name else {
                    return walk_stmt(self, stmt);
                };
                self.path.push(name);
                let n = self.seen.entry(self.path.join(".")).or_insert(0);
                self.out.insert(stmt.range().start(), *n);
                *n += 1;
                walk_stmt(self, stmt);
                self.path.pop();
            }
        }
        let mut w = Walker { path: Vec::new(), seen: FxHashMap::default(), out: FxHashMap::default() };
        w.visit_body(&parsed.syntax().body);
        let out = Rc::new(w.out);
        self.nth.borrow_mut().insert(file, out.clone());
        out
    }

    /// A type's identity by structure, or `None` when it can't be written
    /// one-to-one (then the key must not hold it).
    pub fn stable_type(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Option<StableType> {
        let env = ProgramEnvironment::from_file(file);
        match ty {
            Type::NominalInstance(_) => {
                let Shape::Instance(class) = self.shape(ty, file) else {
                    return None;
                };
                let args = vnoc::type_args(self.db, &env, ty)?
                    .into_iter()
                    .map(|a| self.stable_type(a, file))
                    .collect::<Option<Vec<_>>>()?;
                Some(StableType::Instance { class: self.stable_def(class), args })
            }
            Type::Callable(_) => {
                let (params, ret) = vnoc::callable_parts(self.db, ty)?;
                let params = match params {
                    Some(ps) => Some(
                        ps.into_iter()
                            .map(|p| self.stable_type(p, file))
                            .collect::<Option<Vec<_>>>()?,
                    ),
                    None => None,
                };
                Some(StableType::Callable { params, ret: Box::new(self.stable_type(ret, file)?) })
            }
            _ => None,
        }
    }

    /// One-based (line, column) of `offset` in the def's file.
    pub fn line_col(&self, def: Definition<'db>, offset: TextSize) -> (u32, u32) {
        let file = def.file(self.db);
        let source = source_text(self.db, file);
        let lc = line_index(self.db, file).line_column(offset, source.as_str());
        (lc.line.get() as u32, lc.column.get() as u32)
    }

    /// The name of a module literal type.
    pub fn module_name(&self, ty: Type<'db>) -> String {
        match ty {
            Type::ModuleLiteral(m) => m.module(self.db).name(self.db).to_string(),
            _ => String::new(),
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
                    Stmt::FunctionDef(f) if !last && f.name.as_str() == part => {
                        next = Some(&f.body[..])
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
        // A `def` keeps its node's start: call sites are stored relative to it.
        // Anything else (a class) by its definition's range: it used to be 0 for
        // every class, so sorting classes by position did nothing.
        let start = match self.function_node(def, &parsed) {
            Some(f) => f.range().start(),
            None => def.full_range(self.db, &parsed).range().start(),
        };
        (path, start)
    }

    pub fn qname(&self, def: Definition<'db>) -> String {
        let parsed = parsed_module(self.db, def.python_file(self.db)).load(self.db);
        crate::qname::qualified_name(self.db, def, &parsed).unwrap_or_else(|| "<unknown>".into())
    }
}

/// Every `def` and `class` of a body, parents before children, in source order, each with its `__qualname__`
/// and its 1-based place among the defs with that qualname.
fn each_def<'a>(body: &'a [Stmt], f: &mut dyn FnMut(&'a Stmt, &str, u32)) {
    struct Defs<'a, 'f> {
        stack: Vec<(String, bool)>, // qualname, is a function
        seen: FxHashMap<String, u32>,
        f: &'f mut dyn FnMut(&'a Stmt, &str, u32),
    }
    impl<'a> Visitor<'a> for Defs<'a, '_> {
        fn visit_stmt(&mut self, stmt: &'a Stmt) {
            let (name, is_function) = match stmt {
                Stmt::FunctionDef(f) => (f.name.as_str(), true),
                Stmt::ClassDef(c) => (c.name.as_str(), false),
                _ => return walk_stmt(self, stmt),
            };
            let qualname = match self.stack.last() {
                None => name.to_string(),
                Some((q, true)) => format!("{q}.<locals>.{name}"),
                Some((q, false)) => format!("{q}.{name}"),
            };
            let n = self.seen.entry(qualname.clone()).or_insert(0);
            *n += 1;
            (self.f)(stmt, &qualname, *n);
            self.stack.push((qualname, is_function));
            walk_stmt(self, stmt);
            self.stack.pop();
        }
    }
    let mut w = Defs { stack: Vec::new(), seen: FxHashMap::default(), f };
    w.visit_body(body);
}
