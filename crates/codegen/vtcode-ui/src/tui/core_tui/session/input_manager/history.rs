use chrono::{DateTime, Utc};

use crate::tui::core_tui::types::ContentPart;

#[derive(Clone, Debug)]
pub struct InputHistoryEntry {
    pub(super) content: String,
    pub(super) elements: Vec<ContentPart>,
    created_at: DateTime<Utc>,
}

impl InputHistoryEntry {
    pub fn from_content_and_attachments(content: String, attachments: Vec<ContentPart>) -> Self {
        let mut elements = Vec::new();
        if !content.is_empty() {
            elements.push(ContentPart::text(content.clone()));
        }
        elements.extend(attachments.into_iter().filter(ContentPart::is_image));
        Self { content, elements, created_at: Utc::now() }
    }

    /// Create an entry with an explicit timestamp (used for archived history).
    pub fn from_content_and_timestamp(content: String, created_at: DateTime<Utc>) -> Self {
        let elements = if content.is_empty() {
            Vec::new()
        } else {
            vec![ContentPart::text(&content)]
        };
        Self { content, elements, created_at }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn has_attachments(&self) -> bool {
        self.elements.iter().any(|part| part.is_image())
    }

    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty() && !self.has_attachments()
    }

    pub fn attachment_elements(&self) -> Vec<ContentPart> {
        self.elements.iter().filter(|part| part.is_image()).cloned().collect()
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        self.created_at
    }
}
