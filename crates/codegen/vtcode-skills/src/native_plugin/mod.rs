#![allow(
    unsafe_code,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

//! Native Plugin System for VT Code Skills
//!
//! This module provides support for loading native code plugins as skills using libloading.
//! Native plugins offer high-performance, pre-compiled skill logic that can be discovered
//! and loaded dynamically at runtime.
//!
//! # Safety
//!
//! Loading native code plugins requires careful security considerations:
//! - Plugins are loaded from canonicalized, explicitly trusted locations only
//! - Callers must obtain user approval before loading an untrusted plugin
//! - Plugin signatures can be verified (future enhancement)
//! - This module does not sandbox plugin execution; native code runs with process privileges
//! - VT Code serializes plugin FFI calls for ABI v1
//! - All plugin operations go through VT Code's tool system
//!
//! The `unsafe` blocks in this module are necessary for FFI:
//! - Loading dynamic libraries (`Library::new`)
//! - Calling plugin functions (`version_fn`, `metadata_fn`, `execute_fn`,
//!   `free_string_fn`)
//! - Converting C strings (`CStr::from_ptr`)
//!
//! Symbol resolution is centralized in the safe `get_plugin_symbol` wrapper;
//! the remaining raw calls cannot be made safe without losing the ability to
//! load arbitrary plugin code. Each call site carries its own `// SAFETY:`
//! comment documenting the specific invariant that justifies the unsafe
//! operation.
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
//! ```rust,ignore
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
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Mutex;
use tracing::{debug, info, warn};
use vtcode_commons::canonicalize;

/// Current plugin ABI version
/// Increment this when breaking changes are made to the plugin interface
pub const PLUGIN_ABI_VERSION: u32 = 1;

use std::os::raw::c_char;

/// Plugin ABI v1 entry-point signatures.
///
/// `unsafe extern "C" fn` in type position marks that calling through these
/// pointers is unsafe; neither `rustc`'s `unsafe_code` nor clippy's
/// `undocumented_unsafe_blocks` fires here, so the contract is stated once,
/// here. Each pointer is resolved only from a library that
/// `PluginLoader::load_plugin` has canonicalized and verified as trusted, and is
/// invoked exclusively through the wrappers in this module (`get_plugin_symbol`,
/// `NativePlugin::new`, `NativePlugin::execute_ffi`), each of which carries its
/// own call-site safety justification. Preconditions: the pointer was resolved
/// from the matching symbol of the loaded library, the plugin's threading
/// contract (`PluginMetadata::thread_safe`; otherwise serialized by
/// `NativePlugin::execution_lock`) is honoured, the returned `*const c_char` is
/// non-null and nul-terminated, and plugin-owned strings are released with the
/// matching `free_string` symbol.
type PluginVersionFn = unsafe extern "C" fn() -> u32;
type PluginMetadataFn = unsafe extern "C" fn() -> *const c_char;
type PluginExecuteFn = unsafe extern "C" fn(*const c_char) -> *const c_char;
type PluginFreeStringFn = unsafe extern "C" fn(*const c_char);

/// Plugin execution context passed to plugin functions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginContext {
    /// Input data for the plugin
    input: HashMap<String, serde_json::Value>,
    /// Workspace root path
    workspace_root: Option<String>,
    /// Plugin configuration
    config: HashMap<String, serde_json::Value>,
}

/// Plugin execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginResult {
    /// Success flag
    success: bool,
    /// Output data
    output: HashMap<String, serde_json::Value>,
    /// Error message if failed
    error: Option<String>,
    /// Optional file references created by the plugin
    files: Vec<String>,
}

/// Plugin metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Plugin name (identifier)
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Plugin version (semver)
    version: String,
    /// Plugin author
    author: Option<String>,
    /// Plugin ABI version
    abi_version: u32,
    /// When to use this plugin
    when_to_use: Option<String>,
    /// When NOT to use this plugin
    when_not_to_use: Option<String>,
    /// Allowed tools for this plugin
    allowed_tools: Option<Vec<String>>,
    /// Whether the plugin is thread-safe (Sync).
    /// If false, VT Code serializes all calls to this plugin instance.
    #[serde(default)]
    thread_safe: bool,
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
    /// Loaded plugin library handle.
    ///
    /// # Lifetime invariant
    ///
    /// `_library` outlives `self`. `execute_fn` is a raw function pointer copied
    /// out of this library and is only valid while `_library` remains loaded.
    /// `NativePlugin` intentionally has no `Drop`: cleanup is delegated to
    /// `Library`'s own `Drop` (`dlclose`), which runs after `self` is dropped,
    /// guaranteeing the function pointer is never used after the library unloads.
    _library: Library,
    /// Plugin metadata
    metadata: PluginMetadata,
    /// Path to the plugin
    path: PathBuf,
    /// Plugin execute function pointer
    execute_fn: PluginExecuteFn,
    /// Optional plugin-owned deallocator for returned strings
    free_string_fn: Option<PluginFreeStringFn>,
    /// Serialize calls if the plugin is not reentrant.
    execution_lock: Mutex<()>,
    /// Optimized thread-safety flag from metadata.
    thread_safe: bool,
}

