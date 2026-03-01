#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::runtime::{Builder, Runtime};
use vtcode_core::tools::validation::unified_path::validate_and_resolve_path;

const MAX_INPUT_BYTES: usize = 1024;

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("failed to initialize tokio runtime for fuzzing")
    })
}

fn bounded_input(data: &[u8]) -> String {
    let slice = if data.len() > MAX_INPUT_BYTES {
        &data[..MAX_INPUT_BYTES]
    } else {
        data
    };
    String::from_utf8_lossy(slice).into_owned()
}

fn setup_workspace(root: &Path) {
    let _ = std::fs::create_dir_all(root.join("nested"));
    let _ = std::fs::write(root.join("nested/seed.txt"), b"seed");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = symlink("/", root.join("escape_root"));
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::symlink_dir;
        let _ = symlink_dir(r"C:\", root.join("escape_root"));
    }
}

fn canonical_or_fallback(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fuzz_target!(|data: &[u8]| {
    let Ok(workspace) = tempfile::tempdir() else {
        return;
    };
    let workspace_root = workspace.path().to_path_buf();
    setup_workspace(&workspace_root);

    let candidate = bounded_input(data);
    let path_input = if candidate.trim().is_empty() {
        ".".to_string()
    } else {
        candidate
    };

    let result = runtime().block_on(validate_and_resolve_path(&workspace_root, &path_input));
    if let Ok(resolved) = result {
        let canonical_root = canonical_or_fallback(&workspace_root);
        let canonical_resolved = canonical_or_fallback(&resolved);
        assert!(
            canonical_resolved.starts_with(&canonical_root),
            "resolved path escaped workspace: {} -> {}",
            path_input,
            canonical_resolved.display()
        );
    }
});
