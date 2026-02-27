use super::*;

impl Session {
    pub fn set_log_receiver(&mut self, receiver: UnboundedReceiver<LogEntry>) {
        self.log_receiver = Some(receiver);
    }

    fn push_log_line(&mut self, text: Arc<Text<'static>>) {
        if self.log_lines.len() >= MAX_LOG_LINES {
            self.log_lines.pop_front();
            self.log_evicted = true;
        }
        self.log_lines.push_back(text);
        self.log_cached_text = None;
    }

    pub(crate) fn poll_log_entries(&mut self) {
        if !self.show_logs {
            // Drain without processing to avoid accumulation
            if let Some(receiver) = self.log_receiver.as_mut() {
                while receiver.try_recv().is_ok() {}
            }
            return;
        }

        if let Some(receiver) = self.log_receiver.as_mut() {
            let mut drained = Vec::new();
            while drained.len() < MAX_LOG_DRAIN_PER_TICK {
                let Ok(entry) = receiver.try_recv() else {
                    break;
                };
                drained.push(entry);
            }
            if !drained.is_empty() {
                for entry in drained {
                    let rendered = Arc::new(highlight_log_entry(&entry));
                    self.push_log_line(rendered);
                }
                self.mark_dirty();
            }
        }
    }

    pub(crate) fn has_logs(&self) -> bool {
        !self.log_lines.is_empty()
    }

    pub(crate) fn log_text(&mut self) -> Arc<Text<'static>> {
        if let Some(cached) = &self.log_cached_text {
            return Arc::clone(cached);
        }

        let mut text = Text::default();
        if self.log_evicted {
            text.lines.push(Line::from("(oldest logs dropped)"));
        }

        for entry in self.log_lines.iter() {
            text.lines.extend(entry.lines.clone());
        }

        if text.lines.is_empty() {
            text.lines.push(Line::from("No logs yet"));
        }

        let arc = Arc::new(text);
        self.log_cached_text = Some(Arc::clone(&arc));
        arc
    }
}
