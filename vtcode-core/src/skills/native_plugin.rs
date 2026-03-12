//! Native Plugin System for VT Code Skills
//!
//! This module provides support for loading native code plugins as skills using libloading.
//! Native plugins offer high-performance, pre-compiled skill logic that can be discovered
//! and loaded dynamically at runtime.
//!
//! # Safety
//!
//! Loading native code plugins requires careful security considerations:
//! - Plugins are loaded from canonicalized trusted locations only
//! - Plugin signatures can be verified (future enhancement)
//! - Plugin execution is sandboxed where possible
//! - VT Code serializes plugin FFI calls for ABI v1
//! - All plugin operations go through VT Code's tool system
//!
//! # Plugin Structure
//!
//! A native plugin skill consists of:
//! - `plugin.json` - Metadata (name, description, version, author)
//! - `lib<name>.dylib` (macOS) or `lib<name>.so` (Linux) or `<name>.dll` (Windows)
//! - Optional: `README.md`, `scripts/`, `templates/`
//!
//! # Plugin ABI
//!
//! Plugins must export the following C-compatible symbols:
//! - `vtcode_plugin_version()` - Returns ABI version
//! - `vtcode_plugin_metadata()` - Returns plugin metadata JSON
//! - `vtcode_plugin_execute()` - Main execution entry point
//!
//! # Example
//!
//! ```rust,no_run
//! use vtcode_core::skills::native_plugin::{NativePlugin, PluginLoader};
//!
//! let mut loader = PluginLoader::new();
//! let plugin = loader.load_plugin("/path/to/plugin").unwrap();
//! let result = plugin.execute(&input).unwrap();
//! ```

use anyhow::{Context, Result, anyhow};
use hashbrown::HashMap;
use libloading::{Library, Symbol};
use serde::{Deserialize, Serialize};
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Mutex;
use tracing::{debug, info, warn};

/// Current plugin ABI version
/// Increment this when breaking changes are made to the plugin interface
pub const PLUGIN_ABI_VERSION: u32 = 1;

type PluginVersionFn = unsafe extern "C" fn() -> u32;
type PluginMetadataFn = unsafe extern "C" fn() -> *const libc::c_char;
type PluginExecuteFn = unsafe extern "C" fn(*const libc::c_char) -> *const libc::c_char;
type PluginFreeStringFn = unsafe extern "C" fn(*const libc::c_char);

/// Plugin execution context passed to plugin functions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginContext {
    /// Input data for the plugin
    pub input: HashMap<String, serde_json::Value>,
    /// Workspace root path
    pub workspace_root: Option<String>,
    /// Plugin configuration
    pub config: HashMap<String, serde_json::Value>,
}

/// Plugin execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginResult {
    /// Success flag
    pub success: bool,
    /// Output data
    pub output: HashMap<String, serde_json::Value>,
    /// Error message if failed
    pub error: Option<String>,
    /// Optional file references created by the plugin
    pub files: Vec<String>,
}

/// Plugin metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Plugin name (identifier)
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Plugin version (semver)
    pub version: String,
    /// Plugin author
    pub author: Option<String>,
    /// Plugin ABI version
    pub abi_version: u32,
    /// When to use this plugin
    pub when_to_use: Option<String>,
    /// When NOT to use this plugin
    pub when_not_to_use: Option<String>,
    /// Allowed tools for this plugin
    pub allowed_tools: Option<Vec<String>>,
}

/// C-compatible plugin metadata for FFI
#[repr(C)]
#[derive(Debug, Clone)]
pub struct PluginMetadataFFI {
    /// Pointer to JSON metadata string
    pub json_ptr: *const libc::c_char,
}

/// C-compatible plugin result for FFI
#[repr(C)]
pub struct PluginResultFFI {
    /// Pointer to JSON result string
    pub json_ptr: *const libc::c_char,
}

