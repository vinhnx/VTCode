# Command Safety Examples: VT Code + Codex Patterns

This document shows practical examples of the new command safety module in action.

## Basic Usage

```rust
use vtcode_core::command_safety::{SafeCommandRegistry, is_safe_command};

let registry = SafeCommandRegistry::new();

// Evaluate a command
let cmd = vec!["git".to_string(), "status".to_string()];
if is_safe_command(&registry, &cmd) {
    println!("Safe to execute");
} else {
    println!("Blocked for safety reasons");
}
```

## Git Safety Patterns

### ✅ Safe Commands
```rust
let registry = SafeCommandRegistry::new();

// Read-only operations are safe
assert!(is_safe_command(&registry, &["git", "status"]));
assert!(is_safe_command(&registry, &["git", "log"]));
assert!(is_safe_command(&registry, &["git", "diff", "HEAD"]));
assert!(is_safe_command(&registry, &["git", "show", "v1.0"]));
assert!(is_safe_command(&registry, &["git", "branch", "-a"]));
```

### ❌ Dangerous Commands
```rust
let registry = SafeCommandRegistry::new();

// Destructive operations are blocked
assert!(!is_safe_command(&registry, &["git", "reset", "--hard"]));
assert!(!is_safe_command(&registry, &["git", "rm", "file.txt"]));
assert!(!is_safe_command(&registry, &["git", "clean", "-fdx"]));
```

## Cargo Safety Patterns

### ✅ Safe Commands
```rust
let registry = SafeCommandRegistry::new();

// Read-only checks
assert!(is_safe_command(&registry, &["cargo", "check"]));
assert!(is_safe_command(&registry, &["cargo", "build"]));
assert!(is_safe_command(&registry, &["cargo", "clippy"]));

// Format with verification
assert!(is_safe_command(&registry, &[
    "cargo", "fmt", "--check"
]));
```

### ❌ Dangerous Commands
```rust
let registry = SafeCommandRegistry::new();

// Destructive operations
assert!(!is_safe_command(&registry, &["cargo", "clean"]));
assert!(!is_safe_command(&registry, &["cargo", "install"]));

// Format without verification
assert!(!is_safe_command(&registry, &["cargo", "fmt"]));
```

## Find Command Safety Patterns

### ✅ Safe Usage
```rust
let registry = SafeCommandRegistry::new();

// Simple search without dangerous operations
assert!(is_safe_command(&registry, &[
    "find", ".", "-name", "*.rs"
]));

assert!(is_safe_command(&registry, &[
    "find", "/path", "-type", "f", "-size", "+100m"
]));
```

### ❌ Dangerous Options
```rust
let registry = SafeCommandRegistry::new();

// Deletion is blocked
assert!(!is_safe_command(&registry, &[
    "find", ".", "-name", "*.tmp", "-delete"
]));

// Arbitrary command execution is blocked
assert!(!is_safe_command(&registry, &[
    "find", ".", "-exec", "rm", "{}", ";"
]));

// File writing is blocked
assert!(!is_safe_command(&registry, &[
    "find", ".", "-fprint", "results.txt"
]));
```

## Sed Command Safety Patterns

### ✅ Safe Usage (Read-only Print)
```rust
let registry = SafeCommandRegistry::new();

// Print specific lines
assert!(is_safe_command(&registry, &[
    "sed", "-n", "10p", "file.txt"
]));

// Print range
assert!(is_safe_command(&registry, &[
    "sed", "-n", "1,5p", "file.txt"
]));
```

### ❌ Dangerous Usage (Modification)
```rust
let registry = SafeCommandRegistry::new();

// In-place substitution is blocked
assert!(!is_safe_command(&registry, &[
    "sed", "s/foo/bar/g", "file.txt"
]));

// Without -n flag for printing
assert!(!is_safe_command(&registry, &[
    "sed", "10p", "file.txt"
]));
```

## Base64 Command Safety Patterns

### ✅ Safe Usage (Stdout)
```rust
let registry = SafeCommandRegistry::new();

// Output to stdout
assert!(is_safe_command(&registry, &[
    "base64", "file.txt"
]));
```

### ❌ Dangerous Usage (Output Redirection)
```rust
let registry = SafeCommandRegistry::new();

// Output to file is blocked
assert!(!is_safe_command(&registry, &[
    "base64", "file.txt", "-o", "output.txt"
]));

// Using --output flag
assert!(!is_safe_command(&registry, &[
    "base64", "file.txt", "--output=output.txt"
]));
```

## Ripgrep Command Safety Patterns

### ✅ Safe Usage
```rust
let registry = SafeCommandRegistry::new();

// Simple search
assert!(is_safe_command(&registry, &[
    "rg", "pattern", "."
]));

// With options
assert!(is_safe_command(&registry, &[
    "rg", "--type", "rs", "TODO", "src/"
]));
```

