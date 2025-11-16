# Command Allowlist & Audit Policy

This document outlines recommended safe commands to include in `[commands].allow_list` and how to use the audit system to log and retain permission decisions.

## Goals

-   Permit common, low-risk developer tools and commands (build, test, inspection) without prompting.
-   Require explicit confirmation for destructive or publish-like operations (e.g., `git reset --hard`, `git push --force`, `cargo publish`).
-   Log all decisions for auditability, retention, and incident analysis.

## Recommended Allowlist Strategy

-   Keep `allow_list` minimal, with base verbs such as `ls`, `pwd`, `git`, `cargo`, `python`, `npm`.
-   Use `allow_glob` for broader ecosystems that require many subcommands, e.g. `git *`, `cargo *`, `npm *`.
-   Avoid adding dangerous commands to `allow_list` explicitly (e.g., `rm`, `sudo`). Rely on `deny_list` and regex matching for disallowed operations.
-   Examples:
    -   Minimal `allow_list`: `ls`, `pwd`, `echo`, `git`, `cargo`
    -   Useful `allow_glob` patterns: `git *`, `cargo *`, `node *`, `npm *`.

## Confirmation Policy (`confirm=true`)

-   For any operation where `execpolicy` detects destructive flags, the agent will require `confirm=true` in the `EnhancedTerminalInput` or equivalent.
-   Example scenarios:
    -   `git reset --hard` — require confirm
    -   `git push --force` — require confirm
    -   `cargo publish` — require confirm
    -   `docker run --privileged` — require confirm

## Dry-Run / Pre-Flight Audits

-   Prefer a dry-run pattern where possible.
    -   Git: `git status` and `git diff` before destructive operations.
    -   Cargo: `cargo build` and `cargo check` for build-related changes before `publish`.
-   The agent should show the diff and require explicit `confirm` to proceed.

## Audit Logging & Retention

-   All permission decisions are written to `~/.vtcode/audit/permissions-YYYY-MM-DD.log` in JSON format by default.
-   Recommended retention policy:
    -   Keep logs for at least 30 days for investigation and audit.
    -   Optionally rotate and compress older logs monthly.
    -   Optionally ship logs to a centralized audit store for long-term retention (e.g., cloud or local corporate log server).
-   To enable audit logging, ensure `permissions.audit_enabled = true` in `vtcode.toml`.

## Best Practices

-   Use `allow_glob` where feasible and keep `allow_list` minimal
-   Prefer `confirm=true` for destructive actions and require dry-run when possible
-   Use the audit logs to investigate accidental or malicious actions and configure retention
-   Keep deny lists robust; explicitly deny dangerous operations (e.g., `rm -rf`, `sudo`)