/// Native plugin trait for type-erased plugin operations
pub trait NativePluginTrait: Send + Sync + std::fmt::Debug {
    /// Get plugin metadata
    fn metadata(&self) -> &PluginMetadata;

    /// Get plugin path
    fn path(&self) -> &Path;

    /// Execute the plugin with given context
    fn execute(&self, ctx: &PluginContext) -> Result<PluginResult>;
}

/// A loaded native plugin
pub struct NativePlugin {
    /// Plugin library handle (kept alive to prevent unloading)
    _library: Library,
    /// Plugin metadata
    metadata: PluginMetadata,
    /// Path to the plugin
    path: PathBuf,
    /// Plugin execute function pointer
    execute_fn: PluginExecuteFn,
    /// Optional plugin-owned deallocator for returned strings
    free_string_fn: Option<PluginFreeStringFn>,
    /// Serialize ABI v1 plugin calls until per-plugin concurrency is explicit.
    execution_lock: Mutex<()>,
}

fn ensure_non_null_c_string_ptr(
    ptr: *const libc::c_char,
    context: &'static str,
) -> Result<NonNull<libc::c_char>> {
    NonNull::new(ptr.cast_mut()).ok_or_else(|| anyhow!("{context} returned null pointer"))
}

fn decode_plugin_c_string(
    ptr: NonNull<libc::c_char>,
    free_string_fn: Option<PluginFreeStringFn>,
    utf8_error_context: &'static str,
) -> Result<String> {
    let raw_ptr = ptr.as_ptr() as *const libc::c_char;
    // SAFETY:
    // 1. `raw_ptr` is guaranteed to be non-null (validated by `ensure_non_null_c_string_ptr`).
    // 2. We assume the plugin-returned pointer is a valid nul-terminated C string per the plugin ABI.
    // 3. The reference created by `CStr::from_ptr` is only used to copy the data into a Rust `String`.
    //    Since we own the only reference during this brief window and copy-then-release,
    //    we avoid Undefined Behavior related to mutable aliasing that "Unsafe Rust is not C" warns about.
    let decoded = unsafe { CStr::from_ptr(raw_ptr) }
        .to_str()
        .context(utf8_error_context)
        .map(str::to_owned);

    if let Some(free_fn) = free_string_fn {
        // SAFETY: The pointer originated from the same plugin instance that provided `free_fn`.
        // We call it only after we've finished reading the data into our own `String`.
        unsafe { free_fn(raw_ptr) };
    }

    decoded
}

impl std::fmt::Debug for NativePlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativePlugin")
            .field("metadata", &self.metadata)
            .field("path", &self.path)
            .finish()
    }
}

// SAFETY: `NativePlugin` owns the library handle for its full lifetime, and all
// state exposed through this type is either immutable or accessed under
// `execution_lock`. Moving the wrapper to another thread does not invalidate the
// loaded library or any function pointers.
unsafe impl Send for NativePlugin {}
// SAFETY: shared access is serialized through `execution_lock`, so VT Code never
// issues overlapping ABI v1 plugin calls through the same `NativePlugin`.
unsafe impl Sync for NativePlugin {}

fn canonicalize_existing_path(path: &Path, label: &str) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("Failed to resolve {label} '{}'", path.display()))
}

fn normalize_trusted_dir(path: PathBuf) -> PathBuf {
    canonicalize_existing_path(&path, "trusted plugin directory").unwrap_or_else(|_| {
        if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(&path))
                .unwrap_or(path)
        }
    })
}

