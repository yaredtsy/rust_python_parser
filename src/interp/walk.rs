use ruff_python_ast::name::Name;
// src/interp/walk.rs
use ruff_python_ast::visitor::{Visitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, ExprCall, ExprRef, Stmt};
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
            Stmt::If(s) => {
                self.eval(&s.test, &mut env);
                env = self.walk_stmts(&s.body, env);
                for clause in &s.elif_else_clauses {
                    if let Some(t) = &clause.test {
                        self.eval(t, &mut env);
                    }
                    env = self.walk_stmts(&clause.body, env);
                }
            }
            // For, While, With, Try, Match: the same change, env threaded through.
            // ...
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
