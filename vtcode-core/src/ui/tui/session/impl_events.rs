use super::*;

impl Session {
    pub fn handle_event(
        &mut self,
        event: CrosstermEvent,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) {
        match event {
            CrosstermEvent::Key(key) => {
                // Only process Press events to avoid duplicate character insertion
                // Repeat events can cause characters to be inserted multiple times
                if matches!(key.kind, KeyEventKind::Press)
                    && let Some(outbound) = events::process_key(self, key)
                {
                    self.emit_inline_event(&outbound, events, callback);
                }
            }
            CrosstermEvent::Mouse(MouseEvent { kind, .. }) => match kind {
                MouseEventKind::ScrollDown => {
                    self.scroll_line_down();
                    self.mark_dirty();
                }
                MouseEventKind::ScrollUp => {
                    self.scroll_line_up();
                    self.mark_dirty();
                }
                _ => {}
            },
            CrosstermEvent::Paste(content) => {
                if self.input_enabled {
                    self.insert_text(&content);
                    self.check_file_reference_trigger();
                    self.check_prompt_reference_trigger();
                    self.mark_dirty();
                } else if let Some(modal) = self.modal.as_mut()
                    && let (Some(list), Some(search)) = (modal.list.as_mut(), modal.search.as_mut())
                {
                    search.insert(&content);
                    list.apply_search(&search.query);
                    self.mark_dirty();
                }
            }
            CrosstermEvent::Resize(_, rows) => {
                self.apply_view_rows(rows);
                self.mark_dirty();
            }
            _ => {}
        }
    }
}
