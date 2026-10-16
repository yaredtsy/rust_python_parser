use crate::positions::Position;

/// One output node per AST node. A `Call` node is exactly one `ExprCall`, so
/// the visitor's "each node is visited once" guarantee *is* the deduplication —
/// there is no chain to expand and nothing to get out of sync.
///
/// `call_index` / `call_col_pos` used to live here. They existed because Jedi
/// resolves by cursor position and every call in a chain shares a start, so it
/// needed a tiebreak. One node per `ExprCall` makes `position` unique, which is
/// the whole handle; the interpreter looks a call up by its range.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    /// "FunctionSchema/<uuid>" | "ClassSchema/<uuid>" | null for calls
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
    Call,
}
