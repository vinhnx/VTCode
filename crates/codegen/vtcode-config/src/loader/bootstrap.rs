use std::fs;
use std::path::{Path, PathBuf};
use vtcode_commons::VtCodePaths;

use anyhow::{Context, Result};

use super::config::VTCodeConfig;
use crate::defaults::{self, ConfigDefaultsProvider};

const DEFAULT_GITIGNORE_FILE_NAME: &str = ".vtcodegitignore";

/// Determine where configuration and gitignore files should be created when
/// bootstrapping a workspace.
fn determine_bootstrap_targets(
    workspace: &Path,
    use_home_dir: bool,
    config_file_name: &str,
    defaults_provider: &dyn ConfigDefaultsProvider,
) -> Result<(PathBuf, PathBuf)> {
    if use_home_dir {
        if let Some(home_config_path) = select_home_config_path(defaults_provider, config_file_name)? {
            let gitignore_path = gitignore_path_for(&home_config_path);
            return Ok((home_config_path, gitignore_path));
        }
    }

    let config_path = workspace.join(config_file_name);
    let gitignore_path = workspace.join(DEFAULT_GITIGNORE_FILE_NAME);
    Ok((config_path, gitignore_path))
}

/// Returns the preferred gitignore path for a given configuration file.
fn gitignore_path_for(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|parent| parent.join(DEFAULT_GITIGNORE_FILE_NAME))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_GITIGNORE_FILE_NAME))
}

/// Ensures the parent directory for the provided path exists, creating it if
/// necessary.
fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    Ok(())
}

/// Ensures a canonical user-level configuration parent exists privately.
fn ensure_private_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        let _ = VtCodePaths::ensure_user_dir(parent)
            .with_context(|| format!("Failed to create private directory: {}", parent.display()))?;
    }

    Ok(())
}

/// Selects the canonical user configuration path from the defaults provider.
fn select_home_config_path(
    defaults_provider: &dyn ConfigDefaultsProvider,
    config_file_name: &str,
) -> Result<Option<PathBuf>> {
    defaults_provider
        .canonical_user_config_path(config_file_name)
        .context("Failed to resolve canonical user configuration path")
}

impl VTCodeConfig {
    /// Bootstrap project with config + gitignore
    pub fn bootstrap_project<P: AsRef<Path>>(workspace: P, force: bool) -> Result<Vec<String>> {
        Self::bootstrap_project_with_options(workspace, force, false)
    }

    /// Bootstrap project with config + gitignore, with option to create in home directory
    pub fn bootstrap_project_with_options<P: AsRef<Path>>(
        workspace: P,
        force: bool,
        use_home_dir: bool,
    ) -> Result<Vec<String>> {
        let workspace = workspace.as_ref().to_path_buf();
        defaults::with_config_defaults(|provider| {
            Self::bootstrap_project_with_provider(&workspace, force, use_home_dir, provider)
        })
    }