impl NativePlugin {
    /// Create a new native plugin from a loaded library
    pub fn new(library: Library, path: PathBuf) -> Result<Self> {
        // Verify ABI version
        // SAFETY: The symbol name and signature are defined by the plugin ABI.
        // We trust the library at `lib_path` (already validated as trusted).
        let version_fn: Symbol<PluginVersionFn> = unsafe {
            library
                .get(b"vtcode_plugin_version\0")
                .context("Failed to load vtcode_plugin_version symbol")?
        };

        // SAFETY: The function pointer was loaded from a validated ABI symbol.
        let abi_version = unsafe { version_fn() };
        if abi_version != PLUGIN_ABI_VERSION {
            return Err(anyhow!(
                "Plugin ABI version mismatch: expected {}, got {}",
                PLUGIN_ABI_VERSION,
                abi_version
            ));
        }

        // Optional cleanup function for plugin-owned strings.
        // SAFETY: Symbol name and signature follow the plugin ABI.
        let free_string_fn = unsafe {
            library
                .get::<PluginFreeStringFn>(b"vtcode_plugin_free_string\0")
                .map(|symbol| *symbol)
                .ok()
        };

        // Load metadata
        // SAFETY: Symbol name and signature are defined by the plugin ABI.
        let metadata_fn: Symbol<PluginMetadataFn> = unsafe {
            library
                .get(b"vtcode_plugin_metadata\0")
                .context("Failed to load vtcode_plugin_metadata symbol")?
        };

        // SAFETY: Function pointer loaded from the validated ABI symbol.
        let metadata_ptr =
            ensure_non_null_c_string_ptr(unsafe { metadata_fn() }, "Plugin metadata function")?;
        let metadata_json = decode_plugin_c_string(
            metadata_ptr,
            free_string_fn,
            "Plugin metadata is not valid UTF-8",
        )?;

        let metadata: PluginMetadata =
            serde_json::from_str(&metadata_json).context("Failed to parse plugin metadata JSON")?;

        // Load execute function
        // SAFETY: Symbol name and signature are defined by the plugin ABI.
        let execute_fn: Symbol<PluginExecuteFn> = unsafe {
            library
                .get(b"vtcode_plugin_execute\0")
                .context("Failed to load vtcode_plugin_execute symbol")?
        };

        let execute_fn_ptr = *execute_fn;

        Ok(Self {
            _library: library,
            metadata,
            path,
            execute_fn: execute_fn_ptr,
            free_string_fn,
            execution_lock: Mutex::new(()),
        })
    }

    /// Execute the plugin with the given context
    pub fn execute(&self, ctx: &PluginContext) -> Result<PluginResult> {
        let input_json =
            serde_json::to_string(ctx).context("Failed to serialize plugin context")?;

        let input_cstr =
            CString::new(input_json).context("Failed to create C string from input JSON")?;

        let _execution_guard = self
            .execution_lock
            .lock()
            .map_err(|_| anyhow!("native plugin execution lock poisoned"))?;

        // SAFETY:
        // 1. The `input_cstr` pointer is valid for the duration of this call.
        // 2. The `execute_fn` obeys the plugin ABI and expects a nul-terminated string.
        // 3. VT Code holds `execution_lock`, so this plugin instance will not observe
        //    overlapping ABI v1 calls from multiple threads.
        let result_ptr = ensure_non_null_c_string_ptr(
            unsafe { (self.execute_fn)(input_cstr.as_ptr()) },
            "Plugin execute function",
        )?;
        let result_json = decode_plugin_c_string(
            result_ptr,
            self.free_string_fn,
            "Plugin result is not valid UTF-8",
        )?;

        let result: PluginResult =
            serde_json::from_str(&result_json).context("Failed to parse plugin result JSON")?;

        Ok(result)
    }
}

impl NativePluginTrait for NativePlugin {
    fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn execute(&self, ctx: &PluginContext) -> Result<PluginResult> {
        self.execute(ctx)
    }
}

/// Plugin loader responsible for discovering and loading native plugins
pub struct PluginLoader {
    /// Trusted plugin directories
    trusted_dirs: Vec<PathBuf>,
}

impl PluginLoader {
    /// Create a new plugin loader
    pub fn new() -> Self {
        Self {
            trusted_dirs: Vec::new(),
        }
    }

