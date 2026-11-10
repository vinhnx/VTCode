//! Shared input protocol types.
//!
//! `ContentPart` and `SubmittedInput` are defined here (moved from
//! `vtcode-ui`) so headless `vtcode-core` builds share one type identity
//! with the TUI instead of stubbing parallel definitions.

use std::ops::Deref;

/// Structured input content used by the TUI input/history systems.
///
/// `Image` data is boxed (base64 payloads dwarf every other variant) so a
/// `Vec<ContentPart>` of text parts does not pay the inline footprint.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Image { data: Box<str>, media_type: String },
}

impl ContentPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image(data: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self::Image {
            data: data.into().into_boxed_str(),
            media_type: media_type.into(),
        }
    }

    pub fn is_image(&self) -> bool {
        matches!(self, Self::Image { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedInput {
    pub text: String,
    pub attachments: Vec<ContentPart>,
    /// Whether this submission may be merged with other queued text-only
    /// submissions of the same agent into a single model turn. Ctrl+Enter and
    /// Tab set this when they queue plain text while busy; busy plain-Enter
    /// queues as its own non-batchable turn and slash commands stay a
    /// one-per-turn dispatch.
    pub batchable: bool,
}

impl SubmittedInput {
    pub fn new(text: impl Into<String>, attachments: Vec<ContentPart>) -> Self {
        Self { text: text.into(), attachments, batchable: false }
    }

    fn text_only(text: impl Into<String>) -> Self {
        Self::new(text, Vec::new())
    }

    /// Mark this submission as eligible for queue batching (Ctrl+Enter).
    pub fn batchable(mut self) -> Self {
        self.batchable = true;
        self
    }

    pub fn trim_text(self) -> Self {
        Self {
            text: self.text.trim().to_string(),
            attachments: self.attachments,
            batchable: self.batchable,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.attachments.is_empty()
    }

    pub fn has_attachments(&self) -> bool {
        !self.attachments.is_empty()
    }

    /// Whether this submission may steer a live turn: plain text only.
    /// Slash commands are runloop actions and the steering channel is
    /// text-only, so those stay queued.
    pub fn is_steerable_text(&self) -> bool {
        !self.has_attachments() && !self.text.trim_start().starts_with('/')
    }
}

impl From<String> for SubmittedInput {
    fn from(text: String) -> Self {
        Self::text_only(text)
    }
}

impl From<&str> for SubmittedInput {
    fn from(text: &str) -> Self {
        Self::text_only(text)
    }
}

impl Deref for SubmittedInput {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl PartialEq<&str> for SubmittedInput {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

impl PartialEq<&str> for &SubmittedInput {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

impl PartialEq<String> for SubmittedInput {
    fn eq(&self, other: &String) -> bool {
        self.text == *other
    }
}

impl PartialEq<&String> for SubmittedInput {
    fn eq(&self, other: &&String) -> bool {
        self.text == **other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steerable_text_matrix() {
        assert!(SubmittedInput::from("run tests").is_steerable_text());
        assert!(!SubmittedInput::from("/compact now").is_steerable_text());
        assert!(!SubmittedInput::from("  /compact now").is_steerable_text());
        assert!(
            !SubmittedInput::new("describe this", vec![ContentPart::image("aGVsbG8=", "image/png")])
                .is_steerable_text()
        );
    }

    #[test]
    fn content_part_constructors_keep_shape() {
        let text = ContentPart::text("hello");
        assert!(!text.is_image());
        assert!(matches!(text, ContentPart::Text { .. }));

        let image = ContentPart::image("aGVsbG8=", "image/png");
        assert!(image.is_image());
        assert!(matches!(
            image,
            ContentPart::Image { ref media_type, .. } if media_type == "image/png"
        ));
    }

    #[test]
    fn batchable_and_trim_roundtrip() {
        let input = SubmittedInput::from("  padded  ").batchable().trim_text();
        assert!(input.batchable);
        assert_eq!(input.text, "padded");
        assert_eq!(&*input, "padded");
    }
}
