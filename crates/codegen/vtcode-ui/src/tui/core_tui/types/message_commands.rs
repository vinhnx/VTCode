//! Shared text-message payload definitions and send methods for both UI protocols.
//!
//! Keep this boundary narrow: app-only captures, overlays, and deferred input
//! remain owned by the app protocol. Macros retain existing struct-variant syntax
//! and handle method signatures without merging the distinct command enums.

macro_rules! define_inline_message_commands {
    ($vis:vis enum $name:ident { $($rest:tt)* }) => {
        $vis enum $name {
            Diagnostic(Box<vtcode_commons::diagnostics::Diagnostic>),
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
            AppendLines {
                lines: Vec<vtcode_commons::ui_protocol::InlineTranscriptLine>,
            },
            $($rest)*
        }

        impl $name {
            /// Merge `next` into `self` when both are adjacent transcript appends,
            /// folding a run of streamed lines into one `AppendLines` command.
            ///
            /// Returns `Ok(())` when `next` was absorbed, or `Err(next)` unchanged
            /// when the pair cannot merge, so the drain can flush `self` and start a
            /// fresh run with the returned command. Non-append commands never merge,
            /// which keeps runs separated and preserves order.
            #[expect(
                clippy::result_large_err,
                reason = "The command is a control-flow sentinel moved back into the drain, not an error payload."
            )]
            pub(crate) fn absorb_append(&mut self, next: Self) -> Result<(), Self> {
                use vtcode_commons::ui_protocol::InlineTranscriptLine;
                match (self, next) {
                    (Self::AppendLines { lines }, Self::AppendLine { kind, segments }) => {
                        lines.push(InlineTranscriptLine { kind, segments });
                        Ok(())
                    }
                    (destination, Self::AppendLine { kind, segments }) => match destination {
                        Self::AppendLine { kind: previous_kind, segments: previous_segments } => {
                            let head = InlineTranscriptLine {
                                kind: *previous_kind,
                                segments: std::mem::take(previous_segments),
                            };
                            *destination = Self::AppendLines {
                                lines: vec![head, InlineTranscriptLine { kind, segments }],
                            };
                            Ok(())
                        }
                        _ => Err(Self::AppendLine { kind, segments }),
                    },
                    (_, other) => Err(other),
                }
            }
        }
    };
}