    /// Add a trusted plugin directory
    pub fn add_trusted_dir(&mut self, path: PathBuf) -> &mut Self {
        let path = normalize_trusted_dir(path);
        if !self.trusted_dirs.contains(&path) {
            self.trusted_dirs.push(path);
        }
        self
    }

    /// Get trusted plugin directories
    pub fn trusted_dirs(&self) -> &[PathBuf] {
        &self.trusted_dirs
    }

    /// Load a plugin from a specific path
    pub fn load_plugin(&self, plugin_path: &Path) -> Result<Box<dyn NativePluginTrait>> {
        debug!("Loading native plugin from {:?}", plugin_path);

        let plugin_path = self.ensure_trusted_path(plugin_path, "Plugin path")?;

        // Find the dynamic library file
        let lib_path = self.find_library_file(&plugin_path)?;
        let lib_path = self.ensure_trusted_path(&lib_path, "Plugin library path")?;

        // SAFETY: Loading a dynamic library is inherently unsafe because:
        // 1. The library code executes with full privileges.
        // 2. `lib_path` is an existing canonical path under a trusted root, so
        //    path traversal and symlink escapes were rejected before this point.
        // 3. The library could have bugs or malicious intent.
        //
        // Risk Mitigation:
        // - Only load from canonicalized trusted directories.
        // - Verify ABI version compatibility in `NativePlugin::new`.
        // - Validate metadata format.
        let library = unsafe { Library::new(&lib_path) }
            .with_context(|| format!("Failed to load dynamic library at {:?}", lib_path))?;

        let plugin = NativePlugin::new(library, plugin_path.clone())?;

        info!(
            "Loaded native plugin '{}' v{} from {:?}",
            plugin.metadata.name, plugin.metadata.version, plugin_path
        );

        Ok(Box::new(plugin))
    }

    /// Discover all plugins in trusted directories
    pub fn discover_plugins(&self) -> Result<Vec<Box<dyn NativePluginTrait>>> {
        let mut plugins = Vec::new();

        for dir in &self.trusted_dirs {
            if !dir.exists() {
                continue;
            }

            match self.discover_plugins_in_dir(dir) {
                Ok(mut dir_plugins) => plugins.append(&mut dir_plugins),
                Err(e) => {
                    warn!("Failed to discover plugins in {:?}: {}", dir, e);
                }
            }
        }

        Ok(plugins)
    }

    /// Check if a path is in a trusted directory
    fn is_in_trusted_dir(&self, path: &Path) -> bool {
        self.trusted_dirs.iter().any(|dir| path.starts_with(dir))
    }

    fn ensure_trusted_path(&self, path: &Path, label: &str) -> Result<PathBuf> {
        let path = canonicalize_existing_path(path, label)?;
        if self.is_in_trusted_dir(&path) {
            Ok(path)
        } else {
            Err(anyhow!("{label} {:?} is not in a trusted directory", path))
        }
    }

    /// Find the dynamic library file in a plugin directory
    fn find_library_file(&self, plugin_dir: &Path) -> Result<PathBuf> {
        if !plugin_dir.is_dir() {
            return Err(anyhow!("Plugin path is not a directory"));
        }

        // Look for plugin.json to confirm this is a plugin directory
        let metadata_path = plugin_dir.join("plugin.json");
        if !metadata_path.exists() {
            return Err(anyhow!("No plugin.json found in {:?}", plugin_dir));
        }

        // Look for dynamic library with platform-specific naming
        let lib_name = self.get_library_name_from_metadata(&metadata_path)?;

        let lib_path = plugin_dir.join(&lib_name);
        if lib_path.exists() {
            return Ok(lib_path);
        }

        // Try alternative naming patterns
        let alternatives = self.get_alternative_library_names(&lib_name);
        for alt in alternatives {
            let alt_path = plugin_dir.join(alt);
            if alt_path.exists() {
                return Ok(alt_path);
            }
        }

        Err(anyhow!(
            "No dynamic library found in {:?}. Expected one of: {}, or alternatives",
            plugin_dir,
            lib_name
        ))
    }