    /// Bootstrap project files using the supplied [`ConfigDefaultsProvider`].
    fn bootstrap_project_with_provider<P: AsRef<Path>>(
        workspace: P,
        force: bool,
        use_home_dir: bool,
        defaults_provider: &dyn ConfigDefaultsProvider,
    ) -> Result<Vec<String>> {
        let workspace = workspace.as_ref();
        let config_file_name = defaults_provider.config_file_name().to_string();
        let (config_path, gitignore_path) =
            determine_bootstrap_targets(workspace, use_home_dir, &config_file_name, defaults_provider)?;
        let vtcode_readme_path = workspace.join(".vtcode").join("README.md");
        let ast_grep_config_path = workspace.join("sgconfig.yml");
        let ast_grep_rule_path = workspace.join("rules").join("examples").join("no-console-log.yml");
        let ast_grep_test_path = workspace.join("rule-tests").join("examples").join("no-console-log-test.yml");
        let ast_grep_snapshot_path = workspace
            .join("rule-tests")
            .join("__snapshots__")
            .join("no-console-log-snapshot.yml");

        let home_targets = use_home_dir && config_path != workspace.join(&config_file_name);
        if home_targets {
            ensure_private_parent_dir(&config_path)?;
            ensure_private_parent_dir(&gitignore_path)?;
        } else {
            ensure_parent_dir(&config_path)?;
            ensure_parent_dir(&gitignore_path)?;
        }
        ensure_parent_dir(&vtcode_readme_path)?;
        ensure_parent_dir(&ast_grep_config_path)?;
        ensure_parent_dir(&ast_grep_rule_path)?;
        ensure_parent_dir(&ast_grep_test_path)?;
        ensure_parent_dir(&ast_grep_snapshot_path)?;

        let mut created_files = Vec::new();

        if !config_path.exists() || force {
            let config_content = Self::default_vtcode_toml_template();

            fs::write(&config_path, config_content)
                .with_context(|| format!("Failed to write config file: {}", config_path.display()))?;

            if let Some(file_name) = config_path.file_name().and_then(|name| name.to_str()) {
                created_files.push(file_name.to_string());
            }
        }

        if !gitignore_path.exists() || force {
            let gitignore_content = Self::default_vtcode_gitignore();
            fs::write(&gitignore_path, gitignore_content)
                .with_context(|| format!("Failed to write gitignore file: {}", gitignore_path.display()))?;

            if let Some(file_name) = gitignore_path.file_name().and_then(|name| name.to_str()) {
                created_files.push(file_name.to_string());
            }
        }

        if !vtcode_readme_path.exists() || force {
            let vtcode_readme = Self::default_vtcode_readme_template();
            fs::write(&vtcode_readme_path, vtcode_readme)
                .with_context(|| format!("Failed to write VT Code README: {}", vtcode_readme_path.display()))?;
            created_files.push(".vtcode/README.md".to_string());
        }

        let ast_grep_files = [
            (&ast_grep_config_path, Self::default_ast_grep_config_template(), "sgconfig.yml"),
            (
                &ast_grep_rule_path,
                Self::default_ast_grep_example_rule_template(),
                "rules/examples/no-console-log.yml",
            ),
            (
                &ast_grep_test_path,
                Self::default_ast_grep_example_test_template(),
                "rule-tests/examples/no-console-log-test.yml",
            ),
            (
                &ast_grep_snapshot_path,
                Self::default_ast_grep_example_snapshot_template(),
                "rule-tests/__snapshots__/no-console-log-snapshot.yml",
            ),
        ];

        for (path, contents, label) in ast_grep_files {
            if !path.exists() || force {
                fs::write(path, contents)
                    .with_context(|| format!("Failed to write ast-grep scaffold file: {}", path.display()))?;
                created_files.push(label.to_string());
            }
        }

        Ok(created_files)
    }

    /// Generate the default `vtcode.toml` template used by bootstrap helpers.
    fn default_vtcode_toml_template() -> String {
        include_str!("../../data/default_config.toml").to_owned()
    }

    fn default_vtcode_gitignore() -> String {
        r#"# Security-focused exclusions
.env, .env.local, secrets/, .aws/, .ssh/

# Development artifacts
target/, build/, dist/, node_modules/, vendor/

# Database files
*.db, *.sqlite, *.sqlite3

# Binary files
*.exe, *.dll, *.so, *.dylib, *.bin

# IDE files (comprehensive)
.vscode/, .idea/, *.swp, *.swo
"#
        .to_string()
    }

    fn default_vtcode_readme_template() -> &'static str {
        "# VT Code Workspace Files\n\n- Put always-on repository guidance in `AGENTS.md`.\n- Put path-scoped prompt rules in `.vtcode/rules/*.md` using YAML frontmatter.\n- Keep authoring notes and other workspace docs outside `.vtcode/rules/` so they are not loaded into prompt memory.\n"
    }

    fn default_ast_grep_config_template() -> &'static str {
        "ruleDirs:\n  - rules\nutilDirs:\n  - utils\ntestConfigs:\n  - testDir: rule-tests\n    snapshotDir: __snapshots__\n"
    }

    fn default_ast_grep_example_rule_template() -> &'static str {
        "id: no-console-log\nlanguage: JavaScript\nseverity: error\nmessage: Avoid `console.log` in checked JavaScript files.\nnote: |\n  Avoid `console.log` in checked JavaScript files.\nrule:\n  pattern: console.log($$$ARGS)\nfiles:\n  - '**/*.js'\n"
    }

    fn default_ast_grep_example_test_template() -> &'static str {
        "id: no-console-log\nvalid:\n  - |\n    const logger = {\n      info(message) {\n        return message;\n      },\n    };\ninvalid:\n  - |\n    function greet(name) {\n      console.log(name);\n    }\n"
    }

    fn default_ast_grep_example_snapshot_template() -> &'static str {
        "id: no-console-log\nsnapshots:\n  ? |\n    function greet(name) {\n      console.log(name);\n    }\n  : labels:\n    - source: console.log(name)\n      style: primary\n      start: 25\n      end: 42\n"
    }

    /// Create sample configuration file
    pub fn create_sample_config<P: AsRef<Path>>(output: P) -> Result<()> {
        let output = output.as_ref();
        let config_content = Self::default_vtcode_toml_template();

        fs::write(output, config_content)
            .with_context(|| format!("Failed to write config file: {}", output.display()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests;
