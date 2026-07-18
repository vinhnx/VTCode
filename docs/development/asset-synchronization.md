# **Asset Synchronization**

This guide explains how embedded assets are consumed by the `vtcode-core` crate.

## **Overview**

VT Code embeds selected workspace assets directly into the `vtcode-core` crate at build time. The `build.rs` script resolves these assets from the workspace root, so there is no separate mirror directory to maintain.

## **Synchronized Assets**

The following asset is currently embedded:

| Source (Workspace) | Embedded Destination | Purpose |
|-------------------|---------------------|---------|
| `docs/modules/vtcode_docs_map.md` | `docs/vtcode_docs_map.md` (in crate build output) | Documentation map for self-documentation |

## **How It Works**

1. **Single Source of Truth**: Assets are maintained at the workspace root (e.g., `docs/modules/vtcode_docs_map.md`).
2. **Build-Time Embedding**: `crates/codegen/vtcode-core/build.rs` copies the asset into the crate's build output during compilation.
3. **No Manual Sync Required**: Because the build script reads directly from the workspace root, there is no separate sync step or mirror directory.

## **Verification**

After modifying an embedded asset:

```bash
# Verify the crate still builds
cargo check --package vtcode-core

# Run tests
cargo nextest run -p vtcode-core
```

## **Adding New Embedded Assets**

To embed a new asset:

1. **Add to `build.rs`**: Append a `(source_relative, dest_relative)` tuple to `EMBEDDED_ASSETS` in `crates/codegen/vtcode-core/build.rs`.
2. **Test the Build**: Ensure `cargo check --package vtcode-core` succeeds and the asset is available at runtime.

## **Related Documentation**

- **[Development Guide](./README.md)** - Development overview

---

## **Navigation**

- **[Back to Development Guide](./README.md)**