    /// Get expected library name from plugin metadata
    fn get_library_name_from_metadata(&self, metadata_path: &Path) -> Result<String> {
        let metadata_content =
            std::fs::read_to_string(metadata_path).context("Failed to read plugin metadata")?;
        let metadata: serde_json::Value =
            serde_json::from_str(&metadata_content).context("Invalid plugin metadata JSON")?;

        let name = metadata["name"]
            .as_str()
            .ok_or_else(|| anyhow!("Plugin metadata missing 'name' field"))?;

        Ok(self.library_filename(name))
    }

    /// Get alternative library names to try
    fn get_alternative_library_names(&self, base_name: &str) -> Vec<String> {
        let mut alternatives = Vec::new();

        // Try with and without "lib" prefix
        if let Some(stripped) = base_name.strip_prefix("lib") {
            alternatives.push(stripped.to_string());
        } else {
            alternatives.push(format!("lib{}", base_name));
        }

        // Try different extensions
        let base = base_name.strip_prefix("lib").unwrap_or(base_name);
        #[cfg(target_os = "macos")]
        {
            alternatives.push(format!("{}.dylib", base));
            alternatives.push(format!("lib{}.dylib", base));
        }
        #[cfg(target_os = "linux")]
        {
            alternatives.push(format!("{}.so", base));
            alternatives.push(format!("lib{}.so", base));
        }
        #[cfg(target_os = "windows")]
        {
            alternatives.push(format!("{}.dll", base));
            alternatives.push(format!("lib{}.dll", base));
        }

        alternatives
    }

    /// Discover plugins in a directory
    fn discover_plugins_in_dir(&self, dir: &Path) -> Result<Vec<Box<dyn NativePluginTrait>>> {
        let mut plugins = Vec::new();

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() && path.join("plugin.json").exists() {
                match self.load_plugin(&path) {
                    Ok(plugin) => plugins.push(plugin),
                    Err(e) => {
                        warn!("Failed to load plugin at {:?}: {}", path, e);
                    }
                }
            }
        }