macro_rules! impl_inline_message_methods {
    ($command:ident) => {
        pub fn set_diagnostic_sink(&self, sink: vtcode_commons::diagnostics::DiagnosticSink) -> anyhow::Result<()> {
            self.diagnostic_observer.set_sink(sink)
        }

        pub fn diagnostic(&self, diagnostic: vtcode_commons::diagnostics::Diagnostic) {
            self.present_diagnostic(self.diagnostic_observer.prepare(&diagnostic));
        }

        /// Presentation-only path for renderer output and durable resume replay.
        pub fn present_diagnostic(&self, diagnostic: vtcode_commons::diagnostics::Diagnostic) {
            self.send_command($command::Diagnostic(Box::new(vtcode_commons::diagnostics::redact_diagnostic(
                &diagnostic,
            ))));
        }

        #[track_caller]
        fn try_append_diagnostic(&self, kind: vtcode_commons::ui_protocol::InlineMessageKind, text: &str) -> bool {
            use vtcode_commons::diagnostics::DiagnosticSeverity;
            use vtcode_commons::ui_protocol::InlineMessageKind;
            let severity = match kind {
                InlineMessageKind::Error => DiagnosticSeverity::Error,
                InlineMessageKind::Warning => DiagnosticSeverity::Warning,
                _ => return false,
            };
            if !text.trim().is_empty() {
                self.diagnostic(vtcode_commons::diagnostics::legacy_ui_diagnostic(severity, text));
            }
            true
        }

        #[track_caller]
        pub fn append_line(
            &self,
            kind: vtcode_commons::ui_protocol::InlineMessageKind,
            segments: Vec<vtcode_commons::ui_protocol::InlineSegment>,
        ) {
            if matches!(
                kind,
                vtcode_commons::ui_protocol::InlineMessageKind::Error
                    | vtcode_commons::ui_protocol::InlineMessageKind::Warning
            ) {
                let text = segments.iter().map(|segment| segment.text.as_str()).collect::<String>();
                if self.try_append_diagnostic(kind, &text) {
                    return;
                }
            }
            self.send_command($command::AppendLine { kind, segments });
        }

        #[track_caller]
        pub fn append_pasted_message(
            &self,
            kind: vtcode_commons::ui_protocol::InlineMessageKind,
            text: String,
            line_count: usize,
        ) {
            if self.try_append_diagnostic(kind, &text) {
                return;
            }
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

        #[track_caller]
        pub fn append_lines(&self, lines: Vec<vtcode_commons::ui_protocol::InlineTranscriptLine>) {
            use vtcode_commons::ui_protocol::InlineMessageKind;
            if lines.is_empty() {
                self.send_command($command::AppendLines { lines });
                return;
            }
            if !lines
                .iter()
                .any(|line| matches!(line.kind, InlineMessageKind::Error | InlineMessageKind::Warning))
            {
                self.send_command($command::AppendLines { lines });
                return;
            }
            let mut pending = Vec::new();
            for line in lines {
                if matches!(line.kind, InlineMessageKind::Error | InlineMessageKind::Warning) {
                    if !pending.is_empty() {
                        self.send_command($command::AppendLines { lines: std::mem::take(&mut pending) });
                    }
                    let text = line.segments.iter().map(|segment| segment.text.as_str()).collect::<String>();
                    self.try_append_diagnostic(line.kind, &text);
                } else {
                    pending.push(line);
                }
            }
            if !pending.is_empty() {
                self.send_command($command::AppendLines { lines: pending });
            }
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
        InlineLinkRange, InlineLinkTarget, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTranscriptLine,
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
                handle.append_lines(vec![
                    InlineTranscriptLine { kind: InlineMessageKind::User, segments: vec![first.clone()] },
                    InlineTranscriptLine { kind: InlineMessageKind::Info, segments: Vec::new() },
                ]);
                match receiver.try_recv().expect("append lines") {
                    protocol::InlineCommand::AppendLines { lines } => {
                        assert_eq!(lines.len(), 2);
                        assert_eq!(lines[0].kind, InlineMessageKind::User);
                        assert_eq!(lines[0].segments[0].text, "first");
                        assert!(Arc::ptr_eq(&lines[0].segments[0].style, &style));
                        assert_eq!(lines[1].kind, InlineMessageKind::Info);
                        assert!(lines[1].segments.is_empty());
                    }
                    _ => panic!("expected AppendLines"),
                }
                assert!(matches!(receiver.try_recv(), Err(mpsc::error::TryRecvError::Empty)));

                // The runner drain coalesces adjacent appends in place, keeping
                // the line order and folding into an existing batch.
                let mut merged = protocol::InlineCommand::AppendLine {
                    kind: InlineMessageKind::Agent,
                    segments: vec![first.clone()],
                };
                assert!(
                    merged
                        .absorb_append(protocol::InlineCommand::AppendLine {
                            kind: InlineMessageKind::Info,
                            segments: vec![second.clone()],
                        })
                        .is_ok()
                );
                assert!(
                    merged
                        .absorb_append(protocol::InlineCommand::AppendLine {
                            kind: InlineMessageKind::User,
                            segments: Vec::new(),
                        })
                        .is_ok()
                );
                match &merged {
                    protocol::InlineCommand::AppendLines { lines } => {
                        assert_eq!(lines.len(), 3);
                        assert_eq!(lines[0].kind, InlineMessageKind::Agent);
                        assert_eq!(lines[1].kind, InlineMessageKind::Info);
                        assert_eq!(lines[1].segments[0].text, "second");
                        assert_eq!(lines[2].kind, InlineMessageKind::User);
                        assert!(lines[2].segments.is_empty());
                    }
                    _ => panic!("expected merged AppendLines"),
                }
                // A non-append command is never absorbed; it separates runs and
                // comes back unchanged so the drain can handle it in order.
                let unmerged = merged
                    .absorb_append(protocol::InlineCommand::Inline {
                        kind: InlineMessageKind::Tool,
                        segment: second.clone(),
                    })
                    .expect_err("non-append command must not merge");
                assert!(matches!(unmerged, protocol::InlineCommand::Inline { .. }));
                assert!(matches!(&merged, protocol::InlineCommand::AppendLines { lines } if lines.len() == 3));
            }
        };
    }

    protocol_payload_test!(core_handle_preserves_message_payloads, crate::tui::core_tui::types);
    protocol_payload_test!(app_handle_preserves_message_payloads, crate::tui::core_tui::app::types);
}

