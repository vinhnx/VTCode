use anyhow::Result;
use std::fs;
use std::path::Path;

use super::SimpleIndexerOptions;

/// Traverses a workspace directory tree while respecting [`SimpleIndexerOptions`].
pub struct DirectoryWalker<'a> {
    options: &'a SimpleIndexerOptions,
}

impl<'a> DirectoryWalker<'a> {
    /// Create a new walker for the provided options.
    pub fn new(options: &'a SimpleIndexerOptions) -> Self {
        Self { options }
    }

    /// Walk the directory tree, invoking `callback` for each file discovered.
    pub fn walk<F>(&self, dir_path: &Path, callback: &mut F) -> Result<()>
    where
        F: FnMut(&Path) -> Result<()>,
    {
        if !dir_path.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(dir_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if self.options.ignore_hidden_directories() && name.starts_with('.') {
                        continue;
                    }

                    if self
                        .options
                        .ignored_directory_names()
                        .iter()
                        .any(|ignored| ignored == name)
                    {
                        continue;
                    }
                }

                self.walk(&path, callback)?;
            } else if path.is_file() {
                callback(&path)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn walker_skips_hidden_and_configured_directories() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();

        let include_dir = root.join("src");
        let hidden_dir = root.join(".git");
        let ignored_dir = root.join("target");
        fs::create_dir_all(&include_dir).unwrap();
        fs::create_dir_all(&hidden_dir).unwrap();
        fs::create_dir_all(&ignored_dir).unwrap();

        let include_file = include_dir.join("lib.rs");
        let hidden_file = hidden_dir.join("secret.txt");
        let ignored_file = ignored_dir.join("ignored.rs");
        fs::write(&include_file, "pub fn ok() {}\n").unwrap();
        fs::write(&hidden_file, "should be skipped\n").unwrap();
        fs::write(&ignored_file, "fn ignored() {}\n").unwrap();

        let options = SimpleIndexerOptions::default();
        let walker = DirectoryWalker::new(&options);

        let mut files = Vec::new();
        walker
            .walk(root, &mut |path| {
                files.push(path.to_path_buf());
                Ok(())
            })
            .expect("walk succeeds");

        assert_eq!(files, vec![include_file]);
    }
}