        Ok(plugins)
    }

    /// Convert a plugin name to a library filename for the current platform
    pub fn library_filename(&self, name: &str) -> String {
        #[cfg(target_os = "macos")]
        {
            format!("lib{}.dylib", name)
        }
        #[cfg(target_os = "linux")]
        {
            format!("lib{}.so", name)
        }
        #[cfg(target_os = "windows")]
        {
            format!("{}.dll", name)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            format!("lib{}", name)
        }
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate a plugin directory structure
pub fn validate_plugin_structure(plugin_dir: &Path) -> Result<Vec<String>> {
    let mut errors = Vec::new();

    // Check for plugin.json
    if !plugin_dir.join("plugin.json").exists() {
        errors.push("Missing plugin.json".to_string());
    }

    // Check for dynamic library
    let has_lib = std::fs::read_dir(plugin_dir)
        .map(|entries| {
            entries.filter_map(|e| e.ok()).any(|entry| {
                let path = entry.path();
                let ext = path.extension().and_then(|e| e.to_str());
                matches!(ext, Some("dylib") | Some("so") | Some("dll"))
            })
        })
        .unwrap_or(false);

    if !has_lib {
        errors.push("No dynamic library found (.dylib, .so, or .dll)".to_string());
    }

    // Validate plugin.json structure
    if let Ok(content) = std::fs::read_to_string(plugin_dir.join("plugin.json")) {
        if let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&content) {
            if metadata["name"].as_str().is_none() {
                errors.push("plugin.json missing required 'name' field".to_string());
            }
            if metadata["description"].as_str().is_none() {
                errors.push("plugin.json missing required 'description' field".to_string());
            }
            if metadata["version"].as_str().is_none() {
                errors.push("plugin.json missing required 'version' field".to_string());
            }
        } else {
            errors.push("Invalid JSON in plugin.json".to_string());
        }
    }

    Ok(errors)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    unsafe extern "C" fn test_free_string(ptr: *const libc::c_char) {
        TEST_FREE_WAS_CALLED.with(|was_called| was_called.set(true));
        if !ptr.is_null() {
            // Safety: tests pass pointers produced by `CString::into_raw` in this module.
            let _ = unsafe { CString::from_raw(ptr as *mut libc::c_char) };
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

    unsafe extern "C" fn test_execute_with_delay(
        _input: *const libc::c_char,
    ) -> *const libc::c_char {
        let active_calls = TEST_EXECUTE_ACTIVE_CALLS.fetch_add(1, Ordering::SeqCst) + 1;
        update_max_concurrency(active_calls);
        std::thread::sleep(Duration::from_millis(25));
        TEST_EXECUTE_ACTIVE_CALLS.fetch_sub(1, Ordering::SeqCst);

        CString::new(r#"{"success":true,"output":{},"error":null,"files":[]}"#)
            .unwrap()
            .into_raw()
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
        std::fs::write(
            plugin_dir.join("plugin.json"),
            r#"{"name": "test", "description": "test", "version": "1.0.0"}"#,
        )
        .unwrap();

        let errors = validate_plugin_structure(&plugin_dir).unwrap();
        assert!(errors.iter().any(|e| e.contains("dynamic library")));
    }

    #[test]
    fn test_validate_plugin_structure_complete() {
        let (_temp_dir, plugin_dir) = create_test_plugin_dir();

        // Create valid plugin.json
        std::fs::write(
            plugin_dir.join("plugin.json"),
            r#"{"name": "test", "description": "test", "version": "1.0.0"}"#,
        )
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
        let err = ensure_non_null_c_string_ptr(std::ptr::null::<libc::c_char>(), "Test pointer")
            .expect_err("null pointer should be rejected");

        assert!(
            err.to_string()
                .contains("Test pointer returned null pointer")
        );
    }

    #[test]
    fn test_decode_plugin_c_string_frees_plugin_buffer() {
        TEST_FREE_WAS_CALLED.with(|was_called| was_called.set(false));

        let raw = CString::new("{\"ok\":true}")
            .expect("valid C string")
            .into_raw();
        let ptr = NonNull::new(raw).expect("non-null raw pointer");

        let decoded = decode_plugin_c_string(
            ptr,
            Some(test_free_string),
            "Plugin result is not valid UTF-8",
        )
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

        let err = decode_plugin_c_string(
            ptr,
            Some(test_free_string),
            "Plugin payload is not valid UTF-8",
        )
        .expect_err("invalid UTF-8 should fail decoding");

        assert!(
            err.to_string()
                .contains("Plugin payload is not valid UTF-8")
        );
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
    fn test_native_plugin_serializes_concurrent_execution() {
        TEST_EXECUTE_ACTIVE_CALLS.store(0, Ordering::SeqCst);
        TEST_EXECUTE_MAX_CONCURRENCY.store(0, Ordering::SeqCst);

        let plugin = Arc::new(NativePlugin {
            _library: current_process_library(),
            metadata: PluginMetadata {
                name: "serialized".to_string(),
                description: "test plugin".to_string(),
                version: "1.0.0".to_string(),
                author: None,
                abi_version: PLUGIN_ABI_VERSION,
                when_to_use: None,
                when_not_to_use: None,
                allowed_tools: None,
            },
            path: PathBuf::from("/tmp/serialized-plugin"),
            execute_fn: test_execute_with_delay,
            free_string_fn: Some(test_free_string),
            execution_lock: Mutex::new(()),
        });
        let ctx = PluginContext {
            input: HashMap::new(),
            workspace_root: None,
            config: HashMap::new(),
        };

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
}
