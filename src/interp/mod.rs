mod env;

mod bind;
mod call;
#[cfg(test)]
mod fixture;
mod member;
mod tree;
mod value;
mod walk;
pub use tree::{Mark, Node, to_text};

use ruff_db::files::File;
use ty_project::ProjectDatabase;

use crate::{
    interp::{env::Env, walk::Walk},
    sem::Sem,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Follow every call into the callee's body (parts 01 and 03).
    Inline,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub mode: Mode,
}

impl Default for Options {
    fn default() -> Self {
        Options { mode: Mode::Inline }
    }
}

/// The call tree under `entry`, a function in `file`. `None` when there is no
/// such function.
pub fn call_tree(db: &ProjectDatabase, file: File, entry: &str, _opts: &Options) -> Option<Node> {
    let sem = Sem::new(db);
    let def = sem.entry(file, entry)?;
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed)?;
    let mut walk = Walk::new(&sem, def);
    let qname = sem.qname(def);
    walk.path.push(qname.clone());
    walk.walk_stmts(&node.body, Env::default());

    let mut root = Node::root(sem.qname(def));
    root.children = walk.frame.children;
    Some(root)
}
