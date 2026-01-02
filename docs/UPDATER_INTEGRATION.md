# VT Code Auto-Updater Integration Guide

This guide shows how to integrate the auto-updater into the main CLI binary.

## What's Included

The updater system provides:
- Version checking against GitHub releases
- Semantic version comparison
- Platform-specific download URLs
- Update type detection (major/minor/patch)
- Rate-limited checking (24-hour cache)

## Files

- `src/updater.rs` - Core updater module with version checking logic
- `.github/workflows/native-installer.yml` - Checksum generation workflow

## Quick Integration

### 1. Add Module Declaration

In `src/main.rs` or `src/lib.rs`, add:

```rust
mod updater;

pub use updater::{Updater, UpdateInfo};
```

### 2. Check for Updates (One-time)

In your CLI initialization (e.g., after parsing arguments):

```rust
use crate::updater::Updater;

#[tokio::main]
async fn main() {
    let updater = Updater::new(env!("CARGO_PKG_VERSION"))
        .expect("Invalid version format");
    
    // This is non-blocking and respects rate limits
    if let Ok(Some(update)) = updater.check_for_updates().await {
        eprintln!("📦 New version available: {} (current: {})", 
                  update.version, 
                  env!("CARGO_PKG_VERSION"));
        eprintln!("   Run: vtcode update");
    }
    
    // Record that we checked
    let _ = Updater::record_update_check();
    
    // Continue with normal execution...
}
```

### 3. Add Update Subcommand

Add to your CLI argument parser (using `clap`):

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "vtcode")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Check for and install updates
    Update {
        /// Force update even if already latest version
        #[arg(long)]
        force: bool,
    },
}

async fn handle_update(force: bool) -> anyhow::Result<()> {
    let updater = Updater::new(env!("CARGO_PKG_VERSION"))?;
    
    let current_version = updater.current_version.clone();
    
    match updater.check_for_updates().await? {
        Some(update) if update.version > current_version || force => {
            println!("Downloading VT Code {}...", update.version);
            
            // Download and install update
            // Implementation depends on your packaging strategy
            // For now, just show instructions:
            println!("📥 Download: {}", update.download_url);
            println!("✅ Installation: See https://github.com/vinhnx/vtcode/releases/tag/{}", 
                     update.tag);
        }
        Some(_) => {
            println!("✅ Already on latest version: {}", current_version);
        }
        None => {
            println!("No updates available");
        }
    }
    
    Ok(())
}
```

### 4. Call in Main CLI

```rust
match cli.command {
    Some(Commands::Update { force }) => {
        handle_update(force).await?;
    }
    // ... other commands
    _ => {
        // Start normal VT Code CLI...
    }
}
```

## Environment Variables

The updater respects these environment variables:

- `XDG_CACHE_HOME` - Override cache directory (Unix)
- `APPDATA` - User data directory (Windows, automatic)

## Testing

### Test Version Checking

```rust
#[tokio::test]
async fn test_update_check() {
    let updater = Updater::new("0.1.0").unwrap();
    match updater.check_for_updates().await {
        Ok(Some(info)) => println!("Update available: {}", info.version),
        Ok(None) => println!("Already latest"),
        Err(e) => println!("Check failed: {}", e),
    }
}
```

### Manual Testing

```bash
# Test the updater module compiles
cargo check

# Build and test update command
cargo build
./target/debug/vtcode update
```

## Rate Limiting

The updater implements intelligent rate limiting:

1. **24-hour cache**: Only checks GitHub API once per day
2. **Timeout**: 10-second timeout on API requests
3. **Error handling**: Silently skips on connection errors
4. **No retries**: Fail fast approach

Cache location:
- **Unix**: `$XDG_CACHE_HOME/vtcode/` or `~/.cache/vtcode/`
- **Windows**: `%APPDATA%\vtcode\`

Cache file: `last_update_check` (empty marker file)

## Future Enhancements

### Automatic Binary Update

Once the updater is integrated, you could add:

```rust
async fn apply_update(info: UpdateInfo) -> anyhow::Result<()> {
    use self_update::backends::github::Update;
    
    let status = Update::configure()?
        .repo_owner("vinhnx")
        .repo_name("vtcode")
        .bin_name("vtcode")
        .show_download_progress(true)
        .show_output(true)
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()?
        .update()?;
    
    println!("Update status: {}", status.version());
    Ok(())
}
```

But this requires additional dependencies and testing. The current approach (notify user + direct download) is simpler and safer.

### Notification Styles

Different notification levels based on update type:

```rust
match &update {
    u if u.is_major_update(&current) => {
        eprintln!("🚨 Major update available: {} → {}", current, u.version);
    }
    u if u.is_minor_update(&current) => {
        eprintln!("📦 Minor update available: {} → {}", current, u.version);
    }
    u if u.is_patch_update(&current) => {
        eprintln!("🔧 Patch update available: {} → {}", current, u.version);
    }
    _ => {}
}
```

## Troubleshooting

### Update Check Fails Silently

This is intentional - update checking should never block or crash the application. Check logs if available.

### Cache Issues

To force a fresh check:

```bash
# Unix
rm ~/.cache/vtcode/last_update_check

# Windows
del %APPDATA%\vtcode\last_update_check
```

### API Rate Limits

If you get rate limit errors, ensure you're:
1. Using the 24-hour cache (respects `last_update_check`)
2. Not calling `check_for_updates()` excessively
3. Handling errors gracefully (don't retry immediately)

## See Also

- `src/updater.rs` - Full updater implementation
- `docs/NATIVE_INSTALLER.md` - User-facing installer guide
- `docs/DISTRIBUTION_STRATEGY.md` - How distribution works
