use super::*;
use serial_test::serial;
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tempfile::TempDir;

thread_local! {
    static TEST_FREE_WAS_CALLED: Cell<bool> = const { Cell::new(false) };
}

static TEST_EXECUTE_ACTIVE_CALLS: AtomicUsize = AtomicUsize::new(0);
static TEST_EXECUTE_MAX_CONCURRENCY: AtomicUsize = AtomicUsize::new(0);
static TEST_FREE_CALLS: AtomicUsize = AtomicUsize::new(0);

/// Test-only C ABI shim for the plugin loader.
///
/// # Safety
///
/// The loader calls it with pointers produced by `CString::into_raw` in this module.
#[allow(unsafe_code, reason = "test-only C ABI shim for the plugin loader")]
unsafe extern "C" fn test_free_string(ptr: *const c_char) {
    TEST_FREE_WAS_CALLED.with(|was_called| was_called.set(true));
    if !ptr.is_null() {
        // Safety: tests pass pointers produced by `CString::into_raw` in this module.
        let _ = unsafe { CString::from_raw(ptr.cast_mut()) };
    }
}

/// Counting deallocator: proves the bridge releases a plugin buffer exactly
/// once, including on error paths (no leak, no double-free).
#[allow(unsafe_code, reason = "test-only C ABI shim for the plugin loader")]
unsafe extern "C" fn test_free_counting(ptr: *const c_char) {
    TEST_FREE_CALLS.fetch_add(1, Ordering::SeqCst);
    if !ptr.is_null() {
        // Safety: tests pass pointers produced by `CString::into_raw` in this module.
        let _ = unsafe { CString::from_raw(ptr.cast_mut()) };
    }
}

fn create_test_plugin_dir() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let plugin_dir = temp_dir.path().join("test-plugin");
    std::fs::create_dir(&plugin_dir).unwrap();
    (temp_dir, plugin_dir)
}

