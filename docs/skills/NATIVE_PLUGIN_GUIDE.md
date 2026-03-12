# Native Plugin System for VT Code

## Overview

VT Code's Native Plugin System allows you to extend VT Code's capabilities with high-performance, pre-compiled native code plugins. Built on the [`libloading`](https://docs.rs/libloading) crate, this system enables dynamic loading of native code as skills while maintaining memory safety and security.

## What are Native Plugins?

Native plugins are dynamically-loaded libraries (`.dylib` on macOS, `.so` on Linux, `.dll` on Windows) that implement the VT Code plugin ABI. They provide:

- **High Performance**: Execute compute-intensive tasks at native speed
- **Code Protection**: Keep proprietary algorithms in compiled form
- **System Integration**: Access system APIs and libraries not available to pure Rust code
- **Language Flexibility**: Write plugins in any language that can compile to a dynamic library with C ABI

## Plugin Architecture

### Plugin Structure

A native plugin skill consists of:

```
my-plugin/
├── plugin.json          # Required: Plugin metadata
├── libmy_plugin.dylib   # Required: Compiled plugin library (platform-specific extension)
├── README.md            # Optional: Usage documentation
├── scripts/             # Optional: Helper scripts
└── templates/           # Optional: Reference templates
```

### plugin.json Format

```json
{
  "name": "my-plugin",
  "description": "What this plugin does and when to use it",
  "version": "1.0.0",
  "author": "Your Name",
  "abi_version": 1,
  "when_to_use": "Use when you need high-performance computation",
  "when_not_to_use": "Do not use for simple text processing tasks",
  "allowed_tools": ["bash", "file_read", "file_write"]
}
```

**Required Fields:**
- `name`: Plugin identifier (lowercase, hyphens, max 64 chars)
- `description`: What the plugin does (max 1024 chars)
- `version`: Semantic version string (e.g., "1.0.0")

**Optional Fields:**
- `author`: Plugin creator
- `abi_version`: Plugin ABI version (defaults to 1)
- `when_to_use`: Guidance on when to trigger this plugin
- `when_not_to_use`: Guidance on when NOT to use this plugin
- `allowed_tools`: List of VT Code tools the plugin can use

## Plugin ABI

Plugins must export the following C-compatible symbols:

### 1. `vtcode_plugin_version()`

Returns the ABI version number.

```c
uint32_t vtcode_plugin_version(void);
```

**Implementation:**
```c
uint32_t vtcode_plugin_version(void) {
    return 1;  // Current ABI version
}
```

### 2. `vtcode_plugin_metadata()`

Returns plugin metadata as a JSON string.

```c
const char* vtcode_plugin_metadata(void);
```

**Implementation:**
```c
const char* vtcode_plugin_metadata(void) {
    return R"({
        "name": "my-plugin",
        "description": "High-performance data processing",
        "version": "1.0.0",
        "author": "Your Name",
        "abi_version": 1
    })";
}
```

### 3. `vtcode_plugin_execute()`

Main execution entry point. Takes JSON input, returns JSON output.

```c
const char* vtcode_plugin_execute(const char* input_json);
```

**Input JSON Format:**
```json
{
  "input": {
    "key1": "value1",
    "key2": 42
  },
  "workspace_root": "/path/to/workspace",
  "config": {
    "option1": "value1"
  }
}
```

**Output JSON Format:**
```json
{
  "success": true,
  "output": {
    "result": "processed data",
    "count": 100
  },
  "error": null,
  "files": ["/path/to/generated/file.txt"]
}
```

**Implementation Example:**
```c
const char* vtcode_plugin_execute(const char* input_json) {
    // Parse input_json
    // Execute plugin logic
    // Allocate and return result JSON
    
    static thread_local std::string result;
    result = R"({
        "success": true,
        "output": {"result": "processed"},
        "error": null,
        "files": []
    })";
    return result.c_str();
}
```

## Creating a Plugin

### Step 1: Choose Your Language

You can write plugins in any language that supports C ABI:

- **Rust** (recommended): Type-safe, memory-safe, excellent FFI
- **C/C++**: Maximum performance and compatibility
- **Zig**: Modern systems programming language
- **Other**: Any language with C FFI support

### Step 2: Rust Plugin Example

**Cargo.toml:**
```toml
[package]
name = "my-vtcode-plugin"
version = "1.0.0"
edition = "2021"

[lib]
name = "my_plugin"
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
libc = "0.2"
```

**src/lib.rs:**
```rust
use libc::c_char;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::CString;

#[derive(Debug, Deserialize)]
struct PluginInput {
    input: HashMap<String, serde_json::Value>,
    workspace_root: Option<String>,
    config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct PluginOutput {
    success: bool,
    output: HashMap<String, serde_json::Value>,
    error: Option<String>,
    files: Vec<String>,
}

#[no_mangle]
pub extern "C" fn vtcode_plugin_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn vtcode_plugin_metadata() -> *const c_char {
    let metadata = r#"{
        "name": "my-plugin",
        "description": "High-performance data processing plugin",
        "version": "1.0.0",
        "author": "Your Name",
        "abi_version": 1
    }"#;
    CString::new(metadata).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn vtcode_plugin_execute(input_json: *const c_char) -> *const c_char {
    unsafe {
        let input_str = std::ffi::CStr::from_ptr(input_json).to_str().unwrap();
        let input: PluginInput = serde_json::from_str(input_str).unwrap();
        
        // Plugin logic here
        let mut output = HashMap::new();
        output.insert("result".to_string(), serde_json::json!("processed"));
        
        let result = PluginOutput {
            success: true,
            output,
            error: None,
            files: vec![],
        };
        
        let result_json = serde_json::to_string(&result).unwrap();
        CString::new(result_json).unwrap().into_raw()
    }
}
```

**Build:**
```bash
cargo build --release
# Output: target/release/libmy_plugin.dylib (macOS)
#         target/release/libmy_plugin.so (Linux)
#         target/release/my_plugin.dll (Windows)
```

### Step 3: Install the Plugin

Copy the plugin to a VT Code plugin directory:

**User Plugins:**
```bash
mkdir -p ~/.vtcode/plugins/my-plugin
cp target/release/libmy_plugin.dylib ~/.vtcode/plugins/my-plugin/
cp plugin.json ~/.vtcode/plugins/my-plugin/
```

**Project Plugins:**
```bash
mkdir -p .vtcode/plugins/my-plugin
cp target/release/libmy_plugin.dylib .vtcode/plugins/my-plugin/
cp plugin.json .vtcode/plugins/my-plugin/
```

### Step 4: Use the Plugin

```bash
# List available plugins
vtcode skills list

# View plugin info
vtcode skills info my-plugin

# Use in interactive mode
# The plugin will be automatically loaded when needed
```

## Security Considerations

### Trusted Directories

VT Code only loads plugins from trusted directories:
- `~/.vtcode/plugins/` - User plugins
- `<project>/.vtcode/plugins/` - Project plugins
- `<project>/.agents/plugins/` - Agent plugins

Trusted roots, plugin directories, and library files are canonicalized before
loading. VT Code rejects `..` traversal and symlink escapes that would resolve
outside a trusted root.

**Never load plugins from untrusted sources!** Native code executes with your user privileges.

### Plugin Validation

VT Code validates plugins before loading:
1. Checks plugin.json exists and is valid JSON
2. Verifies required metadata fields
3. Confirms dynamic library exists
4. Validates ABI version compatibility

### Future Enhancements

Planned security features:
- Plugin signature verification
- Checksum validation
- Sandboxed execution (where available)
- Permission system for system access

## Plugin Best Practices

### 1. Error Handling

Always return structured error information:

```rust
let result = PluginOutput {
    success: false,
    output: HashMap::new(),
    error: Some("Invalid input: missing required field 'data'".to_string()),
    files: vec![],
};
```

### 2. Memory Management

For Rust plugins, use `CString` carefully:

```rust
#[no_mangle]
pub extern "C" fn vtcode_plugin_metadata() -> *const c_char {
    let metadata = r#"{"name": "my-plugin"}"#;
    CString::new(metadata).unwrap().into_raw()
    // VT Code will call vtcode_plugin_free_string to free this
}
```

### 3. Concurrency

ABI v1 plugins should still avoid unsynchronized global state, but VT Code
currently executes each loaded plugin instance serially:

```rust
use std::sync::Mutex;

static STATE: Mutex<Option<State>> = Mutex::new(None);
```

Future parallel execution would require an explicit ABI or capability change.

### 4. Performance

- Minimize allocations in hot paths
- Use string views where possible
- Cache expensive computations
- Consider async operations for I/O

### 5. Documentation

Provide clear documentation:
- What the plugin does
- Input/output formats
- Configuration options
- Examples and use cases

## Troubleshooting

### Plugin Not Found

**Problem:** VT Code doesn't list your plugin

**Solutions:**
1. Verify plugin is in a trusted directory
2. Check plugin.json exists and is valid JSON
3. Ensure library filename matches plugin name
4. Run `vtcode skills config` to verify plugin paths

### ABI Version Mismatch

**Problem:** "Plugin ABI version mismatch" error

**Solution:** Update your plugin's `vtcode_plugin_version()` to return the current ABI version (1).

### Library Loading Failed

**Problem:** "Failed to load dynamic library" error

**Solutions:**
1. Check library has correct permissions (executable)
2. Verify library is compiled for your platform
3. Check for missing dependencies (`ldd` on Linux, `otool -L` on macOS)
4. Ensure library architecture matches (e.g., x86_64 vs arm64)

### Plugin Crashes

**Problem:** VT Code crashes when using plugin

**Solutions:**
1. Check plugin logs for error messages
2. Run with `RUST_BACKTRACE=1` for detailed error info
3. Verify plugin handles all input cases
4. Test plugin in isolation first

## Advanced Topics

### Plugin Configuration

Plugins can receive configuration via the `config` field:

```json
{
  "config": {
    "max_threads": 4,
    "cache_enabled": true,
    "output_format": "json"
  }
}
```

### File Generation

Plugins can create files and return references:

```rust
let result = PluginOutput {
    success: true,
    output: HashMap::new(),
    error: None,
    files: vec!["/path/to/generated/file.txt".to_string()],
};
```

### Plugin Chaining

Multiple plugins can be used in sequence:

```bash
# Future feature
vtcode skills use plugin-a | vtcode skills use plugin-b
```

## API Reference

### PluginLoader

```rust
use vtcode_core::skills::native_plugin::PluginLoader;

let mut loader = PluginLoader::new();
loader.add_trusted_dir(PathBuf::from("~/.vtcode/plugins"));

// Discover all plugins
let plugins = loader.discover_plugins()?;

// Load a specific plugin
let plugin = loader.load_plugin(&plugin_path)?;
```

### NativePlugin

```rust
use vtcode_core::skills::native_plugin::{NativePlugin, PluginContext};

let ctx = PluginContext {
    input: HashMap::new(),
    workspace_root: Some("/path/to/workspace".to_string()),
    config: HashMap::new(),
};

let result = plugin.execute(&ctx)?;
```

## Examples

See example plugins in the VT Code repository:
- `examples/plugins/hello-world/` - Minimal plugin example
- `examples/plugins/data-processor/` - Data processing plugin
- `examples/plugins/file-analyzer/` - File analysis plugin

## Contributing

We welcome plugin contributions! Please:
1. Follow the plugin specification
2. Include comprehensive tests
3. Document your plugin thoroughly
4. Share with the community

For questions or support, open an issue on the VT Code repository.

## License

Native plugins are subject to the VT Code MIT License. Your plugin code can be licensed under terms of your choice.

---

**See Also:**
- [Agent Skills Guide](./SKILLS_GUIDE.md) - Traditional instruction-based skills
- [libloading Documentation](https://docs.rs/libloading) - Underlying library
- [FFI Guide](https://doc.rust-lang.org/nomicon/ffi.html) - Rust FFI best practices
