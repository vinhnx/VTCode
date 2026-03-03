# Cargo-Unmaintained

Guide for using `cargo-unmaintained` to detect unmaintained dependencies in VT Code.

## Overview

[`cargo-unmaintained`](https://github.com/trailofbits/cargo-unmaintained) is a Rust tool that automatically finds unmaintained packages in Rust projects. It uses heuristics to detect unmaintained packages by checking:

1. **Archived repository** - The package's repository is archived
2. **Not a repository member** - The package is not a member of its named repository
3. **Stale dependencies** - The package depends on a package whose latest version is incompatible and was released over a year ago, and the package either has no repository or its last commit was over a year ago

## Installation

```bash
cargo install cargo-unmaintained
```

## Usage

### Basic Scan

Run a scan on the entire workspace:

```bash
cd /path/to/vtcode
cargo unmaintained
```

### Verbose Output

Get detailed progress information:

```bash
cargo unmaintained --verbose
```

### JSON Output

For programmatic usage or CI integration:

```bash
cargo unmaintained --json
```

### Check Specific Package

Scan only a specific package:

```bash
cargo unmaintained --package vtcode-core
```

### Show Dependency Paths

See which dependencies bring in unmaintained packages:

```bash
cargo unmaintained --tree
```

## Configuration

### Ignoring Packages

To ignore specific unmaintained packages, add them to your workspace's `Cargo.toml`:

```toml
[package.metadata.unmaintained]
ignore = ["package-name-1", "package-name-2"]
```

VT Code already includes this configuration section in `Cargo.toml` at the workspace root.

### GitHub Token (Optional)

To check if repositories are archived, you can set a GitHub token:

```bash
# Recommended: Path to file containing token
export GITHUB_TOKEN_PATH="$HOME/.github_token"

# Or: Direct token value (less secure)
export GITHUB_TOKEN="your_token_here"
```

Save token to config file:

```bash
cargo unmaintained --save-token
```

## Exit Codes

- `0` - No unmaintained packages found
- `1` - Unmaintained packages found
- `2` - Irrecoverable error occurred

## Common Options

| Option | Description |
|--------|-------------|
| `--color <WHEN>` | Color output: `always`, `auto`, or `never` (default: `auto`) |
| `--fail-fast` | Exit as soon as an unmaintained package is found |
| `--json` | Output JSON (experimental) |
| `--max-age <DAYS>` | Max age for repository commits (default: 365) |
| `--no-cache` | Disable disk caching |
| `--no-exit-code` | Don't set exit code on unmaintained packages |
| `--no-warnings` | Suppress warnings |
| `-p, --package <NAME>` | Check only a specific package |
| `--purge` | Remove cached data and exit |
| `--tree` | Show dependency paths to unmaintained packages |
| `--verbose` | Show detailed progress information |

## Integration with VT Code Development

### Pre-commit Check

Add to your pre-commit workflow to catch unmaintained dependencies early:

```bash
#!/bin/bash
# .git/hooks/pre-commit
cargo unmaintained --no-warnings
```

### CI/CD Integration

Add to your GitHub Actions workflow:

```yaml
- name: Check for unmaintained dependencies
  run: cargo unmaintained --no-warnings
```

### Periodic Audits

Run periodic audits to catch newly unmaintained packages:

```bash
# Monthly audit
cargo unmaintained --verbose --tree
```

## Troubleshooting

### GitHub API Rate Limits

If you see `401` or rate limit errors, set a GitHub token:

```bash
export GITHUB_TOKEN_PATH="$HOME/.github_token"
```

### Slow Scans

For large workspaces like VT Code, scans can take time. Use these options to speed up:

```bash
# Disable caching for fresh scan
cargo unmaintained --no-cache

# Suppress warnings for cleaner output
cargo unmaintained --no-warnings
```

### False Positives

Some packages may be flagged incorrectly. If a package is stable but flagged:

1. Check the package's repository activity
2. If it's actively maintained, add to ignore list in `Cargo.toml`
3. Consider opening an issue with the package maintainer

## Example Output

```
Scanning 632 packages and their dependencies
archival status of `some-crate` using GitHub API...ok (unarchived)
membership of `some-crate` using shallow clone...ok (member)
latest version of `another-crate` using crates.io index...ok (1.2.3)

`old-crate` appears to be unmaintained
  Repository: https://github.com/user/old-crate
  Last commit: 548 days ago
  Used by: vtcode-core -> dependency-chain -> old-crate
```

## Resources

- [cargo-unmaintained GitHub Repository](https://github.com/trailofbits/cargo-unmaintained)
- [crates.io page](https://crates.io/crates/cargo-unmaintained)
- [Trail of Bits Blog](https://www.trailofbits.com/)

## License

cargo-unmaintained is licensed under AGPLv3.
