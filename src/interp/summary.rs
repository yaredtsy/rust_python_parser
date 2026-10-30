// src/interp/summary.rs
use ruff_python_ast::StmtFunctionDef;
use ruff_text_size::{Ranged, TextRange, TextSize};
use smallvec::{SmallVec, smallvec};

use crate::interp::env::Env;
use crate::interp::key::ContextId;
use crate::interp::solver::Solver;
use crate::interp::value::{AbstractValue, Values, dedup};
use crate::interp::walk::{Walk, contains_yield};
use crate::sem::{Definition, Type};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FunctionResult<'db> {
    pub calls: Vec<CallEdge>, // one per call site reached, in evaluation order
    pub returns: SVals<'db>,       // the union of every return
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CallEdge {
    pub site: SiteId,
    pub targets: SmallVec<[Target; 2]>, // one per project callee at this site
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Target {
    Call(ContextId), // Construct and External arrive with classes and part 05
}

/// The call expression's range, relative to the start of the enclosing `def`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SiteId {
    pub start: TextSize,
    pub end: TextSize,
}

impl SiteId {
    pub fn new(range: TextRange, def_start: TextSize) -> Self {
        SiteId {
            start: range.start() - def_start,
            end: range.end() - def_start,
        }
    }
}

pub type SVals<'db> = SmallVec<[SVal<'db>; 2]>;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SVal<'db> {
    Plain(PlainVal<'db>),
    // Closure: step 07. Ref, Obj, Bound: part 04
}

/// Values with no identity. No variant can hold an ActivationId, and later
/// no OriginId, ValueId or DeferId, so a leak does not compile.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PlainVal<'db> {
    Function(Definition<'db>),
    Module(Type<'db>),
    Ty(Type<'db>),
    Unknown,
}

impl<'db> Walk<'db> {
    pub fn finish(
        mut self,
        sol: &Solver<'db>,
        end: Option<Env<'db>>,
        node: &StmtFunctionDef,
    ) -> FunctionResult<'db> {
        if end.is_some() {
            let none = self.none_value(sol);
            self.frame.returns.extend(none); // fell off the end
        }
        let mut returns = if contains_yield(&node.body) {
            smallvec![AbstractValue::Unknown]
        } else {
            std::mem::take(&mut self.frame.returns)
        };
        dedup(&mut returns);
        let returns = returns.into_iter().map(to_sval).collect();
        FunctionResult {
            calls: self.frame.calls,
            returns,
        }
    }

    /// The caller's own values for a callee's returns.
    pub fn resolve(&self, returns: &SVals<'db>) -> Values<'db> {
        returns
            .iter()
            .map(|sv| match sv {
                SVal::Plain(PlainVal::Function(d)) => AbstractValue::Function(*d),
                SVal::Plain(PlainVal::Module(t)) => AbstractValue::Module(*t),
                SVal::Plain(PlainVal::Ty(t)) => AbstractValue::Ty(*t),
                SVal::Plain(PlainVal::Unknown) => AbstractValue::Unknown,
            })
            .collect()
    }
}

fn to_sval(v: AbstractValue<'_>) -> SVal<'_> {
    SVal::Plain(match v {
        AbstractValue::Function(d) => PlainVal::Function(d),
        AbstractValue::Module(t) => PlainVal::Module(t),
        AbstractValue::Ty(t) => PlainVal::Ty(t),
        AbstractValue::Unknown => PlainVal::Unknown,
        AbstractValue::Closure { .. } => PlainVal::Unknown, // step 07
    })
}
