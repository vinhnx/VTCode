//! Ephemeral computer-use screenshot reads for transcript previews.
//!
//! Resolves `computer-preview://` references against the runloop's
//! memory-only registry. Validation failures are errors (fail closed),
//! while unknown ids answer `expired` so the client renders the expired
//! state instead of retrying a read that can never succeed. Nothing here
//! touches the filesystem or the network: a miss means evicted or cleared.

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use vtcode_core::cli::input_hardening::validate_agent_safe_text;

use super::protocol;
use crate::agent::runloop::unified::turn::tool_outcomes::handlers::computer_use::preview as registry;

const MAX_REQUEST_ID_BYTES: usize = 128;

/// Headroom inside the frame budget for the JSON envelope around the body.
const FRAME_HEADROOM_BYTES: usize = 64 * 1024;

/// First downscale target: transcript columns render ~700px wide, so 960px
/// preserves displayed detail while collapsing typical screenshots well
/// under budget. Halved toward the floor while it still does not fit.
const FIRST_SCALED_DIMENSION: u32 = 960;
const MIN_SCALED_DIMENSION: u32 = 240;

pub(super) async fn read(params: protocol::ComputerPreviewReadParams) -> Result<protocol::ComputerPreviewReadResult> {
    validate_params(&params)?;
    let Some(item_id) = protocol::parse_preview_uri(&params.uri) else {
        bail!("invalid computer preview URI");
    };
    let Some(shot) = registry::take_preview(item_id) else {
        return Ok(expired(&params));
    };
    // Pixel work stays off the async runtime; the registry hands out an Arc
    // so the blocking task borrows without copying the bytes first.
    let fitted = tokio::task::spawn_blocking(move || fit_frame(shot.as_png()))
        .await
        .context("preview fit task failed")??;
    Ok(protocol::ComputerPreviewReadResult {
        uri: params.uri.clone(),
        request_id: params.request_id.clone(),
        mime_type: Some("image/png".to_string()),
        total_bytes: u64::try_from(fitted.bytes.len()).unwrap_or(u64::MAX),
        downscaled: fitted.downscaled,
        data: Some(STANDARD.encode(&fitted.bytes)),
        expired: false,
    })
}

/// Bytes plus whether they were scaled to fit.
struct FittedPreview {
    bytes: Vec<u8>,
    downscaled: bool,
}

/// Fit retained PNG bytes into the frame budget, downscaling only when the
/// original cannot fit. Pure over bytes so tests pin it without the registry.
fn fit_frame(png: &[u8]) -> Result<FittedPreview> {
    fit_frame_with_budget(png, protocol::MAX_FRAME_BYTES.saturating_sub(FRAME_HEADROOM_BYTES))
}

fn fit_frame_with_budget(png: &[u8], budget: usize) -> Result<FittedPreview> {
    if base64_len(png.len()) <= budget {
        return Ok(FittedPreview { bytes: png.to_vec(), downscaled: false });
    }
    let mut dimension = FIRST_SCALED_DIMENSION;
    loop {
        if let Some(scaled) = scaled_png(png, dimension)
            && base64_len(scaled.len()) <= budget
        {
            return Ok(FittedPreview { bytes: scaled, downscaled: true });
        }
        if dimension <= MIN_SCALED_DIMENSION {
            break;
        }
        dimension = (dimension / 2).max(MIN_SCALED_DIMENSION);
    }
    bail!("preview cannot fit the frame budget")
}

/// Base64 output length for `len` raw bytes (no padding surprises: the
/// encoder emits canonical padded output, whose length is exactly this).
fn base64_len(len: usize) -> usize {
    len.div_ceil(3) * 4
}

