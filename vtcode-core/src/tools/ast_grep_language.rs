use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AstGrepLanguage {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
    Java,
}

impl AstGrepLanguage {
    pub(crate) fn from_user_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "rs" | "rust" => Some(Self::Rust),
            "py" | "python" | "python3" => Some(Self::Python),
            "js" | "javascript" => Some(Self::JavaScript),
            "ts" | "typescript" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            _ => None,
        }
    }

    pub(crate) fn from_extension(extension: &str) -> Option<Self> {
        match extension.trim().to_ascii_lowercase().as_str() {
            "rs" => Some(Self::Rust),
            "py" => Some(Self::Python),
            "js" => Some(Self::JavaScript),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            _ => None,
        }
    }

    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?;
        Self::from_extension(extension)
    }

    pub(crate) fn infer_from_path_str(path: &str) -> Option<Self> {
        let trimmed = path.trim();
        if trimmed.is_empty() || looks_like_glob(trimmed) {
            return None;
        }
        Self::from_path(Path::new(trimmed))
    }

    pub(crate) fn infer_from_positive_globs<'a>(
        globs: impl IntoIterator<Item = &'a str>,
    ) -> Option<Self> {
        let mut candidate = None;
        let mut saw_inferable = false;

        for glob in globs {
            let trimmed = glob.trim();
            if trimmed.is_empty() || trimmed.starts_with('!') {
                continue;
            }

            let inferred = infer_from_glob(trimmed)?;
            saw_inferable = true;
            match candidate {
                Some(current) if current != inferred => return None,
                Some(_) => {}
                None => candidate = Some(inferred),
            }
        }

        if saw_inferable { candidate } else { None }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Go => "go",
            Self::Java => "java",
        }
    }

    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript | Self::Tsx => "TypeScript",
            Self::Go => "Go",
            Self::Java => "Java",
        }
    }

    pub(crate) fn from_workspace_language(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "rust" => Some(Self::Rust),
            "python" => Some(Self::Python),
            "javascript" => Some(Self::JavaScript),
            "typescript" => Some(Self::TypeScript),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            _ => None,
        }
    }
}

fn infer_from_glob(glob: &str) -> Option<AstGrepLanguage> {
    if glob.contains('{')
        || glob.contains('}')
        || glob.contains('[')
        || glob.contains(']')
        || glob.contains('?')
    {
        return None;
    }

    AstGrepLanguage::from_path(Path::new(glob))
}

fn looks_like_glob(value: &str) -> bool {
    value.contains('*') || value.contains('{') || value.contains('[') || value.contains('?')
}

#[cfg(test)]
mod tests {
    use super::AstGrepLanguage;
    use std::path::Path;

    #[test]
    fn normalizes_common_language_aliases() {
        assert_eq!(
            AstGrepLanguage::from_user_value("rs"),
            Some(AstGrepLanguage::Rust)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("python3"),
            Some(AstGrepLanguage::Python)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("js"),
            Some(AstGrepLanguage::JavaScript)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("ts"),
            Some(AstGrepLanguage::TypeScript)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("tsx"),
            Some(AstGrepLanguage::Tsx)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("go"),
            Some(AstGrepLanguage::Go)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("java"),
            Some(AstGrepLanguage::Java)
        );
    }

    #[test]
    fn infers_language_from_supported_file_paths() {
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/lib.rs")),
            Some(AstGrepLanguage::Rust)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("tools/script.py")),
            Some(AstGrepLanguage::Python)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("web/app.js")),
            Some(AstGrepLanguage::JavaScript)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("web/app.ts")),
            Some(AstGrepLanguage::TypeScript)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("web/app.tsx")),
            Some(AstGrepLanguage::Tsx)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("cmd/main.go")),
            Some(AstGrepLanguage::Go)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/Main.java")),
            Some(AstGrepLanguage::Java)
        );
    }

    #[test]
    fn infers_language_from_positive_globs_when_unambiguous() {
        assert_eq!(
            AstGrepLanguage::infer_from_positive_globs(["*.rs"]),
            Some(AstGrepLanguage::Rust)
        );
        assert_eq!(
            AstGrepLanguage::infer_from_positive_globs(["**/*.ts"]),
            Some(AstGrepLanguage::TypeScript)
        );
        assert_eq!(
            AstGrepLanguage::infer_from_positive_globs(["src/**/*.go"]),
            Some(AstGrepLanguage::Go)
        );
    }

    #[test]
    fn does_not_infer_language_from_mixed_or_weak_globs() {
        assert_eq!(
            AstGrepLanguage::infer_from_positive_globs(["**/*.rs", "**/*.ts"]),
            None
        );
        assert_eq!(AstGrepLanguage::infer_from_positive_globs(["src/**"]), None);
        assert_eq!(
            AstGrepLanguage::infer_from_positive_globs(["!dist/**", "**/*.rs"]),
            Some(AstGrepLanguage::Rust)
        );
    }

    #[test]
    fn does_not_infer_language_from_directories_or_unknown_extensions() {
        assert_eq!(AstGrepLanguage::infer_from_path_str("."), None);
        assert_eq!(AstGrepLanguage::infer_from_path_str("src/"), None);
        assert_eq!(AstGrepLanguage::infer_from_path_str("notes.txt"), None);
        assert_eq!(AstGrepLanguage::from_user_value("mojo"), None);
    }

    #[test]
    fn maps_workspace_language_names_back_to_supported_languages() {
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Rust"),
            Some(AstGrepLanguage::Rust)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("TypeScript"),
            Some(AstGrepLanguage::TypeScript)
        );
        assert_eq!(AstGrepLanguage::from_workspace_language("Swift"), None);
        assert_eq!(AstGrepLanguage::Tsx.display_name(), "TypeScript");
    }
}
