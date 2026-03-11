# Editor Context Bridge

VT Code accepts editor context from any IDE that writes a canonical JSON snapshot file and sets
`VT_IDE_CONTEXT_FILE` in the VT Code process environment.

When `VT_IDE_CONTEXT_FILE` is not set, VT Code also looks for a workspace-local fallback at
`.vtcode/ide-context.json` and then `.vtcode/ide-context.md`. This is primarily for external
terminal sessions started outside the IDE process.

## Contract

- File format: UTF-8 JSON
- Environment variable: `VT_IDE_CONTEXT_FILE`
- Workspace fallback: `.vtcode/ide-context.json`
- Snapshot version: `1`
- Provider families: `vscode_compatible`, `zed`, `generic`
- Backward compatibility: VT Code still reads `VT_VSCODE_CONTEXT_FILE` and legacy VS Code
  markdown snapshots during migration

## Canonical payload

```json
{
  "version": 1,
  "provider_family": "generic",
  "workspace_root": "/workspace",
  "active_file": {
    "path": "/workspace/src/main.rs",
    "language_id": "rust",
    "line_range": {
      "start": 120,
      "end": 148
    },
    "dirty": true,
    "truncated": false,
    "selection": {
      "range": {
        "start_line": 128,
        "start_column": 5,
        "end_line": 132,
        "end_column": 17
      },
      "text": "let value = compute();"
    }
  },
  "visible_editors": [
    {
      "path": "/workspace/src/lib.rs",
      "language_id": "rust",
      "line_range": {
        "start": 1,
        "end": 80
      },
      "dirty": false,
      "truncated": false
    }
  ]
}
```

## Field notes

- `active_file.path` may be absolute, relative to `workspace_root`, or an editor URI like
  `untitled:Scratch-1`.
- `line_range` is optional and is usually the visible/editor-focused range.
- `selection.text` is optional and should only be included for explicit selections.
- `visible_editors` is optional. VT Code parses it for future use but does not inject it into the
  prompt by default in this first pass.

## Integration checklist

1. Write the latest snapshot to a stable file path whenever the active editor changes.
2. Set `VT_IDE_CONTEXT_FILE` for every spawned VT Code process.
3. Use `provider_family = "generic"` for JetBrains and any non-native adapter.
