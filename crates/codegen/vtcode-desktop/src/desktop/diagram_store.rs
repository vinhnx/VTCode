//! Inline diagram store: render cache, worker thread, image hook.
//!
//! [`super::diagram_inline`] owns the stateless half (sandboxed Pele
//! execution, CSS-var inlining, content addressing); this module owns the
//! stateful half. Closed mermaid fences resolve through Kit's `image_source`
//! hook to an App-global FIFO cache, with misses filled by one worker thread
//! and the transcript repainted on completion. Every failure degrades to the
//! code fence plus the browser preview, never a blank.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::component::theme::ThemeColor;

use super::diagram_inline;

/// Rendered images retained: count backstop plus the byte budget below.
const IMAGE_ENTRY_CAP: usize = 32;
/// Worst-case raster retention. Typical diagrams cost 1-3MB at 2x; the
/// budget only binds sessions full of poster-sized diagrams.
const IMAGE_BYTES_CAP: usize = 64 * 1024 * 1024;

/// Worker inbox depth. Renders take milliseconds, so a full inbox means a
/// pathological burst; the hook then stays pending and retries next frame.
const WORKER_QUEUE: usize = 8;

/// Wall-clock budget per headless render on the worker.
const RENDER_BUDGET: Duration = Duration::from_secs(10);

/// Supersample factor for inline rasters. Matches the native renderer's own
/// smoothing so inline diagrams look identical to other scaled art.
const RASTER_SCALE: f64 = 2.0;
/// Longest raster edge. Covers every typical diagram; larger ones keep the
/// browser preview instead of a 100-megapixel inline bitmap.
const MAX_RASTER_DIM_PX: f64 = 2048.0;

/// Cache key: diagram content plus the exact theme it rendered under, so a
/// theme switch re-renders instead of showing stale colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct DiagramKey {
    /// Content hash of the diagram source.
    pub source: diagram_inline::SourceHash,
    /// Content hash of the resolved `--pele-*` stylesheet.
    pub theme: diagram_inline::SourceHash,
}

/// One cached inline render. No `Debug`: the raster type does not implement
/// it, and cache entries never need formatting.
#[derive(Clone)]
pub(super) enum ImageState {
    /// Fill task running; the image hook returns pending.
    Pending,
    /// Rasterized and ready to display.
    Ready(CachedDiagram),
    /// Unrenderable; carries the display message. The code fence and the
    /// browser preview stay available, so failures degrade, never blank.
    Failed(String),
}

impl ImageState {
    /// Retained raster bytes, for the store's memory budget.
    fn bytes(&self) -> usize {
        match self {
            Self::Ready(cached) => cached.bytes,
            Self::Pending | Self::Failed(_) => 0,
        }
    }
}

/// Rasterized diagram plus its retained size for budget accounting.
#[derive(Clone)]
pub(super) struct CachedDiagram {
    /// Device-ready image at [`RASTER_SCALE`].
    pub image: Arc<gpui_kit::RenderImage>,
    /// Raster bytes (`width × height × 4`).
    pub bytes: usize,
}

/// App-global inline diagram cache plus the render worker handle. Interior
/// mutability (rather than `&mut App` updates) because image hooks only get
/// shared access during paint.
pub(super) struct DiagramStore {
    inner: Mutex<StoreInner>,
}

struct StoreInner {
    images: diagram_inline::FifoMap<DiagramKey, ImageState>,
    image_bytes: usize,
    worker: Option<mpsc::SyncSender<WorkerJob>>,
}

impl gpui_kit::Global for DiagramStore {}

impl StoreInner {
    fn insert_pending(&mut self, key: DiagramKey) {
        let evicted = self.images.insert(key, ImageState::Pending);
        self.account_evicted(evicted);
    }

    fn insert_ready(&mut self, key: DiagramKey, cached: CachedDiagram) {
        self.image_bytes = self.image_bytes.saturating_add(cached.bytes);
        let evicted = self.images.insert(key, ImageState::Ready(cached));
        self.account_evicted(evicted);
        while self.image_bytes > IMAGE_BYTES_CAP {
            let Some((_, state)) = self.images.pop_oldest() else {
                break;
            };
            self.image_bytes = self.image_bytes.saturating_sub(state.bytes());
        }
    }

    fn insert_failed(&mut self, key: DiagramKey, message: String) {
        let evicted = self.images.insert(key, ImageState::Failed(message));
        self.account_evicted(evicted);
    }

