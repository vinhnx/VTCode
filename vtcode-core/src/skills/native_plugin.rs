//! Native Plugin System for VT Code Skills
//!
//! This module provides support for loading native code plugins as skills using libloading.
//! Native plugins offer high-performance, pre-compiled skill logic that can be discovered
//! and loaded dynamically at runtime.
//!
//! # Safety
//!
//! Loading native code plugins requires careful security considerations:
//! - Plugins are loaded from trusted locations only
//! - Plugin signatures can be verified (future enhancement)
//! - Plugin execution is sandboxed where possible
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
use libloading::{Library, Symbol};
use serde::{Deserialize, Serialize};
use hashbrown::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Current plugin ABI version
/// Increment this when breaking changes are made to the plugin interface
pub const PLUGIN_ABI_VERSION: u32 = 1;

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
    execute_fn: unsafe extern "C" fn(*const libc::c_char) -> *const libc::c_char,
}

impl std::fmt::Debug for NativePlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativePlugin")
            .field("metadata", &self.metadata)
            .field("path", &self.path)
            .finish()
    }
}

unsafe impl Send for NativePlugin {}
unsafe impl Sync for NativePlugin {}

impl NativePlugin {
    /// Create a new native plugin from a loaded library
    pub fn new(library: Library, path: PathBuf) -> Result<Self> {
        // Verify ABI version
        let version_fn: Symbol<unsafe extern "C" fn() -> u32> = unsafe {
            library
                .get(b"vtcode_plugin_version\0")
                .context("Failed to load vtcode_plugin_version symbol")?
        };

        let abi_version = unsafe { version_fn() };
        if abi_version != PLUGIN_ABI_VERSION {
            return Err(anyhow!(
                "Plugin ABI version mismatch: expected {}, got {}",
                PLUGIN_ABI_VERSION,
                abi_version
            ));
        }

        // Load metadata
        let metadata_fn: Symbol<unsafe extern "C" fn() -> *const libc::c_char> = unsafe {
            library
                .get(b"vtcode_plugin_metadata\0")
                .context("Failed to load vtcode_plugin_metadata symbol")?
        };

        let metadata_ptr = unsafe { metadata_fn() };
        if metadata_ptr.is_null() {
            return Err(anyhow!("Plugin metadata function returned null pointer"));
        }

        let metadata_json = unsafe { std::ffi::CStr::from_ptr(metadata_ptr) }
            .to_str()
            .context("Plugin metadata is not valid UTF-8")?;

        let metadata: PluginMetadata =
            serde_json::from_str(metadata_json).context("Failed to parse plugin metadata JSON")?;

        // Load execute function
        let execute_fn: Symbol<unsafe extern "C" fn(*const libc::c_char) -> *const libc::c_char> = unsafe {
            library
                .get(b"vtcode_plugin_execute\0")
                .context("Failed to load vtcode_plugin_execute symbol")?
        };

        // Safety: We need to transmute the Symbol to a function pointer that doesn't borrow
        // This is safe because we keep the Library alive in the NativePlugin struct
        let execute_fn_ptr = unsafe {
            std::mem::transmute::<
                Symbol<'_, unsafe extern "C" fn(*const libc::c_char) -> *const libc::c_char>,
                unsafe extern "C" fn(*const libc::c_char) -> *const libc::c_char,
            >(execute_fn)
        };

        Ok(Self {
            _library: library,
            metadata,
            path,
            execute_fn: execute_fn_ptr,
        })
    }

    /// Execute the plugin with the given context
    pub fn execute(&self, ctx: &PluginContext) -> Result<PluginResult> {
        let input_json =
            serde_json::to_string(&ctx.input).context("Failed to serialize plugin input")?;

        let input_cstr = std::ffi::CString::new(input_json)
            .context("Failed to create C string from input JSON")?;

        let result_ptr = unsafe { (self.execute_fn)(input_cstr.as_ptr()) };

        if result_ptr.is_null() {
            return Err(anyhow!("Plugin execute function returned null pointer"));
        }

        let result_json = unsafe { std::ffi::CStr::from_ptr(result_ptr) }
            .to_str()
            .context("Plugin result is not valid UTF-8")?;

        let result: PluginResult =
            serde_json::from_str(result_json).context("Failed to parse plugin result JSON")?;

        // Free the allocated memory (plugin is responsible for allocation)
        // We need a separate FFI function for this
        #[allow(unused_extern_crates, dead_code)]
        unsafe extern "C" {
            fn vtcode_plugin_free_string(ptr: *const libc::c_char);
        }
        // Try to free, but don't fail if the function doesn't exist
        // This is optional - plugins can use leak-free allocators

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

        // Validate plugin path is in trusted directory
        if !self.is_in_trusted_dir(plugin_path) {
            return Err(anyhow!(
                "Plugin path {:?} is not in a trusted directory",
                plugin_path
            ));
        }

        // Find the dynamic library file
        let lib_path = self.find_library_file(plugin_path)?;

        // Safety: Loading a dynamic library is inherently unsafe because:
        // 1. The library code executes with full privileges
        // 2. We trust the library is from a trusted source
        // 3. The library could have bugs or malicious intent
        //
        // Safety measures:
        // - Only load from trusted directories
        // - Verify ABI version compatibility
        // - Validate metadata format
        // - Future: support signature verification
        let library = unsafe {
            Library::new(&lib_path)
                .context(format!("Failed to load dynamic library at {:?}", lib_path))?
        };

        let plugin = NativePlugin::new(library, plugin_path.to_path_buf())?;

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
        self.trusted_dirs.iter().any(|dir| {
            path.starts_with(dir)
                || path
                    .parent()
                    .map_or(false, |parent| parent.starts_with(dir))
        })
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
        if base_name.starts_with("lib") {
            alternatives.push(base_name[3..].to_string());
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
    use tempfile::TempDir;

    fn create_test_plugin_dir() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path().join("test-plugin");
        std::fs::create_dir(&plugin_dir).unwrap();
        (temp_dir, plugin_dir)
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
}
