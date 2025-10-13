# Markdown Ledger Migration Notes

## Overview
The markdown ledger stores each record as a dedicated Markdown file that groups serialized representations under human-readable headings. These notes outline the stable layout so downstream crates or tools can safely interoperate with the existing archives.

## File Naming
- Each record key maps directly to a filename of the form `<key>.<extension>` inside the storage directory. The default extension is `md`, and custom extensions are supported through `MarkdownStorageOptions::with_extension`. Keys should therefore remain filesystem friendly (alphanumeric, dashes, or underscores) to avoid portability issues.
- The storage directory can be initialized ahead of time with `MarkdownStorage::init`, which will create the directory tree if it does not already exist.

## Top-Level Structure
All files share the same Markdown template:

```markdown
# <title>

## JSON
```json
<pretty-printed JSON payload>
```

## YAML
```yaml
<serde-rendered YAML payload>
```

## Raw Data
<bullet list summary of top-level fields>
```

Sections are optional and can be toggled via `MarkdownStorageOptions`. When a section is disabled it is omitted entirely, ensuring consumers can rely on the headings that remain.

## Record Identifiers
- `MarkdownStorage::store` accepts a logical key and a display title. The key determines the filename, while the title sets the primary `#` heading inside the document.
- Wrapper helpers (`SimpleKVStorage`, `ProjectStorage`) pass structured data that includes the same key, so applications can reconstruct record identifiers by reading either the filename or the JSON payload's natural primary key.

## Atomic Persistence
Writes are persisted atomically by streaming the rendered Markdown into a temporary file that lives alongside the target path and then renaming it into place. This prevents partial writes if the process is interrupted and makes concurrent readers safe.

## Compatibility Guidance
- Consumers that only need machine-readable data can parse the fenced JSON or YAML code blocks without relying on Markdown semantics.
- The `Raw Data` section is intended for human inspection; it summarizes top-level JSON fields but should not be treated as an authoritative serialization format.
- When migrating existing archives, check for the configured extension and section toggles to ensure new tooling continues to recognize files created with custom options.

## Next Steps
Future work should include example migrations that rename or reorganize keys while preserving the ledger layout, plus tooling to validate archives for consistency before publishing the crate standalone.