    fn account_evicted(&mut self, evicted: Vec<(DiagramKey, ImageState)>) {
        for (_, state) in evicted {
            self.image_bytes = self.image_bytes.saturating_sub(state.bytes());
        }
    }
}

/// One render request for the worker thread.
struct WorkerJob {
    source: String,
    theme: HashMap<String, String>,
    reply: oneshot::Sender<WorkerOutput>,
}

/// Worker result: rasterized image or a display message.
type WorkerOutput = Result<CachedDiagram, String>;

/// Start the render worker. Returns false when the thread cannot spawn;
/// images then fail closed like any other blocked image.
fn spawn_diagram_worker(svg_renderer: gpui_kit::SvgRenderer, jobs: mpsc::Receiver<WorkerJob>) -> bool {
    thread::Builder::new()
        .name("diagram-render".to_string())
        .spawn(move || {
            let sandbox = diagram_inline::DiagramSandbox::new(super::diagram::PELE_BUNDLE_JS);
            while let Ok(job) = jobs.recv() {
                let output = match &sandbox {
                    Ok(sandbox) => render_diagram(sandbox, &svg_renderer, &job.source, &job.theme),
                    Err(error) => Err(error.to_string()),
                };
                let _gone = job.reply.send(output);
            }
        })
        .is_ok()
}

/// Full blocking render: sandboxed JS, theme inlining, parse, raster.
/// Runs only on the worker thread; every failure carries a display message.
fn render_diagram(
    sandbox: &diagram_inline::DiagramSandbox,
    svg_renderer: &gpui_kit::SvgRenderer,
    source: &str,
    theme: &HashMap<String, String>,
) -> WorkerOutput {
    let rendered = sandbox.render(source, RENDER_BUDGET).map_err(|error| error.to_string())?;
    // Gate on Pele's own intrinsic size: inlining only swaps paint values,
    // so geometry (and therefore the raster footprint) is already fixed.
    // Oversized diagrams keep the browser preview.
    let raster_width = rendered.width * RASTER_SCALE;
    let raster_height = rendered.height * RASTER_SCALE;
    if raster_width > MAX_RASTER_DIM_PX || raster_height > MAX_RASTER_DIM_PX {
        return Err(rust_i18n::t!("app.markdown.diagram_too_large").to_string());
    }
    // Unresolved references keep their literal `var()` text, which the SVG
    // parser drops in favor of default paint: degraded colors in exotic
    // cases, never a failed image.
    let (inlined, _unresolved) = diagram_inline::inline_css_vars(&rendered.svg, theme);
    let parsed = svg_renderer
        .parse_svg(inlined.as_bytes())
        .map_err(|error| format!("{error:?}"))?;
    let image = svg_renderer
        .render_parsed(&parsed, 1.0f32)
        .map_err(|error| format!("{error:?}"))?;
    Ok(CachedDiagram {
        image,
        bytes: raster_bytes(raster_width, raster_height),
    })
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=2048"
)]
fn raster_bytes(width: f64, height: f64) -> usize {
    let pixels =
        width.ceil().clamp(0.0, MAX_RASTER_DIM_PX) as usize * height.ceil().clamp(0.0, MAX_RASTER_DIM_PX) as usize;
    pixels.saturating_mul(4)
}

/// Lazily install the store (and its worker) on first diagram sighting.
/// False when the worker thread cannot spawn: images fail closed.
fn ensure_diagram_store(cx: &mut gpui_kit::App) -> bool {
    if cx.try_global::<DiagramStore>().is_some() {
        return true;
    }
    let (jobs_tx, jobs_rx) = mpsc::sync_channel(WORKER_QUEUE);
    if !spawn_diagram_worker(cx.svg_renderer(), jobs_rx) {
        return false;
    }
    cx.set_global(DiagramStore {
        inner: Mutex::new(StoreInner {
            images: diagram_inline::FifoMap::new(IMAGE_ENTRY_CAP),
            image_bytes: 0,
            worker: Some(jobs_tx),
        }),
    });
    true
}

/// Cache key for one diagram under one theme. The theme fingerprint is the
/// resolved stylesheet itself: fixed order, exact bytes, so any theme
/// change re-renders instead of showing stale colors.
fn diagram_key_for(hash: &diagram_inline::SourceHash, colors: &ThemeColor) -> DiagramKey {
    DiagramKey {
        source: *hash,
        theme: diagram_inline::source_hash(&super::diagram::pele_css_vars(colors)),
    }
}

