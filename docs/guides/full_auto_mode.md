# Full-Auto Mode

Full-auto mode lets VT Code run without pausing for human approval. Use this capability only when
you fully trust the workspace configuration and have reviewed the safeguards below.

## Activation Checklist

1. **Update `vtcode.toml`**
    - Enable the feature: `automation.full_auto.enabled = true`.
    - Configure the tool allow-list to match your risk tolerance.
    - (Recommended) Keep `require_profile_ack = true` so a profile file is required.
2. **Create the acknowledgement profile**
    - Place the file referenced by `automation.full_auto.profile_path` in your workspace.
    - Document acceptable behaviours, escalation procedures, and any workspace-specific hazards.
3. **Review tool policies**
    - Full-auto still honours existing tool policies; denied tools remain blocked.
    - Tools not included in the allow-list will be rejected automatically.
4. **Launch the agent**
    - Run `vtcode --full-auto` (combine with other CLI flags as needed).
    - The agent will also set `--skip-confirmations` internally.

## Runtime Behaviour

-   VT Code displays the active allow-list at session start.
-   Tool permission prompts are bypassed for allow-listed tools.
-   Non allow-listed tools are rejected before execution, and their attempts are logged.
-   Git diff confirmations and other safety prompts are skipped automatically.
-   If the acknowledgement profile is missing (while required), the CLI aborts before launching.

## Customising the Allow-List

```toml
[automation.full_auto]
enabled = true
require_profile_ack = true
profile_path = "automation/full_auto_profile.toml"
allowed_tools = [
    "read_file",
    "list_files",
    "grep_file",
    "simple_search",
    "run_terminal_cmd", # optionally include write or shell tools
]
```

Tips:

-   Use the constants listed in `vtcode_core::config::constants::tools` to avoid typos.
-   Include `"*"` to allow every registered tool (not recommended unless the workspace is fully
    isolated).
-   Combine with tool policies if you need per-tool constraints or prompts even in full-auto mode.

## Profile File Recommendations

The profile file is a simple acknowledgement document. Suggested content:

-   Operator name and timestamp approving autonomous execution.
-   Workspace-specific limitations (e.g., directories that must not be modified).
-   Contact or escalation details if the automation encounters unexpected failures.
-   Rollback procedures or monitoring steps to follow after autonomous runs.

Keeping this file under version control provides a clear audit trail for when full-auto mode was
used and under which guardrails.
