use ruff_text_size::TextRange;

use crate::positions::Position;

/// One output node per def. There used to be a third `NodeKind::Call`
/// variant here, one per `ExprCall` collected by `ScopeScanner` — retired:
/// stage 3 (`experience/02-parse-and-ast/08-staged-pipeline.md`) resolves a
/// call when `eval` reaches it, with no pre-collected list to keep in sync.
/// `call_index` / `call_col_pos`, the tiebreak that list needed because every
/// call in a chain shares a start, went with it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    /// "FunctionSchema/<uuid>" | "ClassSchema/<uuid>"
    pub id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: NodeKind,
    pub position: Position,
    pub children: Vec<Node>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_classes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Class,
    Function,
}
