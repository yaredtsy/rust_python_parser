// src/interp/walk.rs
use ruff_python_ast::visitor::{Visitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, ExprCall, ExprRef, Stmt};
use ruff_text_size::Ranged;
use smallvec::smallvec;
use ty_python_core::ProgramFile;

use crate::interp::tree::Node;
use crate::interp::value::AbstractValue::Unknown;
use crate::interp::value::{AbstractValue, Values};
use crate::sem::{Definition, Sem, SemanticModel};

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
        }
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
            Expr::Lambda(_) => smallvec![AbstractValue::Unknown],

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
        for arg in call.arguments.arguments_source_order() {
            self.eval(arg.value());
        }
        for callee in &callees {
            if let AbstractValue::Function(def) = *callee
                && self.sem.is_project(def)
            {
                self.frame
                    .children
                    .push(Node::call(self.sem.qname(def), call.range()));
            }
        }

        let ty = self.sem.type_of(&self.frame.model, ExprRef::from(call));
        self.lift(ty)
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
