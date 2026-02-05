/// File read cache defaults (development and general use)
pub const FILE_READ_CACHE_MIN_SIZE_BYTES: usize = 256 * 1024; // 256 KB
pub const FILE_READ_CACHE_MAX_SIZE_BYTES: usize = 10 * 1024 * 1024; // 10 MB
pub const FILE_READ_CACHE_TTL_SECS: u64 = 300;
pub const FILE_READ_CACHE_MAX_ENTRIES: usize = 128;

/// File read cache defaults for production
pub const FILE_READ_CACHE_PROD_MIN_SIZE_BYTES: usize = 512 * 1024; // 512 KB
pub const FILE_READ_CACHE_PROD_MAX_SIZE_BYTES: usize = 25 * 1024 * 1024; // 25 MB
pub const FILE_READ_CACHE_PROD_TTL_SECS: u64 = 600;
pub const FILE_READ_CACHE_PROD_MAX_ENTRIES: usize = 256;

/// Command cache defaults (development and general use)
pub const COMMAND_CACHE_TTL_MS: u64 = 2_000;
pub const COMMAND_CACHE_MAX_ENTRIES: usize = 128;
pub const COMMAND_CACHE_ALLOWLIST: &[&str] = &["rg", "ls", "git status", "git diff --stat"];

/// Command cache defaults for production
pub const COMMAND_CACHE_PROD_TTL_MS: u64 = 3_000;
pub const COMMAND_CACHE_PROD_MAX_ENTRIES: usize = 256;
pub const COMMAND_CACHE_PROD_ALLOWLIST: &[&str] = &["rg", "ls", "git status", "git diff --stat"];