/// Resolve one inline diagram image for Kit's image element. Synchronous and
/// non-blocking: cache hits return immediately, first sightings enqueue a
/// background fill and return pending (`None`), which Kit renders as its
/// loading state until the completion repaint.
pub(super) fn diagram_image_data(
    hash: &diagram_inline::SourceHash,
    cx: &mut gpui_kit::App,
) -> Option<Result<Arc<gpui_kit::RenderImage>, gpui_kit::ImageCacheError>> {
    use gpui_kit::component::theme::ActiveTheme as _;

    if !ensure_diagram_store(cx) {
        return Some(Err(blocked_image_error()));
    }
    let key = diagram_key_for(hash, &cx.theme().colors);
    let store = cx.global::<DiagramStore>();
    let Ok(mut inner) = store.inner.lock() else {
        return None; // Unreachable (guarded sections cannot panic); retry next frame.
    };
    // Clone out of the guard: the fallthrough below needs `inner` mutably.
    match inner.images.get(&key).cloned() {
        Some(ImageState::Ready(cached)) => return Some(Ok(Arc::clone(&cached.image))),
        Some(ImageState::Failed(message)) => return Some(Err(asset_error(message))),
        Some(ImageState::Pending) => return None,
        None => {}
    }
    let Some(source) = diagram_inline::diagram_source(&key.source) else {
        inner.insert_failed(key, blocked_message());
        return Some(Err(blocked_image_error()));
    };
    let Some(worker) = inner.worker.clone() else {
        inner.insert_failed(key, blocked_message());
        return Some(Err(blocked_image_error()));
    };
    let theme = diagram_inline::inline_theme(&cx.theme().colors);
    let (reply_tx, reply_rx) = oneshot::channel();
    let job = WorkerJob { source, theme, reply: reply_tx };
    if worker.try_send(job).is_err() {
        return None; // Saturated: stay uncached, retry next frame (natural backpressure).
    }
    inner.insert_pending(key);
    drop(inner);
    cx.spawn(async move |async_cx| {
        let output = reply_rx.await.ok().unwrap_or_else(|| Err(blocked_message()));
        async_cx.update_global::<DiagramStore, _>(|store, cx| {
            if let Ok(mut inner) = store.inner.lock() {
                match output {
                    Ok(cached) => inner.insert_ready(key, cached),
                    Err(message) => inner.insert_failed(key, message),
                }
            }
            cx.refresh_windows();
        });
    })
    .detach();
    None
}

fn blocked_message() -> String {
    rust_i18n::t!("app.markdown.images_blocked").to_string()
}

fn blocked_image_error() -> gpui_kit::ImageCacheError {
    asset_error(blocked_message())
}

