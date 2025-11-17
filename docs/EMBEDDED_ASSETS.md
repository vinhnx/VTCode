# Embedded Assets Management

## Overview

The vtcode project uses embedded assets that are compiled into the binary. These assets have a **source of truth** and **fallback copies** that must remain synchronized.

## File Locations

### Source of Truth (Workspace Level)
- `prompts/custom/vtcode.md`
- `prompts/custom/generate-agent-file.md`
- `docs/vtcode_docs_map.md`

### Fallback Copies (Embedded Assets Source)
- `vtcode-core/embedded_assets_source/prompts/custom/vtcode.md`
- `vtcode-core/embedded_assets_source/prompts/custom/generate-agent-file.md`
- `vtcode-core/embedded_assets_source/docs/vtcode_docs_map.md`

## Build System Behavior

The build script (`vtcode-core/build.rs`) enforces strict synchronization:

1. **Primary lookup**: Searches workspace root for the canonical asset
2. **Fallback validation**: If a fallback copy exists, it must match the canonical version byte-for-byte
3. **Error on mismatch**: Build fails if files are out of sync

**Example error message:**
```
embedded asset `prompts/custom/generate-agent-file.md` is out of sync. 
Update `vtcode-core/embedded_assets_source/prompts/custom/generate-agent-file.md` 
to match `prompts/custom/generate-agent-file.md`
```

## Workflow

### Editing Embedded Assets

When you modify a source-of-truth file:

1. Edit the workspace-level file (e.g., `prompts/custom/generate-agent-file.md`)
2. Copy the updated file to the fallback location:
   ```bash
   cp prompts/custom/generate-agent-file.md \
      vtcode-core/embedded_assets_source/prompts/custom/generate-agent-file.md
   ```
3. Commit both changes together
4. Build to verify: `cargo build --release` or `cargo check`

### Adding New Embedded Assets

1. Create the asset in the workspace root (e.g., `prompts/custom/my-file.md`)
2. Register it in `vtcode-core/build.rs` EMBEDDED_ASSETS array:
   ```rust
   const EMBEDDED_ASSETS: &[(&str, &str)] = &[
       ("prompts/custom/my-file.md", "prompts/custom/my-file.md"),
       // ... existing entries
   ];
   ```
3. Create the fallback copy in `vtcode-core/embedded_assets_source/prompts/custom/my-file.md`
4. Build to verify the asset is correctly embedded

## Pre-Commit Check (Recommended)

To prevent sync errors, add a git pre-commit hook at `.git/hooks/pre-commit`:

```bash
#!/bin/bash
set -e

WORKSPACE_DIR="."
EMBEDDED_DIR="vtcode-core/embedded_assets_source"

sync_check() {
    local workspace_file=$1
    local embedded_file=$2
    
    if [ -f "$workspace_file" ] && [ -f "$embedded_file" ]; then
        if ! diff -q "$workspace_file" "$embedded_file" > /dev/null; then
            echo "ERROR: $workspace_file and $embedded_file are out of sync"
            echo "Run: cp $workspace_file $embedded_file"
            return 1
        fi
    fi
}

# Check all embedded assets
sync_check "$WORKSPACE_DIR/prompts/custom/vtcode.md" "$EMBEDDED_DIR/prompts/custom/vtcode.md"
sync_check "$WORKSPACE_DIR/prompts/custom/generate-agent-file.md" "$EMBEDDED_DIR/prompts/custom/generate-agent-file.md"
sync_check "$WORKSPACE_DIR/docs/vtcode_docs_map.md" "$EMBEDDED_DIR/docs/vtcode_docs_map.md"

exit 0
```

Make it executable:
```bash
chmod +x .git/hooks/pre-commit
```

## Troubleshooting

### Build fails: "embedded asset is out of sync"

1. Identify which file is out of sync from the error message
2. Copy the workspace version to the fallback:
   ```bash
   cp <workspace_file> <embedded_file>
   ```
3. Rebuild: `cargo build --release` or `cargo check`
4. Commit both changes

### Files accidentally edited in embedded_assets_source

Always edit files in the workspace root, not in `embedded_assets_source`. The embedded directory is auto-synced from workspace files.
