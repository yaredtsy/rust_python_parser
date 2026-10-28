# 01.02 — One call by name

**You build:** a walker over `main`'s body that finds `helper()`, works out
what `helper` is, and adds a child node for it. The callee's body is not
entered yet.
**Fixture:** `python/one_call.py`
**Plan:** `plan/03-call-tree/05-the-walk.md` § "Evaluation drives the walk",
`04-values.md` § "Why not just use `ty::Type<'db>`"

---

## The fixture

```python
def helper(): pass
def other(): pass

def main():
    helper()
    print(other())
    len("x")
    if True:
        helper()

# expect:
# main
#   helper
#   other
#   helper
```

## Predict

1. In `print(other())`, which of the two calls produces a node, and which one
   is evaluated first?
2. `len("x")` produces no node. Is the string `"x"` still evaluated? Does it
   matter?
3. The second `helper()` is inside an `if`. Without any special code for `if`,
   would a walker that only looks at `Stmt::Expr` find it?

---

## Build

### Decision 1: calls are found by evaluating, not by collecting them

There is no "find every call node, then resolve each" pass. The walker
evaluates each statement's expressions, and when evaluation reaches a call, it
resolves the call right there.

```rust
fn eval_call(&mut self, call: &ExprCall) -> Values<'db> {
    let callees = self.eval(&call.func);              // ← the decision
    for arg in call.arguments.iter_source_order() {
        self.eval(arg.value());
    }
    for callee in &callees {
        if let AbstractValue::Function(def) = *callee
            && self.sem.is_project(def)
        {
            self.frame.children.push(Node::call(self.sem.qname(def), call.range()));
        }
    }
    let ty = self.sem.type_of(&self.frame.model, ExprRef::from(call));
    self.lift(ty)                                     // the call's value: ty's answer, for now
}
```

**Why:** the callee of a call is a value, and values come from evaluation:
`f = helper; f()`, `make()()`, a callback parameter `cb()`. A list of call
nodes collected up front would need a second pass that re-derives every one of
those values, which is the walker again. The Jedi driver has exactly that
split, and it is why it re-infers the same expressions over and over.

**Rejected:** a `SourceOrderVisitor` that collects `ExprCall` nodes. It finds
the call `cb()`, and has no idea what `cb` is.

The call's value is ty's type for the call expression, for now. For a project
function that is replaced in step 10 by what the callee actually returns on
this path.

### Decision 2: Python's evaluation order

The callee first, then the arguments in source order, then the call itself.
`print(other())` therefore evaluates `print` (a value, no node), then
`other()` (a call: node `other`), then calls `print` (not project code: no
node).

**Why:** later steps depend on it. Children come out in evaluation order. In
part 03 a call can change an object, and an argument evaluated later must see
that change.

### Decision 3: your own value type, not ty's `Type`

```rust
// src/interp/value.rs
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AbstractValue<'db> {
    Function(Definition<'db>),   // a def: something you can walk
    Ty(Type<'db>),               // ty's answer, for everything you don't model
    Unknown,                     // nothing known
}

/// Several possibilities, kept apart. Never collapsed into one ty union.
pub type Values<'db> = SmallVec<[AbstractValue<'db>; 2]>;
```

**Why:** ty's `Type` describes a set of runtime values ("some function of this
signature"), and merges alternatives into one union. The walker needs "this
function, on this path", and needs each alternative separately, because each
one produces its own edge. Later it also needs facts ty has no way to state:
"this exact object, built on that line". The enum grows a variant per step
that needs one. `Ty` keeps everything else working in the meantime.

**Why `Values` and not one value:** a name can hold different things on
different paths into the same point (`f = a` or `f = b`). Each alternative
must still give its own edge.

### Decision 4: `sem` sorts ty's types into the few shapes you care about

ty's `Type` has about thirty variants, and they change upstream. The walker
cares about a handful. So `sem` answers with its own small enum, and the
walker never matches on `Type` directly:

```rust
// src/sem.rs
pub enum Shape<'db> {
    Function(Definition<'db>),   // a `def`, as ty sees it
    Union(Vec<Type<'db>>),       // keep the alternatives
    Unknown,                     // Any, Unknown, Never: nothing to go on
    Other,                       // keep as `Ty`
}
```