fn asset_error(message: String) -> gpui_kit::ImageCacheError {
    gpui_kit::ImageCacheError::Asset(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::diagram::PELE_BUNDLE_JS;
    use crate::desktop::diagram_inline::{DiagramSandbox, FifoMap, SourceHash, inline_theme, register_diagram_source};

    fn sandbox() -> DiagramSandbox {
        DiagramSandbox::new(PELE_BUNDLE_JS).expect("real bundle loads")
    }

    /// 120-node chain: narrow source (~4KB, fast) but ~18k units wide, so it
    /// trips the raster gate without the timeout test's render cost.
    fn wide_chain() -> String {
        let mut source = String::from("flowchart LR\n");
        for index in 0..120 {
            use std::fmt::Write;
            writeln!(source, "  N{index}[node {index}] --> N{}[node {}]", index + 1, index + 1)
                .expect("fixture builds");
        }
        source
    }

    fn test_image() -> Arc<gpui_kit::RenderImage> {
        let buffer = image::ImageBuffer::from_raw(2, 2, vec![0u8; 16]).expect("test pixels");
        Arc::new(gpui_kit::RenderImage::new(vec![image::Frame::new(buffer)]))
    }

    fn test_key(tag: u8) -> DiagramKey {
        DiagramKey { source: [tag; 32], theme: [0; 32] }
    }

    #[test]
    fn store_byte_budget_evicts_oldest_ready_first() {
        let mut inner = StoreInner {
            images: FifoMap::new(IMAGE_ENTRY_CAP),
            image_bytes: 0,
            worker: None,
        };
        let first = CachedDiagram { image: test_image(), bytes: IMAGE_BYTES_CAP };
        inner.insert_ready(test_key(1), first);
        assert_eq!(inner.image_bytes, IMAGE_BYTES_CAP);
        // Exactly at the budget: nothing evicted yet.
        assert!(matches!(inner.images.get(&test_key(1)), Some(ImageState::Ready(_))));
        let second = CachedDiagram { image: test_image(), bytes: 8 };
        inner.insert_ready(test_key(2), second);
        assert!(inner.images.get(&test_key(1)).is_none(), "budget evicted the oldest");
        assert!(matches!(inner.images.get(&test_key(2)), Some(ImageState::Ready(_))));
        assert_eq!(inner.image_bytes, 8);
    }

    #[test]
    fn render_diagram_rasterizes_with_system_fonts() {
        let sandbox = sandbox();
        let renderer = gpui_kit::SvgRenderer::new(Arc::new(()));
        let colors = ThemeColor::default();
        let theme = inline_theme(&colors);
        let cached = render_diagram(&sandbox, &renderer, "flowchart TD\n  A[Mermaid] --> B[Pele]", &theme)
            .expect("valid source rasterizes");
        assert!(cached.bytes > 0, "nonempty raster");
        assert_eq!(cached.image.frame_count(), 1, "single frame");
    }

    #[test]
    fn render_diagram_rejects_poster_sized_output() {
        let sandbox = sandbox();
        let renderer = gpui_kit::SvgRenderer::new(Arc::new(()));
        let theme = inline_theme(&ThemeColor::default());
        let error = match render_diagram(&sandbox, &renderer, &wide_chain(), &theme) {
            Ok(_) => panic!("poster-sized output must fail"),
            Err(error) => error,
        };
        assert_eq!(error, rust_i18n::t!("app.markdown.diagram_too_large").to_string());
    }

    // Hook coverage stays synchronous by design: the test scheduler panics
    // on cross-thread wakes, so a real worker round-trip cannot run under
    // `#[gpui_kit::test]`. These tests pin every hook state with a
    // pre-seeded store (no fill task spawns); the worker half is covered by
    // the `render_diagram_*` tests above, and the spawn/reply/refresh glue
    // mirrors the image-loading precedent in `view/file_preview.rs`.

    fn seed_image_state(cx: &mut gpui_kit::TestAppContext, key: DiagramKey, state: ImageState) {
        cx.update(|cx| {
            assert!(ensure_diagram_store(cx), "store installs");
            let store = cx.global::<DiagramStore>();
            let mut inner = store.inner.lock().expect("test lock");
            match state {
                ImageState::Pending => inner.insert_pending(key),
                ImageState::Ready(cached) => inner.insert_ready(key, cached),
                ImageState::Failed(message) => inner.insert_failed(key, message),
            }
        });
    }

    fn theme_key_for(cx: &mut gpui_kit::TestAppContext, hash: &SourceHash) -> DiagramKey {
        cx.update(|cx| {
            use gpui_kit::component::theme::ActiveTheme as _;
            diagram_key_for(hash, &cx.theme().colors)
        })
    }

    #[gpui_kit::test]
    fn image_hook_serves_preseeded_states_without_spawning(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        // Never registered: no worker round-trip, immediate failure.
        let unknown = [0x42; 32];
        let failed = cx.update(|cx| diagram_image_data(&unknown, cx));
        assert!(matches!(failed, Some(Err(_))), "unknown hash fails");
        // Ready: the identical cached image, no fill.
        let hash = register_diagram_source("flowchart TD\n  A[Mermaid] --> B[Pele]");
        let key = theme_key_for(cx, &hash);
        let seeded = CachedDiagram { image: test_image(), bytes: 16 };
        seed_image_state(cx, key, ImageState::Ready(seeded));
        let hit = cx.update(|cx| diagram_image_data(&hash, cx));
        let hit = match hit {
            Some(Ok(image)) => image,
            _ => panic!("ready state serves"),
        };
        let again = cx.update(|cx| diagram_image_data(&hash, cx));
        let again = match again {
            Some(Ok(image)) => image,
            _ => panic!("ready state stable"),
        };
        assert!(Arc::ptr_eq(&hit, &again), "same cached image");
        // Failed and Pending pass through untouched.
        let failed_key = theme_key_for(cx, &register_diagram_source("flowchart TD\n  Q --> W"));
        seed_image_state(cx, failed_key, ImageState::Failed("nope".to_string()));
        let failed_hash = failed_key.source;
        let failed = cx.update(|cx| diagram_image_data(&failed_hash, cx));
        assert!(matches!(failed, Some(Err(_))), "failed state errors");
        let pending_key = theme_key_for(cx, &register_diagram_source("flowchart TD\n  E --> R"));
        seed_image_state(cx, pending_key, ImageState::Pending);
        let pending_hash = pending_key.source;
        let pending = cx.update(|cx| diagram_image_data(&pending_hash, cx));
        assert!(pending.is_none(), "pending state stays pending");
    }
}