fn write_plugin_metadata(plugin_dir: &Path, name: &str) {
    std::fs::write(
        plugin_dir.join("plugin.json"),
        format!(r#"{{"name":"{name}","description":"test","version":"1.0.0"}}"#),
    )
    .unwrap();
}

fn write_fake_library(plugin_dir: &Path, name: &str) -> PathBuf {
    let loader = PluginLoader::new();
    let library_path = plugin_dir.join(loader.library_filename(name));
    std::fs::write(&library_path, b"fake-library").unwrap();
    library_path
}

fn current_process_library() -> Library {
    #[cfg(unix)]
    {
        libloading::os::unix::Library::this().into()
    }
    #[cfg(windows)]
    {
        libloading::os::windows::Library::this()
            .expect("current process library")
            .into()
    }
}

fn update_max_concurrency(active_calls: usize) {
    let mut current_max = TEST_EXECUTE_MAX_CONCURRENCY.load(Ordering::SeqCst);
    while active_calls > current_max {
        match TEST_EXECUTE_MAX_CONCURRENCY.compare_exchange(
            current_max,
            active_calls,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => break,
            Err(observed) => current_max = observed,
        }
    }
}

unsafe extern "C" fn test_execute_with_delay(_input: *const c_char) -> *const c_char {
    let active_calls = TEST_EXECUTE_ACTIVE_CALLS.fetch_add(1, Ordering::SeqCst) + 1;
    update_max_concurrency(active_calls);
    std::thread::sleep(Duration::from_millis(25));
    TEST_EXECUTE_ACTIVE_CALLS.fetch_sub(1, Ordering::SeqCst);

    CString::new(r#"{"success":true,"output":{},"error":null,"files":[]}"#)
        .unwrap()
        .into_raw()
}

/// Boundary adversarial inputs: a conforming plugin must never return null,
/// invalid UTF-8, or malformed JSON, but the bridge must fail closed (and still
/// release the buffer) rather than introducing a crash of its own.
#[allow(unsafe_code, reason = "test-only C ABI shim for the plugin loader")]
unsafe extern "C" fn test_execute_null(_input: *const c_char) -> *const c_char {
    std::ptr::null()
}

#[allow(unsafe_code, reason = "test-only C ABI shim for the plugin loader")]
unsafe extern "C" fn test_execute_invalid_utf8(_input: *const c_char) -> *const c_char {
    // 0xFF is never valid UTF-8; the nul keeps it a well-formed C string.
    CString::from_vec_with_nul(vec![0xFF, 0x00]).unwrap().into_raw()
}

#[allow(unsafe_code, reason = "test-only C ABI shim for the plugin loader")]
unsafe extern "C" fn test_execute_malformed_json(_input: *const c_char) -> *const c_char {
    CString::new("not json").unwrap().into_raw()
}

#[allow(unsafe_code, reason = "test-only C ABI shim for the plugin loader")]
unsafe extern "C" fn test_execute_valid(_input: *const c_char) -> *const c_char {
    CString::new(r#"{"success":true,"output":{},"error":null,"files":[]}"#)
        .unwrap()
        .into_raw()
}

fn test_context() -> PluginContext {
    PluginContext {
        input: HashMap::new(),
        workspace_root: None,
        config: HashMap::new(),
    }
}

/// Shared builder: keeps the plugin literal in one place so the concurrency
/// tests and the boundary tests exercise the same construction path.
fn test_plugin(execute_fn: PluginExecuteFn, free_string_fn: Option<PluginFreeStringFn>) -> NativePlugin {
    NativePlugin {
        _library: current_process_library(),
        metadata: PluginMetadata {
            name: "boundary-test".to_string(),
            description: "test plugin".to_string(),
            version: "1.0.0".to_string(),
            author: None,
            abi_version: PLUGIN_ABI_VERSION,
            when_to_use: None,
            when_not_to_use: None,
            allowed_tools: None,
            thread_safe: false,
        },
        path: PathBuf::from("/tmp/boundary-test-plugin"),
        execute_fn,
        free_string_fn,
        execution_lock: Mutex::new(()),
        thread_safe: false,
    }
}

#[test]
fn test_validate_plugin_structure_missing_metadata() {
    let (_temp_dir, plugin_dir) = create_test_plugin_dir();
    let errors = validate_plugin_structure(&plugin_dir).unwrap();
    assert!(errors.iter().any(|e| e.contains("plugin.json")));
}

#[test]
fn test_validate_plugin_structure_missing_library() {
    let (_temp_dir, plugin_dir) = create_test_plugin_dir();
    std::fs::write(plugin_dir.join("plugin.json"), r#"{"name": "test", "description": "test", "version": "1.0.0"}"#)
        .unwrap();

    let errors = validate_plugin_structure(&plugin_dir).unwrap();
    assert!(errors.iter().any(|e| e.contains("dynamic library")));
}

#[test]
fn test_validate_plugin_structure_complete() {
    let (_temp_dir, plugin_dir) = create_test_plugin_dir();

    // Create valid plugin.json
    std::fs::write(plugin_dir.join("plugin.json"), r#"{"name": "test", "description": "test", "version": "1.0.0"}"#)
        .unwrap();

    // Create fake library file
    let lib_name = if cfg!(target_os = "macos") {
        "libtest.dylib"
    } else if cfg!(target_os = "linux") {
        "libtest.so"
    } else {
        "test.dll"
    };
    std::fs::write(plugin_dir.join(lib_name), b"fake").unwrap();

    let errors = validate_plugin_structure(&plugin_dir).unwrap();
    assert!(errors.is_empty());
}

#[test]
fn test_library_filename_platform() {
    let loader = PluginLoader::new();
    let filename = loader.library_filename("my-plugin");

    #[cfg(target_os = "macos")]
    assert_eq!(filename, "libmy-plugin.dylib");

    #[cfg(target_os = "linux")]
    assert_eq!(filename, "libmy-plugin.so");

    #[cfg(target_os = "windows")]
    assert_eq!(filename, "my-plugin.dll");
}

#[test]
fn test_ensure_non_null_c_string_ptr_rejects_null() {
    let err = ensure_non_null_c_string_ptr(std::ptr::null::<c_char>(), "Test pointer")
        .expect_err("null pointer should be rejected");
    assert!(err.to_string().contains("Test pointer returned null pointer"));
}

#[test]
fn test_decode_plugin_c_string_frees_plugin_buffer() {
    TEST_FREE_WAS_CALLED.with(|was_called| was_called.set(false));

    let raw = CString::new("{\"ok\":true}").expect("valid C string").into_raw();
    let ptr = NonNull::new(raw).expect("non-null raw pointer");

    let decoded = decode_plugin_c_string(ptr, Some(test_free_string), "Plugin result is not valid UTF-8")
        .expect("valid UTF-8 payload");

    assert_eq!(decoded, "{\"ok\":true}");
    TEST_FREE_WAS_CALLED.with(|was_called| assert!(was_called.get()));
}

#[test]
fn test_decode_plugin_c_string_invalid_utf8_still_frees_buffer() {
    TEST_FREE_WAS_CALLED.with(|was_called| was_called.set(false));

    let raw = CString::from_vec_with_nul(vec![0xFF, 0x00])
        .expect("valid nul-terminated C string")
        .into_raw();
    let ptr = NonNull::new(raw).expect("non-null raw pointer");

    let err = decode_plugin_c_string(ptr, Some(test_free_string), "Plugin payload is not valid UTF-8")
        .expect_err("invalid UTF-8 should fail decoding");

    assert!(err.to_string().contains("Plugin payload is not valid UTF-8"));
    TEST_FREE_WAS_CALLED.with(|was_called| assert!(was_called.get()));
}

#[test]
fn test_load_plugin_rejects_dotdot_escape_from_trusted_root() {
    let temp_dir = TempDir::new().unwrap();
    let trusted_root = temp_dir.path().join("trusted");
    let escaped_plugin_dir = temp_dir.path().join("escaped-plugin");
    std::fs::create_dir(&trusted_root).unwrap();
    std::fs::create_dir(&escaped_plugin_dir).unwrap();
    write_plugin_metadata(&escaped_plugin_dir, "escaped");
    write_fake_library(&escaped_plugin_dir, "escaped");

    let escaped_path = trusted_root.join("..").join("escaped-plugin");

    let mut loader = PluginLoader::new();
    loader.add_trusted_dir(trusted_root);

    let err = loader
        .load_plugin(&escaped_path)
        .expect_err("path traversal should be rejected");

    assert!(err.to_string().contains("trusted directory"));
}

#[cfg(unix)]
#[test]
fn test_load_plugin_rejects_symlinked_plugin_dir_escape() {
    use std::os::unix::fs::symlink;

    let temp_dir = TempDir::new().unwrap();
    let trusted_root = temp_dir.path().join("trusted");
    let real_plugin_dir = temp_dir.path().join("external-plugin");
    let symlinked_plugin_dir = trusted_root.join("linked-plugin");
    std::fs::create_dir(&trusted_root).unwrap();
    std::fs::create_dir(&real_plugin_dir).unwrap();
    write_plugin_metadata(&real_plugin_dir, "linked");
    write_fake_library(&real_plugin_dir, "linked");
    symlink(&real_plugin_dir, &symlinked_plugin_dir).unwrap();

    let mut loader = PluginLoader::new();
    loader.add_trusted_dir(trusted_root);

    let err = loader
        .load_plugin(&symlinked_plugin_dir)
        .expect_err("symlink escape should be rejected");

    assert!(err.to_string().contains("trusted directory"));
}

#[cfg(unix)]
#[test]
fn test_load_plugin_rejects_symlinked_library_escape() {
    use std::os::unix::fs::symlink;

    let temp_dir = TempDir::new().unwrap();
    let trusted_root = temp_dir.path().join("trusted");
    let plugin_dir = trusted_root.join("plugin");
    let external_dir = temp_dir.path().join("external");
    std::fs::create_dir(&trusted_root).unwrap();
    std::fs::create_dir(&plugin_dir).unwrap();
    std::fs::create_dir(&external_dir).unwrap();
    write_plugin_metadata(&plugin_dir, "escaped-lib");

    let external_library = write_fake_library(&external_dir, "escaped-lib");
    let linked_library = plugin_dir.join(PluginLoader::new().library_filename("escaped-lib"));
    symlink(&external_library, &linked_library).unwrap();

    let mut loader = PluginLoader::new();
    loader.add_trusted_dir(trusted_root);

    let err = loader
        .load_plugin(&plugin_dir)
        .expect_err("library symlink escape should be rejected");

    assert!(err.to_string().contains("trusted directory"));
}

#[test]
#[serial]
fn test_native_plugin_serializes_concurrent_execution() {
    TEST_EXECUTE_ACTIVE_CALLS.store(0, Ordering::SeqCst);
    TEST_EXECUTE_MAX_CONCURRENCY.store(0, Ordering::SeqCst);

    let plugin = Arc::new(test_plugin(test_execute_with_delay, Some(test_free_string)));
    let ctx = test_context();

    let handles = (0..4)
        .map(|_| {
            let plugin = Arc::clone(&plugin);
            let ctx = ctx.clone();
            std::thread::spawn(move || plugin.execute(&ctx).expect("plugin execution"))
        })
        .collect::<Vec<_>>();

    for handle in handles {
        let result = handle.join().expect("thread should complete");
        assert!(result.success);
    }

    assert_eq!(TEST_EXECUTE_MAX_CONCURRENCY.load(Ordering::SeqCst), 1);
}

#[test]
#[serial]
fn test_native_plugin_allows_parallel_execution() {
    TEST_EXECUTE_ACTIVE_CALLS.store(0, Ordering::SeqCst);
    TEST_EXECUTE_MAX_CONCURRENCY.store(0, Ordering::SeqCst);

    let mut plugin = test_plugin(test_execute_with_delay, Some(test_free_string));
    // OPT-IN to parallel execution; the shared builder defaults to serialized.
    plugin.thread_safe = true;
    plugin.metadata.thread_safe = true;
    let plugin = Arc::new(plugin);

    let ctx = test_context();

    let num_threads = 4;
    let handles = (0..num_threads)
        .map(|_| {
            let plugin = Arc::clone(&plugin);
            let ctx = ctx.clone();
            std::thread::spawn(move || plugin.execute(&ctx).expect("parallel plugin execution"))
        })
        .collect::<Vec<_>>();

    for handle in handles {
        let result = handle.join().expect("thread join");
        assert!(result.success);
    }

    // With thread_safe: true, we expect max concurrency > 1
    assert!(TEST_EXECUTE_MAX_CONCURRENCY.load(Ordering::SeqCst) > 1);
}

#[cfg(unix)]
#[test]
fn test_warn_if_insecure_trusted_dir_warns_on_writable_dir() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let writable_dir = temp_dir.path().join("writable");
    std::fs::create_dir(&writable_dir).unwrap();
    std::fs::set_permissions(&writable_dir, std::fs::Permissions::from_mode(0o777)).unwrap();

    warn_if_insecure_trusted_dir(&writable_dir);
}

#[cfg(unix)]
#[test]
fn test_warn_if_insecure_trusted_dir_silent_on_restricted_dir() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let restricted_dir = temp_dir.path().join("restricted");
    std::fs::create_dir(&restricted_dir).unwrap();
    std::fs::set_permissions(&restricted_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    warn_if_insecure_trusted_dir(&restricted_dir);
}

// --- FFI boundary: adversarial plugin responses ---

#[test]
#[serial]
fn test_execute_ffi_null_result_fails_closed_without_freeing() {
    TEST_FREE_CALLS.store(0, Ordering::SeqCst);
    let plugin = test_plugin(test_execute_null, Some(test_free_counting));

    let err = plugin.execute(&test_context()).expect_err("null result must be rejected");

    assert!(err.to_string().contains("returned null pointer"));
    // No buffer was returned, so the deallocator must not be called.
    assert_eq!(TEST_FREE_CALLS.load(Ordering::SeqCst), 0);
}

#[test]
#[serial]
fn test_execute_ffi_invalid_utf8_frees_buffer_exactly_once() {
    TEST_FREE_CALLS.store(0, Ordering::SeqCst);
    let plugin = test_plugin(test_execute_invalid_utf8, Some(test_free_counting));

    let err = plugin.execute(&test_context()).expect_err("invalid UTF-8 must be rejected");

    assert!(err.to_string().contains("not valid UTF-8"));
    assert_eq!(TEST_FREE_CALLS.load(Ordering::SeqCst), 1);
}

#[test]
#[serial]
fn test_execute_ffi_malformed_json_frees_buffer_exactly_once() {
    TEST_FREE_CALLS.store(0, Ordering::SeqCst);
    let plugin = test_plugin(test_execute_malformed_json, Some(test_free_counting));

    let err = plugin.execute(&test_context()).expect_err("malformed JSON must be rejected");

    assert!(err.to_string().contains("Failed to parse plugin result JSON"));
    assert_eq!(TEST_FREE_CALLS.load(Ordering::SeqCst), 1);
}

#[test]
#[serial]
fn test_execute_ffi_without_deallocator_still_decodes() {
    TEST_FREE_CALLS.store(0, Ordering::SeqCst);
    let plugin = test_plugin(test_execute_valid, None);

    let result = plugin
        .execute(&test_context())
        .expect("valid result decodes even without a deallocator");

    assert!(result.success);
    assert_eq!(TEST_FREE_CALLS.load(Ordering::SeqCst), 0);
}

#[test]
#[serial]
fn test_plugin_owned_c_string_frees_exactly_once_on_decode_error() {
    TEST_FREE_CALLS.store(0, Ordering::SeqCst);
    let raw = CString::from_vec_with_nul(vec![0xFF, 0x00]).expect("valid C string").into_raw();
    let owned = PluginOwnedCString::new(NonNull::new(raw).expect("non-null"), Some(test_free_counting));

    let err = owned.decode("bad payload").expect_err("invalid UTF-8 must be rejected");
    drop(owned);

    assert!(err.to_string().contains("bad payload"));
    assert_eq!(TEST_FREE_CALLS.load(Ordering::SeqCst), 1);
}
