# Security: Sensitive File Filtering

## ⚠️ CRITICAL SECURITY FEATURE

VTCode implements **multi-layer protection** to prevent sensitive files from being exposed through the indexer and file browser.

## Protected Files & Patterns

The following files and patterns are **ALWAYS** excluded:

### Environment Files
- `.env`
- `.env.local`
- `.env.production`
- `.env.development`
- `.env.test`
- `.env.*` (any .env variant)

### Version Control
- `.git/` (entire directory)
- `.gitignore`

### System Files
- `.DS_Store` (macOS)

### All Hidden Files
- Any file or directory starting with `.` (dot)

## Protection Layers

### Layer 1: Indexer (vtcode-indexer)
**File:** `vtcode-indexer/src/lib.rs`

1. **WalkBuilder Configuration**
   ```rust
   .hidden(true)  // Skip ALL hidden files and directories
   ```

2. **TraversalFilter Implementation**
   ```rust
   fn should_index_file(&self, path: &Path, config: &SimpleIndexerConfig) -> bool {
       // Skips hidden files if config.ignore_hidden is true
       // ALWAYS skips .env*, .git, .gitignore, .DS_Store
   }
   ```

### Layer 2: File Browser (vtcode-core)
**File:** `vtcode-core/src/ui/tui/session/file_palette.rs`

**Function:** `should_exclude_file(path: &Path) -> bool`

Filters files before they are loaded into the file browser:

```rust
pub fn load_files(&mut self, files: Vec<String>) {
    self.all_files = files
        .into_iter()
        .filter(|path| {
            // SECURITY: Filter out sensitive files before loading
            !Self::should_exclude_file(Path::new(path))
        })
        // ... rest of loading logic
}
```

## Security Guarantees

✅ **`.env` files NEVER appear in file browser**
✅ **`.env` files NEVER indexed**
✅ **`.git` directory NEVER exposed**  
✅ **All hidden files EXCLUDED by default**
✅ **Protection cannot be disabled** (hardcoded)

## Testing

Security filtering is verified with dedicated tests:

```bash
# Test that sensitive files are filtered
cargo test test_security_filters_sensitive_files

# Test the exclusion logic
cargo test test_should_exclude_file
```

### Test Coverage

```rust
#[test]
fn test_security_filters_sensitive_files() {
    let files = vec![
        "/workspace/src/main.rs",           // ✅ Shown
        "/workspace/.env",                  // ❌ HIDDEN
        "/workspace/.env.local",            // ❌ HIDDEN
        "/workspace/.git/config",           // ❌ HIDDEN
        "/workspace/.gitignore",            // ❌ HIDDEN
        "/workspace/.DS_Store",             // ❌ HIDDEN
        "/workspace/.hidden_file",          // ❌ HIDDEN
        "/workspace/tests/test.rs",         // ✅ Shown
    ];
    
    palette.load_files(files);
    
    assert_eq!(palette.total_items(), 2); // Only main.rs and test.rs
}
```

## Why This Matters

**Security Risks Without Filtering:**
- `.env` files may contain API keys, database passwords, secrets
- `.git` directory may contain sensitive commit history
- Hidden files often contain credentials or private configuration
- Exposing these in file browser = potential credential leakage

**Our Protection:**
- **Defense in depth** - multiple layers
- **Fail-secure** - defaults to excluding, not including
- **Cannot be bypassed** - hardcoded protection
- **Tested** - automated security tests

## Files Modified

1. **vtcode-indexer/src/lib.rs**
   - Changed `.hidden(false)` → `.hidden(true)` in WalkBuilder
   - Added sensitive file filtering in `should_index_file()`

2. **vtcode-core/src/ui/tui/session/file_palette.rs**
   - Added `should_exclude_file()` security function
   - Applied filter in `load_files()` method
   - Added security tests

## Future Enhancements

Potential additional protections:
- [ ] Exclude `id_rsa`, `*.pem`, `*.key` (private keys)
- [ ] Exclude `secrets.json`, `credentials.json`
- [ ] Exclude `node_modules/.env` (nested .env files)
- [ ] Add configurable allow-list for specific hidden files
- [ ] Log attempts to access excluded files (audit trail)

## Compliance

This implementation helps meet security requirements for:
- **GDPR** - Prevents accidental exposure of personal data
- **PCI DSS** - Protects payment card data in config files
- **SOC 2** - Demonstrates access controls
- **HIPAA** - Protects health data in environment variables

## Conclusion

VTCode takes security seriously. Sensitive files are **never** indexed or shown in the file browser, protecting your credentials and secrets from accidental exposure.
