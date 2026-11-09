use std::time::Duration;

use super::*;
use crate::{MAX_DIAGRAM_SOURCE_CHARS, PELE_BUNDLE_JS};

fn sandbox() -> DiagramSandbox {
    DiagramSandbox::new(PELE_BUNDLE_JS).expect("real bundle loads")
}

/// PNG signature: 137 80 78 71 13 10 26 10.
const PNG_MAGIC: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Independent oracle for the encoded file: parse the IHDR dimensions by
/// hand (width/height as big-endian u32 at bytes 16..24) instead of trusting
/// the returned struct or any image helper.
fn ihdr_dimensions(bytes: &[u8]) -> (u32, u32) {
    assert!(bytes.starts_with(&PNG_MAGIC), "png signature present");
    let width = u32::from_be_bytes(bytes.get(16..20).expect("ihdr width").try_into().expect("4 bytes"));
    let height = u32::from_be_bytes(bytes.get(20..24).expect("ihdr height").try_into().expect("4 bytes"));
    (width, height)
}

#[test]
fn png_has_magic_bytes_and_double_geometry() {
    let source = "flowchart TD\n  A[Mermaid] --> B[Pele]";
    let sandbox = sandbox();
    let svg = sandbox.render(source, Duration::from_secs(10)).expect("svg renders");
    let png = sandbox.render_png(source, Duration::from_secs(10)).expect("png renders");
    let (file_width, file_height) = ihdr_dimensions(&png.bytes);
    assert_eq!((file_width, file_height), (png.width_px, png.height_px), "ihdr matches struct");
    for (px, units) in [(png.width_px, svg.width), (png.height_px, svg.height)] {
        let gap = (f64::from(px) - units * f64::from(RASTER_SCALE)).abs();
        assert!(gap < 1.0, "2x geometry within a pixel: {gap}");
    }
    assert!(png.bytes.len() > 100, "nontrivial file: {} bytes", png.bytes.len());
}

#[test]
fn png_renders_hostile_labels_without_breakout() {
    let png = sandbox()
        .render_png(
            "flowchart TD\n  A[\"</script><script>alert(1)</script>\"] --> B[\"<!--x\"]",
            Duration::from_secs(10),
        )
        .expect("hostile source rasterizes");
    let (width, height) = ihdr_dimensions(&png.bytes);
    assert!(width > 0 && height > 0, "nonzero size: {width}x{height}");
}

/// 120-node chain: narrow source (~4KB, fast) but ~18k units wide, so 2x
/// rasterization trips the output gate without the timeout test's cost.
fn wide_chain() -> String {
    let mut source = String::from("flowchart LR\n");
    for index in 0..120 {
        use std::fmt::Write;
        writeln!(source, "  N{index}[node {index}] --> N{}[node {}]", index + 1, index + 1).expect("fixture builds");
    }
    source
}

#[test]
fn png_rejects_poster_sized_output_before_allocating() {
    let error = match sandbox().render_png(&wide_chain(), Duration::from_secs(30)) {
        Ok(_) => panic!("poster-sized output must fail"),
        Err(error) => error,
    };
    assert_eq!(error, RenderError::OutputTooLarge);
    assert!(error.to_string().contains("raster size limit"), "honest message: {error}");
}

#[test]
fn raster_budget_checks_both_sides_of_both_bounds() {
    assert_eq!(raster_dims_checked(8192.0, 2048.0), Ok((8192, 2048)));
    assert_eq!(raster_dims_checked(4096.0, 4096.0), Ok((4096, 4096)));
    assert_eq!(raster_dims_checked(0.1, 0.1), Ok((1, 1)));
    assert_eq!(raster_dims_checked(8192.1, 1.0), Err(RenderError::OutputTooLarge));
    assert_eq!(raster_dims_checked(1.0, 8192.1), Err(RenderError::OutputTooLarge));
    assert_eq!(raster_dims_checked(4096.0, 4097.0), Err(RenderError::OutputTooLarge));
    assert_eq!(raster_dims_checked(8192.0, 8192.0), Err(RenderError::OutputTooLarge));
}

#[test]
fn png_propagates_source_failures_before_raster() {
    let sandbox = sandbox();
    assert!(matches!(sandbox.render_png("Hello", Duration::from_secs(10)), Err(RenderError::Js(_))));
    let too_large = "x".repeat(MAX_DIAGRAM_SOURCE_CHARS + 1);
    assert_eq!(sandbox.render_png(&too_large, Duration::from_secs(10)), Err(RenderError::TooLarge));
}
