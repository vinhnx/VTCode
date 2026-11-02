//! Stable stream-local indexes shared by Responses decoders.

use hashbrown::HashMap;

use crate::provider::LLMError;

use super::StreamAssemblyError;

#[derive(Default)]
pub(crate) struct ToolCallIndexes {
    by_call_id: HashMap<String, usize>,
    next_index: usize,
}

impl ToolCallIndexes {
    pub(crate) fn get(&self, call_id: &str) -> Option<usize> {
        self.by_call_id.get(call_id).copied()
    }

    /// Explicit indexes take precedence. Repeated implicit calls retain their
    /// index; new calls follow the largest index observed so far. Reject an
    /// exhausted index space before changing state rather than overflowing.
    pub(crate) fn resolve(
        &mut self,
        call_id: &str,
        output_index: Option<usize>,
        provider: &str,
    ) -> Result<usize, LLMError> {
        if output_index.is_none()
            && let Some(index) = self.get(call_id)
        {
            return Ok(index);
        }
        let index = output_index.unwrap_or(self.next_index);
        let next_index = index.checked_add(1).ok_or_else(|| {
            StreamAssemblyError::InvalidPayload("tool call index space exhausted".into()).into_llm_error(provider)
        })?;
        self.next_index = self.next_index.max(next_index);
        self.by_call_id.insert(call_id.to_owned(), index);
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_preserve_ids_explicit_precedence_and_sparse_order() {
        let mut indexes = ToolCallIndexes::default();
        assert_eq!(indexes.resolve("first", None, "test").unwrap(), 0);
        assert_eq!(indexes.resolve("sparse", Some(7), "test").unwrap(), 7);
        assert_eq!(indexes.resolve("first", None, "test").unwrap(), 0);
        assert_eq!(indexes.resolve("next", None, "test").unwrap(), 8);
        assert_eq!(indexes.resolve("sparse", Some(2), "test").unwrap(), 2);
        assert_eq!(indexes.resolve("last", None, "test").unwrap(), 9);
    }

    #[test]
    fn overflow_rejection_preserves_previous_mappings_and_allocation() {
        let mut indexes = ToolCallIndexes::default();
        assert_eq!(indexes.resolve("first", None, "test").unwrap(), 0);
        assert!(indexes.resolve("first", Some(usize::MAX), "test").is_err());
        assert_eq!(indexes.get("first"), Some(0));
        assert_eq!(indexes.resolve("second", None, "test").unwrap(), 1);

        assert_eq!(indexes.resolve("edge", Some(usize::MAX - 1), "test").unwrap(), usize::MAX - 1);
        assert!(indexes.resolve("exhausted", None, "test").is_err());
        assert_eq!(indexes.get("exhausted"), None);
        assert_eq!(indexes.resolve("first", None, "test").unwrap(), 0);
    }
}