```rust
// src/interp/value.rs
pub fn lift(&self, ty: Type<'db>) -> Values<'db> {
    match self.sem.shape(ty, self.frame.file) {
        Shape::Function(d) => smallvec![AbstractValue::Function(d)],
        Shape::Union(members) => members.into_iter().flat_map(|t| self.lift(t)).collect(),
        Shape::Unknown => smallvec![AbstractValue::Unknown],
        Shape::Other => smallvec![AbstractValue::Ty(ty)],
    }
}
```

`Shape` gains a variant each time a later step needs one: `Module` in step 04,
`Class` and `Instance` in part 03.

### Decision 5: non-project callees get no node, but their arguments are walked

`print`, `len` and anything from site-packages is not project code, so it gets
no node. Its arguments were already evaluated by decision 2, so
`print(other())` still finds `other`. The Jedi driver finds `other()` as its
own call, so the trees agree.

"Project code" is "ty's project includes this file":

```rust
pub fn is_project(&self, def: Definition<'db>) -> bool {
    let file = def.file(self.db);
    let Some(path) = file.path(self.db).as_system_path() else { return false };   // typeshed is vendored
    matches!(self.db.project().is_file_included(self.db, path), IncludeResult::Included { .. })
}
```

**[check]** that `.venv/` and `site-packages` under the project root are
excluded by ty's default filters. If not, add the exclusion here.

### Decision 6: every expression is evaluated exactly once

The fallback arm of `eval` evaluates each direct child expression (so calls
inside a list, an f-string or a comprehension are reached), then asks ty for
the whole expression's type. Nothing is evaluated twice, because evaluating a
call twice would record its node twice.

Lambdas are the exception: their body runs only when they are called, so `eval`
does not enter them. **[check]** what the Jedi driver does with a call inside
a lambda body, and match it.

### Decision 7: statements are walked linearly, for now

`if`, `for`, `while`, `with`, `try` and `match` evaluate their head expression,
then walk each body in order, one after the other. That finds every call. It
is not the right meaning of `if`, and step 09 replaces it. Nested `def` and
`class` statements are skipped: their bodies belong to other functions (step 11
and part 03).

### Yours

- `smallvec` in `Cargo.toml`, copied from `vendor/ruff/Cargo.toml` so cargo
  keeps one copy: `smallvec = { version = "1.13.2", features = ["union", "const_generics", "const_new"] }`.
- `sem.rs`: `type_of`, `shape`, `is_project`, `parsed`, `function_node`.
- `interp/value.rs`: `AbstractValue`, `Values`, `lift`.
- `interp/walk.rs`: `Walk`, `Frame`, `walk_stmts`, `eval`, `eval_call`, and two
  small visitors: the direct child expressions of an expression, and the
  expressions of a simple statement.
- `call_tree` walks the entry's body and returns its frame's children under
  the root.

<details><summary>Reference: this step's code</summary>

```rust
// src/sem.rs (added)
use ruff_db::parsed::ParsedModuleRef;
use ruff_python_ast::{Expr, ExprRef, StmtFunctionDef};
use ty_project::Db as _;                            // for `db.project()`: a trait method
use ty_project::glob::IncludeResult;
use ty_python_core::ProgramFile;
use ty_python_semantic::types::TypeDefinition;
use ty_python_semantic::{HasType, ProgramEnvironment};

pub enum Shape<'db> {
    Function(Definition<'db>),
    Union(Vec<Type<'db>>),
    Unknown,
    Other,
}

impl<'db> Sem<'db> {
    pub fn model(&self, file: ProgramFile<'db>) -> SemanticModel<'db> {
        SemanticModel::new(self.db, file)
    }

    /// ty's type for an expression of `model`'s file. Unknown if ty has none.
    pub fn type_of(&self, model: &SemanticModel<'db>, expr: ExprRef<'_>) -> Type<'db> {
        expr.inferred_type(model).unwrap_or(Type::unknown())
    }

    pub fn shape(&self, ty: Type<'db>, file: ProgramFile<'db>) -> Shape<'db> {
        match ty {
            Type::Union(u) => Shape::Union(u.elements(self.db).to_vec()),
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
        let Some(path) = file.path(self.db).as_system_path() else { return false };
        matches!(
            self.db.project().is_file_included(self.db, path),
            IncludeResult::Included { .. }
        )
    }

    pub fn parsed(&self, def: Definition<'db>) -> ParsedModuleRef {
        parsed_module(self.db, def.python_file(self.db)).load(self.db)
    }

    /// The `def` statement of a function. `parsed` must be `self.parsed(def)`.
    pub fn function_node<'m>(&self, def: Definition<'db>, parsed: &'m ParsedModuleRef)
        -> Option<&'m StmtFunctionDef>
    {
        match def.kind(self.db) {
            DefinitionKind::Function(f) => Some(f.node(parsed)),
            _ => None,
        }
    }
}
```

