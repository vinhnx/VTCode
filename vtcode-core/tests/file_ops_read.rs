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
    assert_eq!(
        value["metadata"]
            .get("content_kind")
            .and_then(|v| v.as_str()),
        Some("image")
    );
    assert_eq!(
        value["metadata"].get("mime_type").and_then(|v| v.as_str()),
        Some("image/png")
    );

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
    assert_eq!(
        value["metadata"]
            .get("content_kind")
            .and_then(|v| v.as_str()),
        Some("text")
    );
    assert_eq!(
        value["metadata"].get("encoding").and_then(|v| v.as_str()),
        Some("utf8")
    );
}
