# Dotfile Protection System

VT Code includes a comprehensive dotfile protection system that prevents automatic or implicit modification of hidden configuration files (dotfiles) by AI agents or automated tools.

## Overview

Dotfiles are critical configuration files that control development tools, shell environments, cloud credentials, and security settings. Unintentional modifications can:

- Expose credentials and secrets
- Break development environments  
- Cause security vulnerabilities
- Lead to hard-to-debug issues

The dotfile protection system ensures that any modification to these sensitive files requires explicit user confirmation.

## Features

### 1. Explicit User Confirmation

Any attempt to modify a protected dotfile triggers a detailed confirmation prompt showing:

- The exact file being modified
- The tool/operation requesting access
- A preview of proposed changes
- Why the file is protected

### 2. Immutable Audit Logging

All dotfile access attempts are logged with:

- Timestamps (UTC)
- Access type (read/write/modify/delete)
- Outcome (allowed/blocked/denied)
- Tool that initiated the access
- Session identifier
- Cryptographic hash chain for tamper detection

Audit logs are stored at `~/.vtcode/dotfile_audit.log` by default.

### 3. Whitelist with Secondary Authentication

Specific dotfiles can be whitelisted for modification, but still require secondary authentication to prevent accidental changes.

### 4. Cascade Prevention

The system blocks cascading modifications where changing one dotfile triggers automatic updates to others. Each dotfile must be modified independently with explicit confirmation.

### 5. Automatic Backups

Before any permitted modification, the system creates a backup preserving:

- Original file content
- File permissions
- SHA-256 content hash for integrity verification

Backups are stored at `~/.vtcode/dotfile_backups/` with rotation (default: 10 backups per file).

### 6. Permission Preservation

Original file permissions and ownership are preserved across modifications.

## Protected Files

The following dotfiles are protected by default:

### Git Configuration
- `.gitignore`, `.gitattributes`, `.gitmodules`, `.gitconfig`, `.git-credentials`

### Environment Files
- `.env`, `.env.local`, `.env.development`, `.env.production`, `.env.test`, `.env.*`

### Shell Configuration
- `.zshrc`, `.bashrc`, `.bash_profile`, `.profile`, `.zshenv`, `.zsh_history`

### Editor Configuration
- `.editorconfig`, `.vscode/*`, `.idea/*`, `.cursor/*`, `.vimrc`, `.vim/*`, `.nvim/*`

### Security & Credentials
- `.ssh/*`, `.gnupg/*`, `.aws/*`, `.azure/*`, `.kube/*`, `.docker/*`
- `.npmrc`, `.pypirc`, `.cargo/credentials.toml`, `.netrc`

### Code Formatting & Linting
- `.prettierrc*`, `.eslintrc*`, `.babelrc*`, `.stylelintrc*`

### Docker
- `.dockerignore`, `.docker/*`

### And many more...

See `vtcode-config/src/core/dotfile_protection.rs` for the complete list.

## Configuration

Configure dotfile protection in `vtcode.toml`:

```toml
[dotfile_protection]
# Enable/disable the entire system (default: true)
enabled = true

# Require explicit confirmation for modifications (default: true)
require_explicit_confirmation = true

# Enable audit logging (default: true)
audit_logging_enabled = true

# Path to audit log (default: ~/.vtcode/dotfile_audit.log)
audit_log_path = "~/.vtcode/dotfile_audit.log"

# Prevent cascading modifications (default: true)
prevent_cascading_modifications = true

# Create backups before modifications (default: true)
create_backups = true

# Backup directory (default: ~/.vtcode/dotfile_backups)
backup_directory = "~/.vtcode/dotfile_backups"

# Maximum backups per file (default: 10)
max_backups_per_file = 10

# Preserve original permissions (default: true)
preserve_permissions = true

# Block modifications during automated operations (default: true)
block_during_automation = true

# Require secondary auth for whitelisted files (default: true)
require_secondary_auth_for_whitelist = true

# Whitelist specific files (require secondary confirmation)
whitelist = [".gitignore", ".prettierrc"]

# Additional patterns to protect
additional_protected_patterns = [".myconfig", ".custom/*"]

# Operations that block dotfile modifications
blocked_operations = [
    "dependency_installation",
    "code_formatting",
    "git_operations",
    "project_initialization",
    "build_operations",
    "test_execution",
    "linting",
    "auto_fix"
]
```

## How It Works

### Protection Flow

