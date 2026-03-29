# Ast-Grep Project Workflows

Use the public structural tool first for read-only project checks:

- `workflow="scan"` maps to `sg scan <path> --config <config_path>`
- `workflow="test"` maps to `sg test --config <config_path>`
- Default config path is workspace `sgconfig.yml`
- `filter` narrows rules in `scan` and test cases in `test`
- `globs`, `context_lines`, and `max_results` apply to `scan`
- `skip_snapshot_tests` applies only to `test`

Switch to direct CLI work through `unified_exec` when the task needs:

- `sg new`
- Rewrite/apply behavior
- Interactive flags
- `transform`
- `rewriters`
- Custom `sgconfig.yml` authoring across `customLanguages`, `languageGlobs`, `languageInjections`, or `expandoChar`
- Iterative rule debugging that depends on unsupported ast-grep flags or output formats
