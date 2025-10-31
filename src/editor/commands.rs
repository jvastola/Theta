use crate::network::EntityHandle;
use serde::{Deserialize, Serialize};

pub const CMD_SELECTION_HIGHLIGHT: &str = "editor.selection.highlight";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectionHighlightCommand {
    pub entity: EntityHandle,
    pub active: bool,
}

impl SelectionHighlightCommand {
    pub fn new(entity: EntityHandle, active: bool) -> Self {
        Self { entity, active }
    }
}
