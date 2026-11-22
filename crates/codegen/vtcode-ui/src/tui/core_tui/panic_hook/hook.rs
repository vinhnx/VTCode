use std::any::Any;
use std::backtrace::Backtrace;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::panic;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use better_panic::{Settings as BetterPanicSettings, Verbosity as BetterPanicVerbosity};
use human_panic::{Metadata as HumanPanicMetadata, handle_dump as human_panic_dump, print_msg};

use super::restore::restore_tui;
use super::state::{self, AppMetadata};

static PANIC_HOOK_ONCE: std::sync::Once = std::sync::Once::new();
static PANIC_COUNT: AtomicU64 = AtomicU64::new(0);

/// Panic payloads are `&'static str` or `String` in practice. Mirror the
/// standard hook's downcast order so the recorded message stays faithful.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "Box<dyn Any>"
    }
}

fn location_label(location: &panic::Location<'_>) -> String {
    format!("{}:{}:{}", location.file(), location.line(), location.column())
}

/// Semantic context extracted once from a real panic and shared by every
/// sink: the structured tracing event and the crash report's TOML comment.
struct PanicContext {
    thread: String,
    message: String,
    location: String,
    count: u64,
}

impl PanicContext {
    fn capture(info: &panic::PanicHookInfo<'_>) -> Self {
        let thread = std::thread::current();
        Self {
            thread: thread.name().unwrap_or("<unnamed>").to_string(),
            message: panic_message(info.payload()).to_string(),
            location: info.location().map(location_label).unwrap_or_else(|| "<unknown>".to_string()),
            count: PANIC_COUNT.fetch_add(1, Ordering::Relaxed) + 1,
        }
    }
}

/// Emit one structured ERROR event for a panic, before the mode-specific
/// rendering. The message repeats thread and location because the session
/// error collector stores only the `message` field; the typed fields feed the
/// debug log.
fn record_panic(ctx: &PanicContext) {
    let backtrace = Backtrace::capture();
    tracing::error!(
        target: "vtcode.panic",
        thread = ctx.thread,
        location = %ctx.location,
        panic_count = ctx.count,
        backtrace = %backtrace,
        "panic on thread '{}' at {}: {}",
        ctx.thread,
        ctx.location,
        ctx.message
    );
}

/// The default tracing setup (no `RUST_LOG`, no `debug.enable_tracing`) has no
/// file layer, so the structured event lives only in the in-memory error
/// collector, which is drained at graceful session finalization — a release
/// panic raw-exits before that. The human-panic report is always written, so
/// pin the semantic context there as a TOML comment (valid anywhere in the
/// document, needs no escaping beyond newline flattening).
fn append_context_to_crash_report(path: Option<&Path>, ctx: &PanicContext) {
    let Some(path) = path else {
        return;
    };
    let Ok(mut report) = OpenOptions::new().append(true).open(path) else {
        return;
    };
    let one_line = |value: &str| -> String { value.chars().map(|c| if c.is_control() { ' ' } else { c }).collect() };
    let _ = writeln!(
        report,
        "# vtcode.panic thread={} location={} count={} message={}",
        one_line(&ctx.thread),
        one_line(&ctx.location),
        ctx.count,
        one_line(&ctx.message)
    );
}

/// This hook is the only place that sees every panic, including those on worker
/// threads that never reach the CLI's `Result` error path.
fn log_panic(info: &panic::PanicHookInfo<'_>) -> PanicContext {
    let ctx = PanicContext::capture(info);
    record_panic(&ctx);
    ctx
}

