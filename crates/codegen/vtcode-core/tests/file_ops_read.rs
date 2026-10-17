#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use assert_fs::TempDir;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::json;
use std::io::Write;
use std::sync::Arc;
use vtcode_core::tools::file_ops::FileOpsTool;
use vtcode_core::tools::grep_file::GrepSearchManager;

#[tokio::test]
async fn read_file_returns_base64_for_images() {
    let workspace = TempDir::new().expect("temp workspace");
    let image_path = workspace.path().join("sample.png");

    // Minimal PNG header to mimic binary content
    let mut file = std::fs::File::create(&image_path).expect("create png");
    file.write_all(&[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
    ])
    .expect("write png header");

    let grep_manager = Arc::new(GrepSearchManager::new(workspace.path().to_path_buf()));
    let file_tool = FileOpsTool::new(workspace.path().to_path_buf(), grep_manager);

    let args = json!({
        "path": image_path.to_string_lossy(),
    });

    let value = file_tool.read_file(args).await.expect("read file");
    assert_eq!(value["success"].as_bool(), Some(true));
    assert_eq!(value["binary"].as_bool(), Some(true));
    assert_eq!(value["encoding"].as_str(), Some("base64"));
    // Fields surfaced top-level are not duplicated into metadata.data.
    assert_eq!(value["content_kind"].as_str(), Some("image"));
    assert_eq!(value["mime_type"].as_str(), Some("image/png"));
    assert!(value["metadata"]["data"].get("content_kind").is_none());
    assert!(value["metadata"]["data"].get("mime_type").is_none());

    let base64_payload = value["content"].as_str().expect("base64 payload");
    let decoded = BASE64.decode(base64_payload).expect("decode base64 image");
    assert_eq!(decoded.len(), 16);
}

#[tokio::test]
async fn read_file_reports_text_metadata() {
    let workspace = TempDir::new().expect("temp workspace");
    let text_path = workspace.path().join("note.txt");
    std::fs::write(&text_path, b"hello world\n").expect("write text file");

    let grep_manager = Arc::new(GrepSearchManager::new(workspace.path().to_path_buf()));
    let file_tool = FileOpsTool::new(workspace.path().to_path_buf(), grep_manager);

    let args = json!({
        "path": text_path.to_string_lossy(),
    });

    let value = file_tool.read_file(args).await.expect("read file");
    assert_eq!(value["success"].as_bool(), Some(true));
    assert_eq!(value["content_kind"].as_str(), Some("text"));
    assert_eq!(value["encoding"].as_str(), Some("utf8"));
    // Fields surfaced top-level are not duplicated into metadata.data.
    assert!(value["metadata"]["data"].get("content_kind").is_none());
    assert!(value["metadata"]["data"].get("encoding").is_none());
}

#[tokio::test]
async fn read_file_reads_svg_as_text_instead_of_an_image_part() {
    // SVG is XML text: providers refuse it as `input_image` (400), so the
    // read tool must return it as text rather than base64 image content.
    let workspace = TempDir::new().expect("temp workspace");
    let svg_path = workspace.path().join("logo.svg");
    std::fs::write(&svg_path, b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>\n").expect("write svg");

    let grep_manager = Arc::new(GrepSearchManager::new(workspace.path().to_path_buf()));
    let file_tool = FileOpsTool::new(workspace.path().to_path_buf(), grep_manager);

    let args = json!({
        "path": svg_path.to_string_lossy(),
    });

    let value = file_tool.read_file(args).await.expect("read file");
    assert_eq!(value["success"].as_bool(), Some(true));
    assert_eq!(value["binary"].as_bool(), None);
    let content = value["content"].as_str().expect("text content");
    assert!(content.contains("<svg"), "svg source should be readable as text, got: {content}");
}

#[tokio::test]
async fn read_file_rejects_bmp_with_an_actionable_error() {
    // BMP is binary but no vision API accepts it: fail loudly instead of
    // returning mojibake text or a payload providers reject with 400.
    let workspace = TempDir::new().expect("temp workspace");
    let bmp_path = workspace.path().join("bitmap.bmp");
    std::fs::write(&bmp_path, b"BM binary-image-bytes").expect("write bmp");

    let grep_manager = Arc::new(GrepSearchManager::new(workspace.path().to_path_buf()));
    let file_tool = FileOpsTool::new(workspace.path().to_path_buf(), grep_manager);

    let args = json!({
        "path": bmp_path.to_string_lossy(),
    });

    let outcome = file_tool.read_file(args).await;
    let message = match outcome {
        Ok(value) => {
            assert_eq!(value["success"].as_bool(), Some(false), "bmp read must not succeed");
            value.to_string()
        }
        Err(error) => error.to_string(),
    };
    assert!(message.contains("Unsupported image format"), "unexpected outcome: {message}");
}
