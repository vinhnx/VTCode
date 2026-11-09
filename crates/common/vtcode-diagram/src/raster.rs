//! SVG-to-PNG rasterization for headless output.
//!
//! [`DiagramSandbox::render_png`] parses Pele's SVG with `usvg`, paints it
//! with `resvg` onto a `tiny-skia` pixmap at [`RASTER_SCALE`], and encodes
//! PNG. Fonts resolve from the OS font database, so text matches what a
//! browser would show; the output has no background fill, exactly like the
//! SVG it rasterizes.

use std::time::Duration;

use crate::{DiagramSandbox, RenderError, RenderedSvg};

/// Supersample factor for raster output. 2x keeps diagram text crisp on
/// high-density displays and matches the desktop inline previews.
pub const RASTER_SCALE: f32 = 2.0;

/// Longest file-raster edge in pixels. File output is not transcript
/// embedded, so the budget is roomier than the inline previews': a typical
/// tall workflow diagram rasterizes near 1200x4400 at 2x.
pub const MAX_FILE_RASTER_EDGE_PX: f64 = 8192.0;

/// Total file-raster pixel budget (4096 squared, 64MB RGBA). Bounds memory
/// for hostile input that fits inside the edge cap on both sides.
pub const MAX_FILE_RASTER_PIXELS: u64 = 16_777_216;

/// PNG bytes plus their pixel dimensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPng {
    /// Encoded PNG file bytes.
    pub bytes: Vec<u8>,
    /// Pixmap width in pixels.
    pub width_px: u32,
    /// Pixmap height in pixels.
    pub height_px: u32,
}

impl DiagramSandbox {
    /// Render one diagram source straight to PNG, enforcing the size gate
    /// before any JS executes, the wall-clock `budget` during execution,
    /// and the raster footprint gate before allocating the pixmap.
    pub fn render_png(&self, source: &str, budget: Duration) -> Result<RenderedPng, RenderError> {
        rasterize_png(&self.render(source, budget)?)
    }
}

fn rasterize_png(rendered: &RenderedSvg) -> Result<RenderedPng, RenderError> {
    let width_f = rendered.width * f64::from(RASTER_SCALE);
    let height_f = rendered.height * f64::from(RASTER_SCALE);
    let (width_px, height_px) = raster_dims_checked(width_f, height_f)?;
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(&rendered.svg, &options)
        .map_err(|error| RenderError::Js(format!("svg parse failed: {error}")))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width_px, height_px)
        .ok_or_else(|| RenderError::Js(format!("invalid raster size {width_px}x{height_px}")))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(RASTER_SCALE, RASTER_SCALE), &mut pixmap.as_mut());
    let bytes = pixmap
        .encode_png()
        .map_err(|error| RenderError::Js(format!("png encode failed: {error}")))?;
    Ok(RenderedPng { bytes, width_px, height_px })
}

/// Pixel dimensions for scaled SVG geometry, enforcing the edge gate
/// before conversion and the total-pixel budget after. Both bounds fail
/// with [`RenderError::OutputTooLarge`].
fn raster_dims_checked(width: f64, height: f64) -> Result<(u32, u32), RenderError> {
    if width > MAX_FILE_RASTER_EDGE_PX || height > MAX_FILE_RASTER_EDGE_PX {
        return Err(RenderError::OutputTooLarge);
    }
    let (width_px, height_px) = (raster_dim(width), raster_dim(height));
    if u64::from(width_px) * u64::from(height_px) > MAX_FILE_RASTER_PIXELS {
        return Err(RenderError::OutputTooLarge);
    }
    Ok((width_px, height_px))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "edge gate bounds the input; clamp makes it total"
)]
fn raster_dim(edge: f64) -> u32 {
    edge.ceil().clamp(1.0, MAX_FILE_RASTER_EDGE_PX) as u32
}

#[cfg(test)]
mod tests;