#[cfg(test)]
mod diagnostic_tests {
    use std::sync::{Arc, Mutex};
    use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};
    use vtcode_commons::ui_protocol::{InlineMessageKind, InlineSegment, InlineTextStyle, InlineTranscriptLine};

    macro_rules! diagnostic_protocol_test {
        ($name:ident, $module:path) => {
            #[test]
            fn $name() {
                use $module as protocol;
                let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
                let handle = protocol::InlineHandle::new_for_tests(sender);
                let clone = handle.clone();
                let captured = Arc::new(Mutex::new(Vec::<Diagnostic>::new()));
                let evidence = Arc::clone(&captured);
                handle.set_diagnostic_sink(Arc::new(move |diagnostic| { evidence.lock().unwrap().push(diagnostic.clone()); Ok(()) })).unwrap();
                let segment = |text: &str| InlineSegment {text: text.into(), style: Arc::new(InlineTextStyle::default())};
                clone.append_line(InlineMessageKind::Warning, vec![segment("Visible warning\npassword=direct-ui-private-secret")]);
                let protocol::InlineCommand::Diagnostic(diagnostic) = receiver.try_recv().unwrap() else { panic!("canonical warning"); };
                assert_eq!(diagnostic.summary, "Visible warning");
                assert!(!diagnostic.agent_details.contains("direct-ui-private-secret"));
                assert_eq!(captured.lock().unwrap().len(), 1, "cloned handles share one session observer");
                clone.diagnostic(*diagnostic.clone());
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::Diagnostic(_)));
                assert_eq!(captured.lock().unwrap().len(), 1, "identical state persists once");
                handle.present_diagnostic(*diagnostic.clone());
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::Diagnostic(_)));
                assert_eq!(captured.lock().unwrap().len(), 1, "presentation and restore never re-emit");
                handle.append_lines(vec![
                    InlineTranscriptLine {kind: InlineMessageKind::Agent, segments: vec![segment("before")]},
                    InlineTranscriptLine {kind: InlineMessageKind::Error, segments: vec![segment("failure")]},
                    InlineTranscriptLine {kind: InlineMessageKind::Info, segments: vec![segment("after help")]},
                ]);
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::AppendLines { lines } if lines[0].segments[0].text == "before"));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::Diagnostic(d) if d.severity == DiagnosticSeverity::Error));
                assert!(matches!(receiver.try_recv().unwrap(), protocol::InlineCommand::AppendLines { lines } if lines[0].segments[0].text == "after help"));
                clone.set_diagnostic_sink(Arc::new(|_| anyhow::bail!("password=sink-private-secret"))).unwrap();
                handle.append_pasted_message(InlineMessageKind::Error, "Summary stays visible".into(), 1);
                let protocol::InlineCommand::Diagnostic(failed) = receiver.try_recv().unwrap() else { panic!("failed persistence retains summary"); };
                assert_eq!(failed.summary, "Summary stays visible");
                assert!(failed.agent_details.contains("Diagnostic persistence failed"));
                assert!(!failed.agent_details.contains("sink-private-secret"));
                let (other_sender, mut other_receiver) = tokio::sync::mpsc::unbounded_channel();
                let other = protocol::InlineHandle::new_for_tests(other_sender);
                other.append_line(InlineMessageKind::Warning, vec![segment("Other session warning")]);
                let protocol::InlineCommand::Diagnostic(other) = other_receiver.try_recv().unwrap() else { panic!("isolated session diagnostic"); };
                assert!(other.agent_details.is_empty(), "observer failure cannot leak across sessions");
            }
        };
    }
    diagnostic_protocol_test!(
        core_direct_diagnostic_observer_preserves_order_and_isolation,
        crate::tui::core_tui::types
    );
    diagnostic_protocol_test!(
        app_direct_diagnostic_observer_preserves_order_and_isolation,
        crate::tui::core_tui::app::types
    );
}
