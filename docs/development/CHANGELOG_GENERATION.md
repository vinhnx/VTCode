# Changelog Generation with git-cliff

VT Code uses [git-cliff](https://git-cliff.org) for automated changelog generation from Git commit history. This provides consistent, well-formatted changelogs that follow conventional commit standards.

## Installation

Install git-cliff using Cargo:

```bash
cargo install git-cliff
```

Or use it via Docker without installation:

```bash
docker run --rm -v "$(pwd):/app" -w /app ghcr.io/orhunp/git-cliff:latest --config cliff.toml
```

## Configuration

The configuration file `cliff.toml` is located at the project root. It defines:

- **Commit parsing rules**: How to parse conventional commits
- **Grouping**: How to organize commits by type (Features, Bug Fixes, etc.)
- **Filtering**: Which commits to exclude (version bumps, release commits)
- **Template**: The Markdown format for changelog entries

### Commit Types

The following conventional commit types are recognized:

| Type | Section | Included |
|------|---------|----------|
| `feat` | Features | ✅ |
| `fix` | Bug Fixes | ✅ |
| `perf` | Performance | ✅ |
| `refactor` | Refactors | ✅ |
| `security` | Security | ✅ |
| `docs` | Documentation | ✅ |
| `test` | Tests | ✅ |
| `build` | Build | ✅ |
| `ci` | CI | ✅ |
| `deps` | Dependencies | ✅ |
| `chore` | Chores | ❌ (excluded) |

## Usage

### Authentication

git-cliff can operate in two modes:

**Online mode (recommended):** Fetches GitHub usernames from commit metadata
```bash
export GITHUB_TOKEN=$(gh auth token)  # Get token from gh CLI
git-cliff --config cliff.toml --tag "0.82.4" --unreleased
```

Benefits:
- Shows actual GitHub usernames with @ prefix (e.g., `@vinhnx`)
- Works with outside contributors automatically
- No hardcoded email mappings needed

**Offline mode:** Uses git commit author name (no API calls)
```bash
git-cliff --config cliff.toml --tag "0.82.4" --unreleased
```

Note: Offline mode shows the full git author name as-is (e.g., `Vinh Nguyen`).

The release script automatically detects and uses your GitHub token if available.

### Generate Changelog for a Release

```bash
# Generate changelog for a specific version (for releases)
git-cliff --config cliff.toml --tag "0.82.4" --unreleased --output CHANGELOG.md

# Preview before writing
git-cliff --config cliff.toml --tag "0.82.4" --unreleased
```

### Generate Full Changelog

```bash
# Generate complete changelog from all tags
git-cliff --config cliff.toml --output CHANGELOG.md

# Preview without writing to file
git-cliff --config cliff.toml
```

### Generate Recent Versions

```bash
# Last 3 versions
git-cliff --config cliff.toml --latest 3

# Last version only
git-cliff --config cliff.toml --latest 1
```

### Custom Range

```bash
# Specific tag range
git-cliff --config cliff.toml v0.80.0..v0.82.0

# From specific tag to HEAD
git-cliff --config cliff.toml v0.80.0..HEAD
```

## Integration with Release Process

The release script (`scripts/release.sh`) automatically uses git-cliff when available:

1. **Check for git-cliff**: Script checks if `git-cliff` is installed
2. **Generate changelog**: Creates formatted changelog entry for the new version
3. **Generate release notes**: Creates GitHub Release body from changelog
4. **Fallback**: If git-cliff is not available, uses built-in changelog generator

### Release Workflow

```bash
# Dry run to preview changelog
./scripts/release.sh --patch --dry-run

# Create release (git-cliff will be used automatically)
./scripts/release.sh --patch
```

## Commit Message Format

VT Code follows [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

### Examples

```bash
# Feature
feat(wizard): implement freeform text input for wizard modals

# Bug fix
fix(indexer): resolve memory leak in document processing

# Refactor
refactor(core): simplify agent state management

# Documentation
docs(prompts): enhance system prompt guidelines

# Performance
perf(search): optimize tree-sitter parsing speed
```

## Excluded Commits

The following commit patterns are automatically excluded from the changelog:

- `chore(release):` - Release automation commits
- `bump version` - Version number updates
- `update version` - Version-related updates
- `release v*` - Release tag commits
- `update homebrew` - Homebrew formula updates
- `update changelog` - Changelog update commits

## Customization

To customize the changelog format, edit `cliff.toml`:

### Add New Commit Type

```toml
[git.commit_parsers]
{ message = "^style", group = "Styles" },
```

### Change Section Order

Edit the order in the template `body` section:

```tera
{% for group, commits in commits | group_by(attribute="group") %}
    ### {{ group }}
    {% for commit in commits %}
        - {{ commit.message }}
    {% endfor %}
{% endfor %}
```

### Modify Output Format

The template uses [Tera](https://keats.github.io/tera/) templating:

```tera
## {{ version }} - {{ timestamp | date(format="%Y-%m-%d") }}

{% for group, commits in commits | group_by(attribute="group") %}
    ### {{ group }}
    {% for commit in commits %}
        - {{ commit.message | upper_first }} ({% if commit.remote.username %}@{{ commit.remote.username }}{% endif %})
    {% endfor %}
{% endfor %}
```

## Troubleshooting

### git-cliff Not Found

```bash
# Install git-cliff
cargo install git-cliff

# Verify installation
git-cliff --version
```

### Incorrect Commit Grouping

Check that commit messages follow conventional commit format:

```bash
# View recent commits
git log --oneline -10

# Test parsing with git-cliff
git-cliff --config cliff.toml --unreleased
```

### Missing GitHub Username

git-cliff fetches GitHub usernames from commit metadata. Ensure:

1. Commits are associated with a GitHub account
2. Git remote is properly configured
3. Network access is available for API calls

## Resources

- [git-cliff Documentation](https://git-cliff.org/docs/)
- [Configuration Guide](https://git-cliff.org/docs/configuration)
- [Conventional Commits](https://www.conventionalcommits.org/)
- [Tera Templating](https://keats.github.io/tera/docs/)