/// Initialize the panic hook to restore terminal state on panic and provide better formatting.
pub fn init_panic_hook() {
    PANIC_HOOK_ONCE.call_once(|| {
        let original_hook = panic::take_hook();

        let better_panic_hook = BetterPanicSettings::new()
            .verbosity(BetterPanicVerbosity::Full)
            .most_recent_first(false)
            .lineno_suffix(true)
            .create_panic_handler();

        panic::set_hook(Box::new(move |panic_info| {
            let ctx = log_panic(panic_info);

            let is_debug = state::is_debug_mode();

            if state::is_tui_initialized() {
                let _ = restore_tui();
            }

            if cfg!(debug_assertions) && is_debug {
                if state::is_color_eyre_enabled() {
                    state::maybe_prepare_color_eyre_hooks();
                    if let Some(panic_hook) = state::COLOR_EYRE_PANIC_HOOK.get() {
                        vtcode_commons::diagnostics::stderr(format_args!("{}", panic_hook.panic_report(panic_info)));
                        return;
                    }
                }

                better_panic_hook(panic_info);
                return;
            }

            // Release mode: human-panic dump + user-facing message
            let metadata = state::app_metadata();
            let mut report_metadata = HumanPanicMetadata::new(metadata.name, metadata.version)
                .authors(format!("authored by {}", metadata.authors));

            if let Some(repository) = metadata.repository {
                report_metadata = report_metadata.support(format!("Open a support request at {repository}"));
            }

            let file_path = human_panic_dump(&report_metadata, panic_info);
            append_context_to_crash_report(file_path.as_deref(), &ctx);
            if let Err(error) = print_msg(file_path, &report_metadata) {
                vtcode_commons::diagnostics::stderr(format_args!(
                    "\nVT Code encountered a critical error and needs to shut down."
                ));
                vtcode_commons::diagnostics::stderr(format_args!("Failed to print crash report details: {error}"));
                original_hook(panic_info);
            }

            // A panic can arrive on any worker thread. Use the raw-exit helper
            // so libc `exit` handlers and thread-local destructors cannot run
            // while the rest of the process is still unwinding. The crash
            // report is already written to disk and printed above, so flush
            // the buffered streams explicitly before skipping their cleanup.
            // Second flush: `restore_tui()` and the rendering above can emit
            // tracing events that landed in the buffer after the hook's first
            // event. `_exit` skips `BufWriter` destructors, so flush them now.
            vtcode_commons::trace_flush::flush_trace_log();
            let _ = std::io::Write::flush(&mut std::io::stdout());
            let _ = std::io::Write::flush(&mut std::io::stderr());
            vtcode_commons::process_exit::emergency_exit(1);
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;
    use std::sync::{Arc, Mutex};
    use tracing::field::{Field, Visit};
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::Context;
    use tracing_subscriber::prelude::*;

    #[derive(Default, Debug)]
    struct CapturedEvent {
        target: String,
        thread: String,
        location: String,
        count: u64,
        message: String,
    }

    #[derive(Default)]
    struct CaptureVisitor(CapturedEvent);

    impl Visit for CaptureVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            match field.name() {
                "thread" => self.0.thread = value.to_string(),
                "message" => self.0.message = value.to_string(),
                _ => {}
            }
        }

        fn record_u64(&mut self, field: &Field, value: u64) {
            if field.name() == "panic_count" {
                self.0.count = value;
            }
        }

        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            match field.name() {
                "location" => self.0.location = format!("{value:?}"),
                "message" => self.0.message = format!("{value:?}"),
                _ => {}
            }
        }
    }

    struct CaptureLayer(Arc<Mutex<Option<CapturedEvent>>>);

    impl<S: tracing::Subscriber> Layer<S> for CaptureLayer {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            let mut visitor = CaptureVisitor::default();
            event.record(&mut visitor);
            let mut captured = visitor.0;
            captured.target = event.metadata().target().to_string();
            *self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(captured);
        }
    }

    #[test]
    fn panic_message_matches_standard_hook_downcasts() {
        assert_eq!(panic_message(&"literal"), "literal");
        assert_eq!(panic_message(&String::from("owned")), "owned");
        struct Opaque;
        assert_eq!(panic_message(&Opaque), "Box<dyn Any>");
    }

    #[test]
    fn location_label_includes_file_line_and_column() {
        let location = panic::Location::caller();
        let label = location_label(location);
        assert!(label.contains("hook.rs"), "{label}");
        assert!(label.contains(&location.line().to_string()), "{label}");
        assert!(label.contains(&location.column().to_string()), "{label}");
    }

    #[test]
    fn panic_event_carries_thread_location_count_and_message() {
        let captured = Arc::new(Mutex::new(None));
        let subscriber = tracing_subscriber::registry().with(CaptureLayer(captured.clone()));

        let ctx = PanicContext {
            thread: "vtcode-rt-worker".to_string(),
            message: "boom".to_string(),
            location: "/src/main.rs:42:9".to_string(),
            count: 7,
        };
        tracing::subscriber::with_default(subscriber, || record_panic(&ctx));

        let event = captured
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .expect("the panic event must reach the subscriber");
        assert_eq!(event.target, "vtcode.panic");
        assert_eq!(event.thread, "vtcode-rt-worker");
        assert_eq!(event.location, "/src/main.rs:42:9");
        assert_eq!(event.count, 7);
        assert!(event.message.contains("vtcode-rt-worker"), "{}", event.message);
        assert!(event.message.contains("/src/main.rs:42:9"), "{}", event.message);
        assert!(event.message.contains("boom"), "{}", event.message);
    }

    #[test]
    #[serial_test::serial]
    fn real_panic_hook_info_reaches_the_event() {
        let captured = Arc::new(Mutex::new(None));
        let subscriber = tracing_subscriber::registry().with(CaptureLayer(captured.clone()));

        let result = tracing::subscriber::with_default(subscriber, || {
            let original_hook = panic::take_hook();
            panic::set_hook(Box::new(|info| {
                let _ = log_panic(info);
            }));
            let result = panic::catch_unwind(|| panic!("integration boom"));
            panic::set_hook(original_hook);
            result
        });

        assert!(result.is_err(), "the closure must actually panic");
        let event = captured
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .expect("the installed hook must record a real panic");
        assert_eq!(event.target, "vtcode.panic");
        assert!(event.message.contains("integration boom"), "{}", event.message);
        assert!(event.location.contains("hook.rs"), "{}", event.location);
        assert!(!event.thread.is_empty(), "the hook must record the current thread name");
        assert!(event.count > 0, "the hook must increment the panic counter");
    }

    #[test]
    fn crash_report_gets_semantic_context_on_one_valid_toml_comment_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.toml");
        std::fs::write(&path, "name = 'vtcode'\nbacktrace = '...'\n").unwrap();
        let ctx = PanicContext {
            thread: "vtcode-rt-worker".to_string(),
            message: "line one\nline\u{7}two".to_string(),
            location: "/src/hook.rs:42:9".to_string(),
            count: 3,
        };

        append_context_to_crash_report(Some(&path), &ctx);
        let report = std::fs::read_to_string(&path).unwrap();

        assert!(report.starts_with("name = 'vtcode'\n"), "original report must be preserved");
        assert!(report.contains("# vtcode.panic thread=vtcode-rt-worker"), "{report}");
        assert!(report.contains("location=/src/hook.rs:42:9"), "{report}");
        assert!(report.contains("count=3"), "{report}");
        assert!(report.contains("message=line one line two"), "{report}");
        assert_eq!(report.lines().count(), 3, "flattened message must stay on one line");

        // Missing report (or no report at all) is a best-effort no-op.
        append_context_to_crash_report(Some(&dir.path().join("missing.toml")), &ctx);
        append_context_to_crash_report(None, &ctx);
    }
}