/// Decode, fit within a `dimension` box preserving aspect, and re-encode.
/// `None` on undecodable input; the caller fails the read closed.
fn scaled_png(png: &[u8], dimension: u32) -> Option<Vec<u8>> {
    let image = image::load_from_memory(png).ok()?;
    let resized = image.resize(dimension, dimension, image::imageops::FilterType::Triangle);
    let mut out = Vec::new();
    resized
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

fn expired(params: &protocol::ComputerPreviewReadParams) -> protocol::ComputerPreviewReadResult {
    protocol::ComputerPreviewReadResult {
        uri: params.uri.clone(),
        request_id: params.request_id.clone(),
        mime_type: None,
        data: None,
        total_bytes: 0,
        downscaled: false,
        expired: true,
    }
}

fn validate_params(params: &protocol::ComputerPreviewReadParams) -> Result<()> {
    if params.request_id.is_empty() || params.request_id.len() > MAX_REQUEST_ID_BYTES {
        bail!("invalid computer preview read request identity");
    }
    if protocol::parse_preview_uri(&params.uri).is_none() {
        bail!("invalid computer preview URI");
    }
    validate_agent_safe_text("preview uri", &params.uri)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::{MAX_REQUEST_ID_BYTES, STANDARD, read, registry, validate_params};
    use vtcode_computer_use::Screenshot;

    fn params(uri: &str) -> super::protocol::ComputerPreviewReadParams {
        super::protocol::ComputerPreviewReadParams {
            uri: uri.to_string(),
            request_id: "req-1".to_string(),
        }
    }

    fn probe_png() -> Screenshot {
        Screenshot::png([b"\x89PNG\r\n\x1a\n".as_slice(), b"probe"].concat()).unwrap()
    }

    #[test]
    fn validate_params_accepts_preview_uri_and_rejects_the_rest() {
        validate_params(&params("computer-preview://row-1")).expect("valid preview uri");
        for uri in ["", "   ", "ui://row-1", "file:///x.png", "computer-preview://bad\nid"] {
            validate_params(&params(uri)).expect_err("invalid uri");
        }
        let mut bad_id = params("computer-preview://row-1");
        bad_id.request_id.clear();
        validate_params(&bad_id).expect_err("empty request id");
        bad_id.request_id = "r".repeat(MAX_REQUEST_ID_BYTES + 1);
        validate_params(&bad_id).expect_err("oversized request id");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn read_serves_retained_bytes_and_expires_misses() {
        registry::clear_previews();
        // Unknown id: explicit expiry, no body, no error.
        let miss = read(params("computer-preview://gone")).await.unwrap();
        assert!(miss.expired);
        assert!(miss.data.is_none());
        assert_eq!(miss.request_id, "req-1");
        // Retained id: base64 body round-trips to the stored PNG, unscaled.
        registry::retain_preview("row-1", probe_png());
        let hit = read(params("computer-preview://row-1")).await.unwrap();
        assert!(!hit.expired);
        assert!(!hit.downscaled);
        assert_eq!(hit.mime_type.as_deref(), Some("image/png"));
        let body = STANDARD.decode(hit.data.unwrap()).unwrap();
        assert_eq!(body, [b"\x89PNG\r\n\x1a\n".as_slice(), b"probe"].concat());
        assert_eq!(hit.total_bytes, u64::try_from(body.len()).unwrap());
        registry::clear_previews();
    }

    fn real_png(width: u32, height: u32) -> Vec<u8> {
        let image =
            image::RgbaImage::from_fn(width, height, |x, y| image::Rgba([(x % 251) as u8, (y % 251) as u8, 128, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn fit_frame_passes_small_shots_through_untouched() {
        let png = real_png(64, 64);
        let fitted = super::fit_frame(&png).unwrap();
        assert!(!fitted.downscaled);
        assert_eq!(fitted.bytes, png);
    }

    #[test]
    fn fit_frame_downscales_only_until_it_fits() {
        let png = real_png(1200, 900);
        // Budget sized so the original cannot fit but one 960px step can:
        // exactly one scale step, byte-identical to the direct downscale.
        let scaled_once = super::scaled_png(&png, 960).unwrap();
        let budget = super::base64_len(scaled_once.len());
        assert!(super::base64_len(png.len()) > budget);
        let fitted = super::fit_frame_with_budget(&png, budget).unwrap();
        assert!(fitted.downscaled);
        assert_eq!(fitted.bytes, scaled_once);
        // Asymmetric: an impossible budget and corrupt bytes both fail closed.
        assert!(super::fit_frame_with_budget(&png, 10).is_err());
        assert!(super::fit_frame_with_budget(b"\x89PNG\r\n\x1a\njunk", 10).is_err());
    }

    #[test]
    fn base64_len_matches_padded_output() {
        for (len, encoded) in [(0, 0), (1, 4), (2, 4), (3, 4), (4, 8), (64, 88)] {
            assert_eq!(super::base64_len(len), encoded, "len {len}");
        }
    }
}