```
File Modification Request
         │
         ▼
    Is file a dotfile?
         │
    ┌────┴────┐
    No       Yes
    │         │
    ▼         ▼
  Allow   Check protection
            │
         ┌──┴──┐
         │     │
         ▼     ▼
      Cascade? Automated?
         │         │
         ▼         ▼
       Block    Block
         │
         ▼
    Whitelisted?
         │
    ┌────┴────┐
    No       Yes
    │         │
    ▼         ▼
  Confirm   Secondary Auth
    │         │
    └────┬────┘
         ▼
    User Decision
         │
    ┌────┴────┐
    Reject  Approve
    │         │
    ▼         ▼
   Log     Create Backup
            │
            ▼
         Allow Modify
            │
            ▼
          Log Access
```

### Audit Log Format

Each audit entry is a JSON object:

```json
{
  "id": "uuid",
  "timestamp": "2025-01-31T12:00:00Z",
  "file_path": ".gitignore",
  "access_type": "write",
  "outcome": "allowed_with_confirmation",
  "initiator": "write_file",
  "session_id": "session-abc123",
  "proposed_changes": "Adding node_modules to ignore",
  "previous_hash": "...",
  "entry_hash": "...",
  "context": null,
  "during_automation": false
}
```

### Backup Structure

```
~/.vtcode/dotfile_backups/
├── backups.json                    # Index of all backups
├── _gitignore.20250131_120000_000.backup
├── _gitignore.20250131_120500_000.backup
├── _env.20250131_121000_000.backup
└── ...
```

## API Usage

### Programmatic Access

```rust
use vtcode_core::dotfile_protection::{
    DotfileGuardian, AccessContext, AccessType, ProtectionDecision,
};
use vtcode_config::core::DotfileProtectionConfig;

// Create guardian with default configuration
let config = DotfileProtectionConfig::default();
let guardian = DotfileGuardian::new(config).await?;

// Check if a file is protected
if guardian.is_protected(Path::new(".gitignore")) {
    // Request access
    let context = AccessContext::new(
        ".gitignore",
        AccessType::Write,
        "my_tool",
        "session-123"
    ).with_proposed_changes("Adding node_modules");
    
    match guardian.request_access(&context).await? {
        ProtectionDecision::Allowed => {
            // Proceed with modification
        }
        ProtectionDecision::RequiresConfirmation(req) => {
            // Show confirmation dialog to user
            if user_confirms() {
                guardian.confirm_modification(&context, false).await?;
                // Proceed with modification
            } else {
                guardian.reject_modification(&context).await?;
            }
        }
        ProtectionDecision::Blocked(violation) => {
            // Cannot modify - show error
            eprintln!("{}", violation);
        }
        _ => {}
    }
}
```

### Restoring from Backup

```rust
// Get latest backup
if let Some(backup) = guardian.get_latest_backup(Path::new(".gitignore")).await? {
    println!("Backup created at: {}", backup.created_at);
    println!("Original hash: {}", backup.content_hash);
    
    // Restore
    backup.restore().await?;
}
```

### Verifying Audit Log Integrity

```rust
if guardian.verify_audit_integrity().await? {
    println!("Audit log integrity verified");
} else {
    eprintln!("WARNING: Audit log may have been tampered with!");
}
```

## Best Practices

1. **Keep protection enabled** - The system is designed to prevent accidental damage
2. **Review audit logs periodically** - Check for unexpected access patterns
3. **Use whitelist sparingly** - Only whitelist files that need frequent modification
4. **Don't disable for automation** - If tools need to modify dotfiles, do it explicitly
5. **Backup before major changes** - The system does this automatically, but manual backups help

## Troubleshooting

### "Dotfile modification blocked during automation"

This occurs when an automated tool (npm install, cargo build, etc.) tries to modify a dotfile. This is intentional. Modify dotfiles manually or disable `block_during_automation` (not recommended).

### "Cascading modification blocked"

You've already modified one dotfile and are trying to modify another in the same session. Either:
- Reset the session with `/reset` or restart VT Code
- Modify each file in a separate session

### Audit log integrity failure

The audit log may have been tampered with. Check:
1. File permissions on `~/.vtcode/dotfile_audit.log`
2. Whether any external process modified the file
3. Consider starting a fresh audit log (backup the old one)

### Can't restore from backup

Ensure:
1. The backup file still exists
2. You have write permissions to the original file location
3. The backup hash matches (file wasn't corrupted)

## Security Considerations

- Audit logs contain hashes, not actual file contents
- Backup files preserve original permissions
- The system cannot be bypassed by MCP tools or subagents
- All access is logged regardless of outcome
