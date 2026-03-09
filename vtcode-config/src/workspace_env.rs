use anyhow::{Context, Result, anyhow};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use tempfile::Builder;

pub fn read_workspace_env_value(workspace: &Path, env_key: &str) -> Result<Option<String>> {
    let env_path = workspace.join(".env");
    let iter = match dotenvy::from_path_iter(&env_path) {
        Ok(iter) => iter,
        Err(dotenvy::Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(err) => {
            return Err(anyhow!(err).context(format!("Failed to read {}", env_path.display())));
        }
    };

    for item in iter {
        let (key, value) = item
            .map_err(|err: dotenvy::Error| anyhow!(err))
            .with_context(|| format!("Failed to parse {}", env_path.display()))?;
        if key == env_key {
            if value.trim().is_empty() {
                return Ok(None);
            }
            return Ok(Some(value));
        }
    }

    Ok(None)
}

pub fn write_workspace_env_value(workspace: &Path, key: &str, value: &str) -> Result<()> {
    let env_path = workspace.join(".env");
    let mut lines = read_existing_lines(&env_path)?;
    upsert_env_line(&mut lines, key, value);

    let parent = env_path.parent().unwrap_or(workspace);
    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create directory {}", parent.display()))?;

    let temp = Builder::new()
        .prefix(".env.")
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| format!("Failed to create temporary file in {}", parent.display()))?;

    set_private_permissions(temp.as_file(), temp.path())?;

    {
        let mut writer = BufWriter::new(temp.as_file());
        for line in &lines {
            writeln!(writer, "{line}")
                .with_context(|| format!("Failed to write .env entry for {key}"))?;
        }
        writer
            .flush()
            .with_context(|| format!("Failed to flush temporary .env for {}", key))?;
    }

    temp.as_file()
        .sync_all()
        .with_context(|| format!("Failed to sync temporary .env for {}", key))?;

    let _persisted = temp
        .persist(&env_path)
        .with_context(|| format!("Failed to persist {}", env_path.display()))?;

    set_private_path_permissions(&env_path)?;
    Ok(())
}

fn read_existing_lines(env_path: &Path) -> Result<Vec<String>> {
    if !env_path.exists() {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(env_path)
        .with_context(|| format!("Failed to read {}", env_path.display()))?;
    Ok(contents.lines().map(|line| line.to_string()).collect())
}

fn upsert_env_line(lines: &mut Vec<String>, key: &str, value: &str) {
    let mut replaced = false;
    for line in lines.iter_mut() {
        if let Some((existing_key, _)) = line.split_once('=')
            && existing_key.trim() == key
        {
            *line = format!("{key}={value}");
            replaced = true;
        }
    }

    if !replaced {
        lines.push(format!("{key}={value}"));
    }
}

#[cfg(unix)]
fn set_private_permissions(file: &File, path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_permissions(_file: &File, _path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_path_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Failed to set permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_path_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{read_workspace_env_value, write_workspace_env_value};
    use anyhow::Result;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn read_returns_value_when_present() -> Result<()> {
        let dir = tempdir()?;
        fs::write(dir.path().join(".env"), "OPENAI_API_KEY=sk-test\n")?;

        let value = read_workspace_env_value(dir.path(), "OPENAI_API_KEY")?;

        assert_eq!(value, Some("sk-test".to_string()));
        Ok(())
    }

    #[test]
    fn read_returns_none_when_missing() -> Result<()> {
        let dir = tempdir()?;

        let value = read_workspace_env_value(dir.path(), "OPENAI_API_KEY")?;

        assert_eq!(value, None);
        Ok(())
    }

    #[test]
    fn write_adds_new_key() -> Result<()> {
        let dir = tempdir()?;

        write_workspace_env_value(dir.path(), "OPENAI_API_KEY", "sk-test")?;

        let contents = fs::read_to_string(dir.path().join(".env"))?;
        assert_eq!(contents, "OPENAI_API_KEY=sk-test\n");
        Ok(())
    }

    #[test]
    fn write_replaces_existing_key() -> Result<()> {
        let dir = tempdir()?;
        fs::write(
            dir.path().join(".env"),
            "OPENAI_API_KEY=old-value\nOTHER_KEY=value\n",
        )?;

        write_workspace_env_value(dir.path(), "OPENAI_API_KEY", "new-value")?;

        let contents = fs::read_to_string(dir.path().join(".env"))?;
        assert_eq!(contents, "OPENAI_API_KEY=new-value\nOTHER_KEY=value\n");
        Ok(())
    }
}
