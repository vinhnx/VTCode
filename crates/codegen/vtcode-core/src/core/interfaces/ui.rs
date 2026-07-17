use async_trait::async_trait;

use crate::ui::tui::{InlineEvent, InlineHandle, InlineSession};

/// Common contract for interactive inline UI sessions.
#[async_trait]
pub trait UiSession {
    fn inline_handle(&self) -> &InlineHandle;

    fn clone_inline_handle(&self) -> InlineHandle {
        self.inline_handle().clone()
    }

    async fn next_event(&mut self) -> Option<InlineEvent>;

    fn request_redraw(&self) {
        self.inline_handle().force_redraw();
    }

    fn shutdown(&self) {
        self.inline_handle().shutdown();
    }
}

#[async_trait]
impl UiSession for InlineSession {
    fn inline_handle(&self) -> &InlineHandle {
        &self.handle
    }

    async fn next_event(&mut self) -> Option<InlineEvent> {
        self.events.recv().await
    }
}