### ❌ Dangerous Usage
```rust
let registry = SafeCommandRegistry::new();

// Arbitrary command preprocessing is blocked
assert!(!is_safe_command(&registry, &[
    "rg", "--pre", "gunzip", "pattern"
]));

// Hostname discovery is blocked
assert!(!is_safe_command(&registry, &[
    "rg", "--hostname-bin", "hostname", "pattern"
]));

// Archive search is blocked out of abundance of caution
assert!(!is_safe_command(&registry, &[
    "rg", "-z", "pattern"
]));
```

## Windows/PowerShell Safety Patterns

### ❌ Dangerous PowerShell Invocations
```rust
use vtcode_core::command_safety::command_might_be_dangerous;

// Launching URL with Start-Process
assert!(command_might_be_dangerous(&[
    "powershell".to_string(),
    "Start-Process 'https://example.com'".to_string(),
]));

// Browser with URL
assert!(command_might_be_dangerous(&[
    "powershell".to_string(),
    "firefox https://example.com".to_string(),
]));

// ShellExecute with URL
assert!(command_might_be_dangerous(&[
    "powershell".to_string(),
    "[System.Diagnostics.Process]::Start('https://example.com')".to_string(),
]));
```

### ✅ Safe PowerShell Commands
```rust
use vtcode_core::command_safety::command_might_be_dangerous;

// Simple text output
assert!(!command_might_be_dangerous(&[
    "powershell".to_string(),
    "Write-Host 'hello'".to_string(),
]));

// No URLs = safe
assert!(!command_might_be_dangerous(&[
    "powershell".to_string(),
    "Start-Process notepad.exe".to_string(),
]));
```

## Dangerous Commands (Hardcoded Blocks)

### System-Level Dangers
```rust
use vtcode_core::command_safety::command_might_be_dangerous;

// File system destruction
assert!(command_might_be_dangerous(&["rm".to_string(), "-rf".to_string(), "/".to_string()]));

// Disk operations
assert!(command_might_be_dangerous(&["dd".to_string(), "if=/dev/zero".to_string()]));
assert!(command_might_be_dangerous(&["mkfs".to_string()]));

// System control
assert!(command_might_be_dangerous(&["shutdown".to_string()]));
assert!(command_might_be_dangerous(&["reboot".to_string()]));
```

### Privilege Escalation
```rust
use vtcode_core::command_safety::command_might_be_dangerous;

// Sudo wrapping checks the inner command
assert!(command_might_be_dangerous(&[
    "sudo".to_string(),
    "git".to_string(),
    "reset".to_string(),
]));

// But safe commands are still safe
assert!(!command_might_be_dangerous(&[
    "sudo".to_string(),
    "git".to_string(),
    "status".to_string(),
]));
```

## Shell Script Parsing (bash -lc)

```rust
use vtcode_core::command_safety::shell_parser::parse_bash_lc_commands;

let cmd = vec![
    "bash".to_string(),
    "-lc".to_string(),
    "git status && cargo check".to_string(),
];

if let Some(commands) = parse_bash_lc_commands(&cmd) {
    // Returns: [["git", "status"], ["cargo", "check"]]
    assert_eq!(commands.len(), 2);
    
    // Each can be checked independently
    for subcmd in commands {
        // evaluate each command for safety
    }
}
```

## Integration with Policy Evaluator (Phase 5)

```rust
// Future integration (Phase 5):
// The command_safety registry will be integrated with
// CommandPolicyEvaluator for comprehensive safety checking

use vtcode_core::command_safety::SafeCommandRegistry;
use vtcode_core::tools::CommandPolicyEvaluator;

let registry = SafeCommandRegistry::new();
let policy = CommandPolicyEvaluator::from_config(&config);

// Enhanced evaluation that checks both:
// 1. Deny-list/allow-list from policy
// 2. Subcommand rules from registry
// 3. Option blacklists from registry
// 4. Dangerous command detection
```

## Error Handling Example

```rust
use vtcode_core::command_safety::{SafeCommandRegistry, SafetyDecision};

let registry = SafeCommandRegistry::new();
let cmd = vec!["find".to_string(), ".".to_string(), "-delete".to_string()];

match registry.is_safe(&cmd) {
    SafetyDecision::Allow => {
        println!("Command is safe");
    }
    SafetyDecision::Deny(reason) => {
        eprintln!("Command blocked: {}", reason);
        // Example: "Option -delete is not allowed for find"
    }
    SafetyDecision::Unknown => {
        // Defer to policy evaluator or other checks
        println!("Safety status unknown, using other rules");
    }
}
```

## Testing with Custom Commands

```rust
#[test]
fn my_custom_safety_test() {
    let registry = SafeCommandRegistry::new();
    
    // Test your own commands
    let cmd = vec!["mycommand".to_string(), "arg1".to_string()];
    
    // Result will be SafetyDecision::Unknown if not in registry
    // Then policy evaluator takes over in Phase 5
    let result = registry.is_safe(&cmd);
    assert_eq!(result, SafetyDecision::Unknown);
}
```

## References

- **Module Documentation:** `docs/CODEX_COMMAND_SAFETY_INTEGRATION.md`
- **Implementation Plan:** 5-phase roadmap with timeline
- **Source Code:** `vtcode-core/src/command_safety/`