```rust
// src/interp/walk.rs
use ruff_python_ast::visitor::{Visitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, ExprCall, ExprRef, Stmt};
use ruff_text_size::Ranged;
use smallvec::smallvec;
use ty_python_core::ProgramFile;

use crate::interp::tree::Node;
use crate::interp::value::{AbstractValue, Values};
use crate::sem::{Definition, SemanticModel, Sem};

pub struct Walk<'a, 'db> {
    pub sem: &'a Sem<'db>,
    pub frame: Frame<'db>,
}

/// The body being walked now.
pub struct Frame<'db> {
    pub func: Definition<'db>,
    pub file: ProgramFile<'db>,
    pub model: SemanticModel<'db>,
    pub children: Vec<Node>,
}

impl<'db> Frame<'db> {
    pub fn new(sem: &Sem<'db>, func: Definition<'db>) -> Self {
        let file = func.program_file(sem.db);
        Frame { func, file, model: sem.model(file), children: Vec::new() }
    }
}

impl<'a, 'db> Walk<'a, 'db> {
    pub fn new(sem: &'a Sem<'db>, func: Definition<'db>) -> Self {
        Walk { sem, frame: Frame::new(sem, func) }
    }

    pub fn walk_stmts(&mut self, body: &[Stmt]) {
        for stmt in body {
            self.walk_stmt(stmt);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(s) => {
                self.eval(&s.value);
            }
            Stmt::Assign(s) => {
                self.eval(&s.value); // binding: step 03
            }
            Stmt::Return(s) => {
                if let Some(v) = &s.value {
                    self.eval(v); // the value: step 10
                }
            }
            // Linear for now. Step 09 replaces these with fork and join.
            Stmt::If(s) => {
                self.eval(&s.test);
                self.walk_stmts(&s.body);
                for clause in &s.elif_else_clauses {
                    if let Some(t) = &clause.test {
                        self.eval(t);
                    }
                    self.walk_stmts(&clause.body);
                }
            }
            Stmt::For(s) => {
                self.eval(&s.iter);
                self.walk_stmts(&s.body);
                self.walk_stmts(&s.orelse);
            }
            Stmt::While(s) => {
                self.eval(&s.test);
                self.walk_stmts(&s.body);
                self.walk_stmts(&s.orelse);
            }
            Stmt::With(s) => {
                for item in &s.items {
                    self.eval(&item.context_expr);
                }
                self.walk_stmts(&s.body);
            }
            Stmt::Try(s) => {
                self.walk_stmts(&s.body);
                for h in &s.handlers {
                    let h = h.as_except_handler().unwrap();
                    self.walk_stmts(&h.body);
                }
                self.walk_stmts(&s.orelse);
                self.walk_stmts(&s.finalbody);
            }
            Stmt::Match(s) => {
                self.eval(&s.subject);
                for case in &s.cases {
                    self.walk_stmts(&case.body);
                }
            }
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {} // step 11, part 03
            other => {
                for e in simple_stmt_exprs(other) {
                    self.eval(e);
                }
            }
        }
    }

    pub fn eval(&mut self, expr: &Expr) -> Values<'db> {
        match expr {
            Expr::Call(call) => self.eval_call(call),
            Expr::Lambda(_) => smallvec![AbstractValue::Unknown], // the body runs only when called
            other => {
                for child in direct_children(other) {
                    self.eval(child);
                }
                let ty = self.sem.type_of(&self.frame.model, ExprRef::from(other));
                self.lift(ty)
            }
        }
    }

    fn eval_call(&mut self, call: &ExprCall) -> Values<'db> {
        let callees = self.eval(&call.func);
        for arg in call.arguments.iter_source_order() {
            self.eval(arg.value());
        }
        for callee in &callees {
            if let AbstractValue::Function(def) = *callee
                && self.sem.is_project(def)
            {
                self.frame.children.push(Node::call(self.sem.qname(def), call.range()));
            }
        }
        let ty = self.sem.type_of(&self.frame.model, ExprRef::from(call));
        self.lift(ty)
    }
}

/// The direct sub-expressions of `expr`, in source order.
fn direct_children(expr: &Expr) -> Vec<&Expr> {
    struct Kids<'a>(Vec<&'a Expr>);
    impl<'a> Visitor<'a> for Kids<'a> {
        fn visit_expr(&mut self, e: &'a Expr) {
            self.0.push(e); // no recursion: `eval` recurses
        }
    }
    let mut kids = Kids(Vec::new());
    walk_expr(&mut kids, expr);
    kids.0
}

/// Every top-level expression of a simple statement (`assert`, `del`, `raise`, `x += …`).
fn simple_stmt_exprs(stmt: &Stmt) -> Vec<&Expr> {
    struct Exprs<'a>(Vec<&'a Expr>);
    impl<'a> Visitor<'a> for Exprs<'a> {
        fn visit_expr(&mut self, e: &'a Expr) {
            self.0.push(e);
        }
        fn visit_stmt(&mut self, _: &'a Stmt) {} // simple statements have no nested ones
    }
    let mut v = Exprs(Vec::new());
    walk_stmt(&mut v, stmt);
    v.0
}
```