fn ensure_non_null_c_string_ptr(ptr: *const c_char, context: &'static str) -> Result<NonNull<c_char>> {
    NonNull::new(ptr.cast_mut()).ok_or_else(|| anyhow!("{context} returned null pointer"))
}

fn decode_plugin_c_string(
    ptr: NonNull<c_char>,
    free_string_fn: Option<PluginFreeStringFn>,
    utf8_error_context: &'static str,
) -> Result<String> {
    let raw_ptr = ptr.as_ptr() as *const c_char;
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

fn canonicalize_existing_path(path: &Path, label: &str) -> Result<PathBuf> {
    canonicalize(path).with_context(|| format!("Failed to resolve {label} '{}'", path.display()))
}

/// Resolve a C-ABI symbol from a trusted, already-validated plugin library.
///
/// Single wrapper around libloading's symbol lookup; every plugin symbol
/// passage goes through here. `T` must be a `Copy` function-pointer type. The
/// returned value is valid only while `library` (`NativePlugin._library`) is loaded.
fn get_plugin_symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T> {
    // SAFETY: symbol name and signature are defined by the plugin ABI; we trust
    // the library (canonicalized/trusted location). `T: Copy` makes the deref safe.
    let symbol: Symbol<T> = unsafe { library.get(name) }.with_context(|| {
        format!(
            "Failed to load plugin symbol: {:?}",
            String::from_utf8_lossy(name.strip_suffix(b"\0").unwrap_or(name))
        )
    })?;
    Ok(*symbol)
}

fn normalize_trusted_dir(path: PathBuf) -> PathBuf {
    canonicalize_existing_path(&path, "trusted plugin directory").unwrap_or_else(|_| {
        if path.is_absolute() {
            path
        } else {
            std::env::current_dir().map(|cwd| cwd.join(&path)).unwrap_or(path)
        }
    })
}

#[cfg(unix)]
fn warn_if_insecure_trusted_dir(dir: &Path) {
    if let Ok(metadata) = std::fs::metadata(dir) {
        let mode = metadata.permissions().mode();
        if mode & 0o022 != 0 {
            let perm_octal = format!("{:o}", mode & 0o777);
            warn!(
                dir = ?dir,
                mode = %perm_octal,
                "trusted plugin directory is writable by group or others; a non-owner user could plant a malicious plugin via symlink or replacement"
            );
        }
    }
}

#[cfg(not(unix))]
fn warn_if_insecure_trusted_dir(_dir: &Path) {}

impl NativePlugin {
    /// Create a new native plugin from a loaded library
    fn new(library: Library, path: PathBuf) -> Result<Self> {
        // Verify ABI version
        let version_fn = get_plugin_symbol::<PluginVersionFn>(&library, b"vtcode_plugin_version\0")?;

        // SAFETY: function pointer was loaded from a validated ABI symbol.
        let abi_version = unsafe { version_fn() };
        if abi_version != PLUGIN_ABI_VERSION {
            return Err(anyhow!("Plugin ABI version mismatch: expected {PLUGIN_ABI_VERSION}, got {abi_version}"));
        }

        // Optional cleanup function for plugin-owned strings.
        let free_string_fn = get_plugin_symbol::<PluginFreeStringFn>(&library, b"vtcode_plugin_free_string\0").ok();

        // Load metadata
        let metadata_fn = get_plugin_symbol::<PluginMetadataFn>(&library, b"vtcode_plugin_metadata\0")?;

        // SAFETY: function pointer loaded from the validated ABI symbol.
        let metadata_ptr = ensure_non_null_c_string_ptr(unsafe { metadata_fn() }, "Plugin metadata function")?;
        let metadata_json = decode_plugin_c_string(metadata_ptr, free_string_fn, "Plugin metadata is not valid UTF-8")?;

        let metadata: PluginMetadata =
            serde_json::from_str(&metadata_json).context("Failed to parse plugin metadata JSON")?;

        // Load execute function
        let execute_fn = get_plugin_symbol::<PluginExecuteFn>(&library, b"vtcode_plugin_execute\0")?;

        Ok(Self {
            _library: library,
            metadata: metadata.clone(), // Clone metadata to store it
            path,
            execute_fn,
            free_string_fn,
            execution_lock: Mutex::new(()),
            thread_safe: metadata.thread_safe, // Store the thread_safe flag
        })
    }

    /// Execute the plugin with the given context
    fn execute(&self, ctx: &PluginContext) -> Result<PluginResult> {
        let input_json = serde_json::to_string(ctx).context("Failed to serialize plugin context")?;
        let input_cstr = CString::new(input_json).context("Plugin context contains internal null bytes")?;

        // Serialization logic from Antithesis Part 2 (Challenge 2):
        // If the plugin is not explicitly marked as thread-safe, we must serialize
        // access through the execution lock to prevent unsynchronized shared access.
        if !self.thread_safe {
            let _execution_guard = self
                .execution_lock
                .lock()
                .map_err(|_e| anyhow!("native plugin execution lock poisoned"))?;

            self.execute_ffi(input_cstr)
        } else {
            // Reentrant plugins can skip the lock for multi-threaded performance.
            self.execute_ffi(input_cstr)
        }
    }

    /// Internal FFI bridge for plugin execution.
    ///
    /// # Safety
    ///
    /// This function performs the raw FFI call. Callers must ensure that
    /// if the plugin is not thread-safe, it is called under a lock.
    fn execute_ffi(&self, input_cstr: CString) -> Result<PluginResult> {
        // SAFETY:
        // 1. The `input_cstr` pointer is valid for the duration of this call.
        // 2. The `execute_fn` obeys the plugin ABI and expects a nul-terminated string.
        // 3. VT Code either holds `execution_lock` or the plugin is explicitly reentrant (thread_safe: true).
        let result_ptr =
            ensure_non_null_c_string_ptr(unsafe { (self.execute_fn)(input_cstr.as_ptr()) }, "Plugin execute function")?;
        let result_json = decode_plugin_c_string(result_ptr, self.free_string_fn, "Plugin result is not valid UTF-8")?;

        let result: PluginResult = serde_json::from_str(&result_json).context("Failed to parse plugin result JSON")?;

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

/// Plugin loader responsible for discovering and loading native plugins.
///
/// A trusted directory is an application-managed or explicitly user-approved
/// installation location. Repository-controlled directories must not be added
/// as trusted directories: loading a dynamic library executes its constructors
/// before the plugin ABI can be inspected.
pub struct PluginLoader {
    /// Trusted plugin directories
    trusted_dirs: Vec<PathBuf>,
}

impl PluginLoader {
    /// Create a new plugin loader
    pub fn new() -> Self {
        Self { trusted_dirs: Vec::new() }
    }

    /// Add an explicitly trusted plugin directory.
    ///
    /// This method only enforces path containment after the directory is
    /// trusted; it does not verify provenance or obtain user consent. Callers
    /// must not pass paths selected from repository contents.
    pub fn add_trusted_dir(&mut self, path: PathBuf) -> &mut Self {
        let path = normalize_trusted_dir(path);
        if !self.trusted_dirs.contains(&path) {
            warn_if_insecure_trusted_dir(&path);
            self.trusted_dirs.push(path);
        }
        self
    }

    /// Get trusted plugin directories
    pub fn trusted_dirs(&self) -> &[PathBuf] {
        &self.trusted_dirs
    }

    /// Load a plugin from a specific path.
    ///
    /// # Security
    ///
    /// `Library::new` can execute arbitrary native initialization code before
    /// this function returns. The caller must establish explicit user consent
    /// and provenance before calling this method; plugin metadata is not an
    /// authorization mechanism.
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
            .with_context(|| format!("Failed to load dynamic library at {lib_path:?}"))?;

        let plugin = NativePlugin::new(library, plugin_path.clone())?;

        info!(
            "Loaded native plugin '{}' v{} from {:?}",
            plugin.metadata.name, plugin.metadata.version, plugin_path
        );

        Ok(Box::new(plugin))
    }

    /// Discover and load all plugins in trusted directories.
    ///
    /// Loading occurs during discovery, so callers must establish provenance
    /// and explicit user consent before invoking this method.
    pub fn discover_plugins(&self) -> Result<Vec<Box<dyn NativePluginTrait>>> {
        let mut plugins = Vec::new();

        for dir in &self.trusted_dirs {
            warn_if_insecure_trusted_dir(dir);
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
            Err(anyhow!("{label} {path:?} is not in a trusted directory"))
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
            return Err(anyhow!("No plugin.json found in {plugin_dir:?}"));
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

        Err(anyhow!("No dynamic library found in {plugin_dir:?}. Expected one of: {lib_name}, or alternatives"))
    }

    /// Get expected library name from plugin metadata
    fn get_library_name_from_metadata(&self, metadata_path: &Path) -> Result<String> {
        let metadata_content = std::fs::read_to_string(metadata_path).context("Failed to read plugin metadata")?;
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
            alternatives.push(format!("lib{base_name}"));
        }

        // Try different extensions
        let base = base_name.strip_prefix("lib").unwrap_or(base_name);
        #[cfg(target_os = "macos")]
        {
            alternatives.push(format!("{base}.dylib"));
            alternatives.push(format!("lib{base}.dylib"));
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
            format!("lib{name}.dylib")
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
mod tests;
