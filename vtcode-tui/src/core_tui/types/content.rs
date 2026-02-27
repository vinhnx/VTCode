use serde::{Deserialize, Serialize};

/// Structured input content used by the TUI input/history systems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Image { data: String, media_type: String },
}

impl ContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image(data: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self::Image {
            data: data.into(),
            media_type: media_type.into(),
        }
    }

    pub fn is_image(&self) -> bool {
        matches!(self, Self::Image { .. })
    }
}