```rust
// src/interp/tree.rs (added)
impl Node {
    pub fn call(qname: String, site: TextRange) -> Self {
        Node { qname, site: Some(site), marks: Vec::new(), children: Vec::new() }
    }
}
```

```rust
// src/interp/mod.rs (call_tree, replaced)
pub fn call_tree(db: &ProjectDatabase, file: File, entry: &str, _opts: &Options) -> Option<Node> {
    let sem = Sem::new(db);
    let def = sem.entry(file, entry)?;
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed)?;
    let mut walk = Walk::new(&sem, def);
    walk.walk_stmts(&node.body);
    let mut root = Node::root(sem.qname(def));
    root.children = walk.frame.children;
    Some(root)
}
```

`lift` lives in `impl Walk` in `value.rs`, as shown in decision 4.

</details>

---

## Run

```rust
#[test]
fn p01_02_one_call() {
    check("01-functions/python/one_call.py");
}
```

If you see `main` with no children, `eval` probably doesn't reach the call:
check that `Stmt::Expr` evaluates its value. If you see `print` or `len`,
`is_project` is letting typeshed through.

---

## Where you are

```
Walk  { sem, frame }
Frame { func, file, model, children: [Node] }
AbstractValue = Function | Ty | Unknown
```

The walker evaluates, finds calls, and knows a project function when it sees
one. It does not remember anything between statements yet.

---

## Exercise

1. Add `f = helper` and `f()` to `main`. Which node do you get, and who answered
   "what is `f`": your code or ty?
2. Why does the fallback arm of `eval` ask ty for the type of the whole
   expression *after* evaluating its children, instead of skipping the
   children?
3. A file has `def helper` twice, under `if X:` and `else:`. Which definition
   does `helper()` get?

<details><summary>Answers</summary>

1. `helper`. ty answered: `f` is not in any env of yours yet, so the `Name`
   arm fell to ty, which tracks `f = helper` inside the function. The next step
   makes your env answer first, because ty can't know the values that come from
   a caller.
2. The children may contain calls (`[make() for _ in xs]`, `f"{make()}"`), and
   a call is only found when evaluation reaches it. Skipping them would lose
   those nodes.
3. Both: ty's type for `helper` is a union of the two function literals, `lift`
   keeps both alternatives, and each gives a node. `Values` exists for exactly
   this.

</details>

---

→ Next: [`03-locals-and-the-env.md`](03-locals-and-the-env.md)
