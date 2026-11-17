# Agent modes and permissions

CLI, Desktop, and ACP use `SubagentSpec::display_name()` for built-in names.
Canonical IDs remain stable for saved sessions, configuration, and selection payloads.
Custom agents retain their own names and take precedence over built-in aliases.

| Name | Stored ID | Purpose |
| --- | --- | --- |
| Build | `build` | Implement changes using configured permissions |
| Autonomous | `auto` | Implement and verify with automatic agent permissions |
| Ask | `duck` | Discuss and explore without editing files |
| Plan | `plan` | Prepare a plan for approval |
| Coordinator | `coordinator` | Coordinate matrix workers |

`ask` and `autonomous` are selection aliases. Build and Autonomous retain their distinct
prompts and permission defaults; changing their display names does not migrate permissions.

`PermissionMode` owns the shared English labels, descriptions, and localization keys.
Desktop tests enforce English translation parity while preserving all translated strings.
The separate command permission selector offers:

- **Ask before protected actions:** confirm protected commands through the approval flow.
- **Auto-approve commands:** skip command confirmations; sandbox and tool authorization still apply.
- **Full access:** disable the process sandbox and approval prompts after explicit consent;
  explicit denies still apply.

Agent permissions still participate in effective authorization. The command selector does not
override read-only agent restrictions, and choosing an agent does not imply Full access.
