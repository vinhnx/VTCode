# **Asset Synchronization**

This guide explains the embedded asset synchronization workflow between the workspace and the `vtcode-core` crate.

## **Overview**

VT Code maintains a synchronization system to mirror canonical prompts and documentation assets from the workspace into the `vtcode-core/embedded_assets_source` directory. This ensures that the Rust crate has access to the most up-to-date versions of these critical assets.

## **Why Asset Synchronization?**

The sync automation serves several important purposes:

- **Consistency**: Ensures the Rust crate uses the same prompts and documentation as the workspace
- **Single Source of Truth**: Prompts and docs are maintained in one canonical location
- **Build Integration**: Assets are embedded in the crate at build time for distribution
- **Version Control**: Changes to canonical files are tracked and synchronized

## **Synchronized Assets**

The following assets are currently synchronized:

| Source (Workspace) | Destination (Crate) | Purpose |
|-------------------|-------------------|---------|
| `docs/modules/vtcode_docs_map.md` | `vtcode-core/embedded_assets_source/docs/modules/vtcode_docs_map.md` | Documentation map for self-documentation |

## **Using the Sync Script**

### **Location**

The sync script is located at `scripts/sync_embedded_assets.py`.

### **Basic Usage**

```bash
# Run the sync (actual copying)
python3 scripts/sync_embedded_assets.py

# Preview changes without copying (recommended)
python3 scripts/sync_embedded_assets.py --dry-run
```

### **What the Script Does**

1. **Validates Sources**: Ensures all source files exist before proceeding
2. **Creates Directories**: Automatically creates destination directories as needed
3. **Compares Content**: Only copies files that have changed (byte-level comparison)
4. **Preserves Metadata**: Uses `shutil.copy2()` to preserve timestamps and metadata
5. **Reports Changes**: Shows which files were updated or skipped

### **When to Run the Sync Script**

**Always run the sync script after making changes to synchronized assets:**

-   After editing `docs/modules/vtcode_docs_map.md`
-   After adding or removing synchronized assets

### **Integration with Development Workflow**

#### **For Documentation Changes**

When updating documentation in the workspace:

```bash
# 1. Make your documentation changes
vim docs/modules/vtcode_docs_map.md

# 2. Preview the sync to see what will be updated
python3 scripts/sync_embedded_assets.py --dry-run

# 3. Run the sync if changes look correct
python3 scripts/sync_embedded_assets.py

# 4. Verify the changes were applied
git diff vtcode-core/embedded_assets_source/
```

#### **For Prompt Changes**

When updating system prompts:

```bash
# 1. Edit the prompt files (built-in templates)
# (VT Code uses compiled-in templates for most prompts today)

# 2. Test your changes
cargo test --package vtcode-core
```

## **Automated Integration**

### **Pre-commit Hooks (Recommended)**

Consider adding the sync script to your pre-commit workflow:

```bash
#!/bin/bash
# .git/hooks/pre-commit

# Sync embedded assets before commit
python3 scripts/sync_embedded_assets.py --dry-run
if [ $? -ne 0 ]; then
    echo "Sync required. Run: python3 scripts/sync_embedded_assets.py"
    exit 1
fi
```

### **CI/CD Integration**

Add the sync check to your CI pipeline to ensure assets are always synchronized:

```yaml
# .github/workflows/ci.yml
- name: Sync Embedded Assets Check
  run: |
    python3 scripts/sync_embedded_assets.py --dry-run
    if [ $? -ne 0 ]; then
      echo "Assets need synchronization. Run sync script."
      exit 1
    fi
```

## **Troubleshooting**

### **Common Issues**

**"Source asset missing" Error**
- Ensure the source file exists in the expected location
- Check file permissions and accessibility
- Verify the path in `ASSET_MAPPINGS` is correct

**Permission Errors**
- Ensure you have write permissions to `vtcode-core/embedded_assets_source/`
- Check that the script has execute permissions: `chmod +x scripts/sync_embedded_assets.py`

**Unexpected Changes**
- Always use `--dry-run` first to preview changes
- Review the diff before committing: `git diff vtcode-core/embedded_assets_source/`
- Check that you haven't accidentally modified the wrong files

### **Verification Steps**

After running the sync, verify it worked correctly:

```bash
# 1. Check what changed
git status vtcode-core/embedded_assets_source/

# 2. Review the differences
git diff vtcode-core/embedded_assets_source/

# 3. Ensure tests still pass
cargo test --package vtcode-core

# 4. Verify the crate builds correctly
cargo build --package vtcode-core
```

## **Adding New Synchronized Assets**

To add new assets to the synchronization system:

1. **Update the Script**: Add your file mappings to `ASSET_MAPPINGS` in `scripts/sync_embedded_assets.py`
2. **Test the Sync**: Run `python3 scripts/sync_embedded_assets.py --dry-run` to test
3. **Run Initial Sync**: Execute the sync to create the initial copy
4. **Update Documentation**: Document the new asset in this guide

Example addition to `ASSET_MAPPINGS`:

```python
ASSET_MAPPINGS = {
    # ... existing mappings ...
    ROOT / "docs" / "new-guideline.md": CORE_EMBEDDED
    / "docs" / "new-guideline.md",
}
```

## **Best Practices**

- **Always preview first**: Use `--dry-run` to see what will be changed
- **Run regularly**: Sync assets whenever you modify canonical files
- **Review changes**: Check `git diff` before committing synchronized changes
- **Test after sync**: Ensure the crate still builds and tests pass
- **Document changes**: Update this guide when adding new synchronized assets

## **Related Documentation**

- **[Building from Source](./building.md)** - How the sync integrates with the build process
- **[Testing Strategies](./testing-strategies.md)** - Testing the synchronized assets
- **[Code Standards](./code-style.md)** - Style guidelines for documentation and prompts

---

## **Navigation**

- **[Back to Development Guide](./README.md)**
- **[Next: Adding New Tools](./adding-tools.md)**

## **Quick Reference**

```bash
# Preview sync changes
python3 scripts/sync_embedded_assets.py --dry-run

# Execute sync
python3 scripts/sync_embedded_assets.py

# Check what changed
git diff vtcode-core/embedded_assets_source/
```