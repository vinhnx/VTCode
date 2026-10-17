//! Shared text-message payload definitions and send methods for both UI protocols.
//!
//! Keep this boundary narrow: app-only captures, overlays, and deferred input
//! remain owned by the app protocol. Macros retain existing struct-variant syntax
//! and handle method signatures without merging the distinct command enums.

macro_rules! define_inline_message_commands {
    ($vis:vis enum $name:ident { $($rest:tt)* }) => {
        $vis enum $name {
            AppendLine {
                kind: vtcode_commons::ui_protocol::InlineMessageKind,
                segments: Vec<vtcode_commons::ui_protocol::InlineSegment>,
            },
            AppendPastedMessage {
                kind: vtcode_commons::ui_protocol::InlineMessageKind,
                text: String,
                line_count: usize,
            },
            Inline {
                kind: vtcode_commons::ui_protocol::InlineMessageKind,
                segment: vtcode_commons::ui_protocol::InlineSegment,
            },
            ReplaceLast {
                count: usize,
                kind: vtcode_commons::ui_protocol::InlineMessageKind,
                lines: Vec<Vec<vtcode_commons::ui_protocol::InlineSegment>>,
                link_ranges: Option<Vec<Vec<vtcode_commons::ui_protocol::InlineLinkRange>>>,
            },
            $($rest)*
        }
    };
}

macro_rules! impl_inline_message_methods {
    ($command:ident) => {
        pub fn append_line(
            &self,
            kind: vtcode_commons::ui_protocol::InlineMessageKind,
            segments: Vec<vtcode_commons::ui_protocol::InlineSegment>,
        ) {
            self.send_command($command::AppendLine { kind, segments });
        }

        pub fn append_pasted_message(
            &self,
            kind: vtcode_commons::ui_protocol::InlineMessageKind,
            text: String,
            line_count: usize,
        ) {
            self.send_command($command::AppendPastedMessage { kind, text, line_count });
        }

        pub fn inline(
            &self,
            kind: vtcode_commons::ui_protocol::InlineMessageKind,
            segment: vtcode_commons::ui_protocol::InlineSegment,
        ) {
            self.send_command($command::Inline { kind, segment });
        }

        pub fn replace_last(
            &self,
            count: usize,
            kind: vtcode_commons::ui_protocol::InlineMessageKind,
            lines: Vec<Vec<vtcode_commons::ui_protocol::InlineSegment>>,
        ) {
            self.send_command($command::ReplaceLast { count, kind, lines, link_ranges: None });
        }

        pub fn replace_last_with_links(
            &self,
            count: usize,
            kind: vtcode_commons::ui_protocol::InlineMessageKind,
            lines: Vec<Vec<vtcode_commons::ui_protocol::InlineSegment>>,
            link_ranges: Vec<Vec<vtcode_commons::ui_protocol::InlineLinkRange>>,
        ) {
            self.send_command($command::ReplaceLast { count, kind, lines, link_ranges: Some(link_ranges) });
        }
    };
}

pub(crate) use define_inline_message_commands;
pub(crate) use impl_inline_message_methods;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use vtcode_commons::ui_protocol::{
        InlineLinkRange, InlineLinkTarget, InlineMessageKind, InlineSegment, InlineTextStyle,
    };

    // Tests use the public facades and explicit payload expectations, independently
    // of the production definition/send macros.
    macro_rules! protocol_payload_test {
        ($name:ident, $module:path) => {
            #[test]
            fn $name() {
                use $module as protocol;
                let (sender, mut receiver) = mpsc::unbounded_channel();
                let handle = protocol::InlineHandle::new_for_tests(sender);
                let style = Arc::new(InlineTextStyle::default());
                let first = InlineSegment { text: "first".into(), style: style.clone() };
                let second = InlineSegment { text: "second".into(), style: style.clone() };
                let link_rows = vec![
                    vec![InlineLinkRange { start: 1, end: 4, target: InlineLinkTarget::Url("https://example.com/first".into()) }],
                    Vec::new(),
                ];
                handle.append_line(InlineMessageKind::Agent, vec![first.clone(), second.clone()]);
                handle.append_pasted_message(InlineMessageKind::Info, "left\nright".into(), 42);
                handle.inline(InlineMessageKind::Tool, second.clone());
                handle.replace_last(2, InlineMessageKind::Error, vec![vec![first.clone()], Vec::new()]);
                handle.replace_last_with_links(1, InlineMessageKind::Warning,
                    vec![vec![second.clone()], Vec::new()], link_rows.clone());

                match receiver.try_recv().expect("append") {
                    protocol::InlineCommand::AppendLine { kind, segments } => {
                        assert_eq!(kind, InlineMessageKind::Agent);
                        assert_eq!(segments.iter().map(|segment| segment.text.as_str()).collect::<Vec<_>>(), ["first", "second"]);
                        assert!(segments.iter().all(|segment| Arc::ptr_eq(&segment.style, &style)));
                    }
                    _ => panic!("expected AppendLine"),
                }
                match receiver.try_recv().expect("paste") {
                    protocol::InlineCommand::AppendPastedMessage { kind, text, line_count } => {
                        assert_eq!(kind, InlineMessageKind::Info);
                        assert_eq!(text, "left\nright");
                        assert_eq!(line_count, 42);
                    }
                    _ => panic!("expected AppendPastedMessage"),
                }
                match receiver.try_recv().expect("inline") {
                    protocol::InlineCommand::Inline { kind, segment } => {
                        assert_eq!(kind, InlineMessageKind::Tool);
                        assert_eq!(segment.text, "second");
                        assert!(Arc::ptr_eq(&segment.style, &style));
                    }
                    _ => panic!("expected Inline"),
                }
                match receiver.try_recv().expect("replace") {
                    protocol::InlineCommand::ReplaceLast { count, kind, lines, link_ranges } => {
                        assert_eq!(count, 2);
                        assert_eq!(kind, InlineMessageKind::Error);
                        assert_eq!(lines.len(), 2);
                        assert_eq!(lines[0][0].text, "first");
                        assert!(Arc::ptr_eq(&lines[0][0].style, &style));
                        assert!(lines[1].is_empty());
                        assert!(link_ranges.is_none());
                    }
                    _ => panic!("expected ReplaceLast without links"),
                }
                match receiver.try_recv().expect("replace links") {
                    protocol::InlineCommand::ReplaceLast { count, kind, lines, link_ranges } => {
                        assert_eq!(count, 1);
                        assert_eq!(kind, InlineMessageKind::Warning);
                        assert_eq!(lines.len(), 2);
                        assert_eq!(lines[0][0].text, "second");
                        assert!(lines[1].is_empty());
                        assert_eq!(link_ranges, Some(link_rows));
                    }
                    _ => panic!("expected ReplaceLast with links"),
                }
                handle.replace_last_with_links(0, InlineMessageKind::Info, Vec::new(), Vec::new());
                assert!(matches!(receiver.try_recv().expect("empty linked replacement"),
                    protocol::InlineCommand::ReplaceLast { count: 0, link_ranges: Some(links), lines, .. }
                    if links.is_empty() && lines.is_empty()));
                assert!(matches!(receiver.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
            }
        };
    }

    protocol_payload_test!(core_handle_preserves_message_payloads, crate::tui::core_tui::types);
    protocol_payload_test!(app_handle_preserves_message_payloads, crate::tui::core_tui::app::types);
}
