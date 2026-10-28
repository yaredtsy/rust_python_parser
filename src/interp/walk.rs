use ruff_python_ast::name::Name;
// src/interp/walk.rs
use ruff_python_ast::visitor::{Visitor, walk_expr, walk_pattern, walk_stmt};
use ruff_python_ast::{Expr, ExprCall, ExprRef, Pattern, Stmt};
use ruff_text_size::Ranged;
use smallvec::smallvec;
use ty_python_core::ProgramFile;

use crate::interp::env::Env;
use crate::interp::tree::Node;
use crate::interp::value::AbstractValue::Unknown;
use crate::interp::value::{AbstractValue, Values};
use crate::sem::{Definition, Sem, SemanticModel};

pub struct Walk<'a, 'db> {
    pub sem: &'a Sem<'db>,
    pub frame: Frame<'db>,
    pub path: Vec<String>,
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
        Frame {
            func,
            file,
            model: sem.model(file),
            children: Vec::new(),
        }
    }
}
impl<'a, 'db> Walk<'a, 'db> {
    pub fn new(sem: &'a Sem<'db>, func: Definition<'db>) -> Self {
        Walk {
            sem,
            frame: Frame::new(sem, func),
            path: vec![],
        }
    }

    pub fn walk_stmts(&mut self, body: &[Stmt], mut env: Env<'db>) -> Env<'db> {
        for stmt in body {
            env = self.walk_stmt(stmt, env);
        }
        env
    }
    fn walk_stmt(&mut self, stmt: &Stmt, mut env: Env<'db>) -> Env<'db> {
        match stmt {
            Stmt::Expr(s) => {
                self.eval(&s.value, &mut env);
            }
            Stmt::While(s) => {
                self.eval(&s.test, &mut env);
                let before = env;
                let body = self.walk_stmts(&s.body, before.clone());
                let after = Env::join(vec![before, body]);
                env = self.walk_stmts(&s.orelse, after);
            }
            Stmt::If(s) => {
                let mut cond = env;
                self.eval(&s.test, &mut cond); // evaluated once, value ignored
                let mut arms = vec![self.walk_stmts(&s.body, cond.clone())];
                for clause in &s.elif_else_clauses {
                    if let Some(t) = &clause.test {
                        self.eval(t, &mut cond); // runs only if earlier tests failed
                    }
                    arms.push(self.walk_stmts(&clause.body, cond.clone()));
                }
                if !s.elif_else_clauses.iter().any(|c| c.test.is_none()) {
                    arms.push(cond); // no `else`: skipping is an arm
                }
                env = Env::join(arms);
            }
            Stmt::For(s) => {
                self.eval(&s.iter, &mut env);
                let before = env;
                let mut body_env = before.clone();
                self.bind_from_ty(&s.target, &mut body_env); // the loop variable: ty's element type
                let body = self.walk_stmts(&s.body, body_env);
                let after = Env::join(vec![before, body]); // ran zero times, or ran
                env = self.walk_stmts(&s.orelse, after);
            }
            Stmt::Try(s) => {
                let before = env;
                let body = self.walk_stmts(&s.body, before.clone());
                let start = Env::join(vec![before, body.clone()]);
                let mut arms = vec![self.walk_stmts(&s.orelse, body)];
                for h in &s.handlers {
                    let h = h.as_except_handler().unwrap();
                    arms.push(self.walk_stmts(&h.body, start.clone()));
                }
                let joined = Env::join(arms);
                env = self.walk_stmts(&s.finalbody, joined);
            }
            Stmt::Match(s) => {
                self.eval(&s.subject, &mut env);
                let before = env;
                let mut arms = Vec::new();
                for case in &s.cases {
                    let mut arm = before.clone();
                    for name in pattern_names(&case.pattern) {
                        arm.bind(name, smallvec![AbstractValue::Unknown]);
                    }
                    if let Some(guard) = &case.guard {
                        self.eval(guard, &mut arm);
                    }
                    arms.push(self.walk_stmts(&case.body, arm));
                }
                arms.push(before); // no case matched
                env = Env::join(arms);
            }
            Stmt::With(s) => {
                for item in &s.items {
                    self.eval(&item.context_expr, &mut env);
                    if let Some(target) = &item.optional_vars {
                        self.bind_from_ty(target, &mut env);
                    }
                }
                env = self.walk_stmts(&s.body, env);
            }
            Stmt::Import(s) => {
                for alias in &s.names {
                    let name = match &alias.asname {
                        Some(as_name) => as_name.id.clone(),
                        None => Name::new(alias.name.id.split('.').next().unwrap()),
                    };
                    let ty = self.sem.type_of_import(&self.frame.model, alias);
                    let v = self.lift(ty);
                    env.bind(&name, v);
                }
            }
            Stmt::ImportFrom(s) => {
                for alias in &s.names {
                    let name = alias.asname.as_ref().unwrap_or(&alias.name).id.clone();
                    let ty = self.sem.type_of_import(&self.frame.model, alias);
                    let v = self.lift(ty);
                    env.bind(&name, v);
                }
            }
            Stmt::Assign(s) => {
                let v = self.eval(&s.value, &mut env);
                for target in &s.targets {
                    self.bind_target(target, v.clone(), &mut env);
                }
            }
            Stmt::AnnAssign(s) => {
                if let Some(value) = &s.value {
                    let v = self.eval(value, &mut env);
                    self.bind_target(&s.target, v, &mut env);
                }
            }
            other => {
                for e in simple_stmt_exprs(other) {
                    self.eval(e, &mut env);
                }
            }
        }
        env
    }
    pub fn eval(&mut self, expr: &Expr, env: &mut Env<'db>) -> Values<'db> {
        match expr {
            Expr::Name(n) => match env.get(&n.id) {
                Some(v) => v.clone(),
                None => {
                    let ty = self.sem.type_of(&self.frame.model, ExprRef::from(expr));
                    self.lift(ty)
                }
            },

            Expr::Named(n) => {
                let v = self.eval(&n.value, env);
                self.bind_target(&n.target, v.clone(), env);
                v
            }
            Expr::Call(call) => self.eval_call(call, env),
            Expr::Lambda(_) => smallvec![AbstractValue::Unknown],
            Expr::Attribute(a) => self.eval_attribute(a, env),
            other => {
                for child in direct_children(other) {
                    self.eval(child, env);
                }
                let ty = self.sem.type_of(&self.frame.model, ExprRef::from(other));
                self.lift(ty)
            }
        }
    }
    fn bind_target(&mut self, target: &Expr, v: Values<'db>, env: &mut Env<'db>) {
        match target {
            Expr::Name(n) => env.bind(&n.id, v),
            Expr::Attribute(a) => {
                self.eval(&a.value, env); // the receiver's calls. Binding: part 03
            }
            other => self.bind_from_ty(other, env), // decision 5
        }
    }
    fn bind_from_ty(&mut self, target: &Expr, env: &mut Env<'db>) {
        match target {
            Expr::Name(n) => {
                let ty = self.sem.type_of(&self.frame.model, ExprRef::from(target));
                let v = self.lift(ty);
                env.bind(&n.id, v);
            }
            Expr::Tuple(t) => {
                for e in &t.elts {
                    self.bind_from_ty(e, env)
                }
            }
            Expr::List(l) => {
                for e in &l.elts {
                    self.bind_from_ty(e, env)
                }
            }
            Expr::Starred(st) => self.bind_from_ty(&st.value, env),
            other => {
                self.eval(other, env);
            } // `d[k]`, `a.b`: their calls
        }
    }
}

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

/// Every name a `case` pattern binds, at any depth:
/// `case [Point(x=a), *rest] as whole:` binds `a`, `rest` and `whole`.
fn pattern_names(pattern: &Pattern) -> Vec<&Name> {
    struct Names<'a>(Vec<&'a Name>);
    impl<'a> Visitor<'a> for Names<'a> {
        fn visit_pattern(&mut self, p: &'a Pattern) {
            match p {
                Pattern::MatchAs(m) => {
                    if let Some(n) = &m.name {
                        self.0.push(&n.id)
                    }
                }
                Pattern::MatchStar(m) => {
                    if let Some(n) = &m.name {
                        self.0.push(&n.id)
                    }
                }
                Pattern::MatchMapping(m) => {
                    if let Some(n) = &m.rest {
                        self.0.push(&n.id)
                    }
                }
                _ => {}
            }
            walk_pattern(self, p); // nested patterns
        }
        fn visit_expr(&mut self, _: &'a Expr) {} // `case Color.RED:` binds nothing
    }
    let mut names = Names(Vec::new());
    names.visit_pattern(pattern);
    names.0
}
