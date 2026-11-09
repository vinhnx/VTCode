//! Sandboxed Pele execution: the capped QuickJS runtime behind every
//! headless diagram render.
//!
//! One sandbox loads the vendored bundle once and renders many sources,
//! each under its own wall-clock budget. The runtime is `!Send` by design:
//! owners keep it on one thread (the desktop render worker, or the CLI's
//! main thread for a single render).

use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rquickjs::{Context, Object, Runtime};

/// Heap cap for one diagram render. Typical renders allocate kilobytes; the
/// cap only binds pathological inputs, which then fail instead of growing.
const JS_HEAP_LIMIT_BYTES: usize = 64 * 1024 * 1024;

/// Stack cap for one diagram render. Pele's generated parsers recurse over
/// diagram nesting, so deeply nested hostile input hits this instead of the
/// host stack.
const JS_STACK_LIMIT_BYTES: usize = 1024 * 1024;

/// Budget for evaluating the vendored bundle at sandbox construction. The
/// bundle is trusted first-party-vendored code, so this only guards against
/// a corrupt copy hanging the caller at startup.
const BUNDLE_LOAD_BUDGET: Duration = Duration::from_secs(60);

/// Options pinned for every headless render. Links and images stay off: the
/// SVG is inert by construction, whether it ships standalone (CLI) or is
/// rasterized (desktop).
const RENDER_EXPRESSION: &str =
    "pele.render(vtcode_diagram_source, {links:false, images:false, idPrefix:'vtcode-diagram'})";

/// Failure from [`DiagramSandbox::render`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// The source exceeds [`crate::MAX_DIAGRAM_SOURCE_CHARS`] UTF-16 units;
    /// rejected before any JS executes.
    TooLarge,
    /// The sandbox or Pele itself failed: bundle load, JS exception, or an
    /// exhausted time budget. Carries the JS-side message for inert display.
    Js(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "diagram source exceeds the size limit"),
            Self::Js(message) => write!(f, "diagram render failed: {message}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Headless Pele output: standalone SVG plus its intrinsic size.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedSvg {
    /// Raw SVG; `var(--pele-*)` references carry light-theme fallbacks, so
    /// the document renders as-is in a browser.
    pub svg: String,
    /// Intrinsic width in SVG units, always finite and positive.
    pub width: f64,
    /// Intrinsic height in SVG units, always finite and positive.
    pub height: f64,
}

/// Pinned QuickJS runtime evaluating the vendored Pele bundle.
///
/// The bundle loads once at construction; every render reuses the runtime
/// with a fresh time budget. Neither the runtime nor its context may move
/// threads.
pub struct DiagramSandbox {
    context: Context,
    deadline: Arc<Mutex<Instant>>,
    // Declared last so it drops last: the context must be freed before the
    // runtime it was created from.
    _runtime: Runtime,
}

impl DiagramSandbox {
    /// Load the bundle into a fresh capped runtime. Callers pass
    /// [`crate::PELE_BUNDLE_JS`]; the parameter keeps tests hermetic.
    pub fn new(bundle_js: &str) -> Result<Self, RenderError> {
        let runtime = Runtime::new().map_err(|error| RenderError::Js(format!("{error:?}")))?;
        runtime.set_memory_limit(JS_HEAP_LIMIT_BYTES);
        runtime.set_max_stack_size(JS_STACK_LIMIT_BYTES);
        let deadline = Arc::new(Mutex::new(Instant::now() + BUNDLE_LOAD_BUDGET));
        let handler_deadline = Arc::clone(&deadline);
        // A poisoned mutex means a previous holder panicked mid-update; the
        // only update is an infallible timestamp store, so poisoning is
        // unreachable — fail closed by treating it as expired anyway.
        runtime.set_interrupt_handler(Some(Box::new(move || {
            handler_deadline.lock().map(|due| Instant::now() > *due).unwrap_or(true)
        })));
        let context = Context::full(&runtime).map_err(|error| RenderError::Js(format!("{error:?}")))?;
        context
            .with(|ctx| {
                ctx.eval::<(), _>(bundle_js.as_bytes())
                    .map_err(|error| format_js_error(&ctx, error))
                    .and_then(|()| {
                        ctx.eval::<(), _>(
                            "if (typeof pele === 'undefined' || typeof pele.render !== 'function') \
                         throw new Error('diagram bundle does not define pele.render');",
                        )
                        .map_err(|error| format_js_error(&ctx, error))
                    })
            })
            .map_err(RenderError::Js)?;
        Ok(Self { context, deadline, _runtime: runtime })
    }

    /// Render one diagram source to SVG, enforcing the size gate before any
    /// JS executes and the wall-clock `budget` during execution.
    pub fn render(&self, source: &str, budget: Duration) -> Result<RenderedSvg, RenderError> {
        if source.encode_utf16().count() > crate::MAX_DIAGRAM_SOURCE_CHARS {
            return Err(RenderError::TooLarge);
        }
        *self.deadline.lock().unwrap_or_else(|poison| poison.into_inner()) = Instant::now() + budget;
        self.context.with(|ctx| {
            ctx.globals()
                .set("vtcode_diagram_source", source)
                .map_err(|error| RenderError::Js(format!("{error:?}")))?;
            let result: Object = ctx
                .eval(RENDER_EXPRESSION)
                .map_err(|error| RenderError::Js(format_js_error(&ctx, error)))?;
            let svg: String = result.get("svg").map_err(|error| RenderError::Js(format!("{error:?}")))?;
            let width: f64 = result.get("width").map_err(|error| RenderError::Js(format!("{error:?}")))?;
            let height: f64 = result.get("height").map_err(|error| RenderError::Js(format!("{error:?}")))?;
            if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
                return Err(RenderError::Js(format!("invalid diagram size {width}x{height}")));
            }
            Ok(RenderedSvg { svg, width, height })
        })
    }
}

fn format_js_error(ctx: &rquickjs::Ctx<'_>, error: rquickjs::Error) -> String {
    if matches!(error, rquickjs::Error::Exception) {
        let caught: rquickjs::Value = ctx.catch();
        if let Some(object) = caught.as_object() {
            let name: String = object.get("name").unwrap_or_else(|_| String::from("Error"));
            let message: String = object.get("message").unwrap_or_else(|_| format!("{caught:?}"));
            return format!("{name}: {message}");
        }
        return format!("exception: {caught:?}");
    }
    format!("{error:?}")
}

#[cfg(test)]
mod tests;
