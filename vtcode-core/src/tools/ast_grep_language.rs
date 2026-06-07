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
    Markdown,
    C,
    Cpp,
    Csharp,
    Css,
    Html,
    Json,
    Yaml,
    Ruby,
    Php,
    Kotlin,
    Swift,
    Lua,
    Bash,
    Sql,
    Scala,
    Elixir,
    Dockerfile,
    Toml,
    Hcl,
    Dart,
    Zig,
    Protobuf,
    Haskell,
    Nix,
    Solidity,
}

impl AstGrepLanguage {
    pub(crate) fn from_user_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "rs" | "rust" => Some(Self::Rust),
            "py" | "python" | "python3" => Some(Self::Python),
            "js" | "javascript" | "jsx" => Some(Self::JavaScript),
            "ts" | "typescript" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "go" | "golang" => Some(Self::Go),
            "java" => Some(Self::Java),
            "md" | "markdown" => Some(Self::Markdown),
            "c" => Some(Self::C),
            "cpp" | "cc" | "c++" | "cxx" => Some(Self::Cpp),
            "cs" | "csharp" | "c#" => Some(Self::Csharp),
            "css" => Some(Self::Css),
            "html" | "htm" => Some(Self::Html),
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            "rb" | "ruby" => Some(Self::Ruby),
            "php" => Some(Self::Php),
            "kt" | "kotlin" => Some(Self::Kotlin),
            "swift" => Some(Self::Swift),
            "lua" => Some(Self::Lua),
            "bash" | "sh" | "shell" => Some(Self::Bash),
            "sql" => Some(Self::Sql),
            "scala" | "sc" => Some(Self::Scala),
            "elixir" | "ex" | "exs" => Some(Self::Elixir),
            "dockerfile" | "docker" => Some(Self::Dockerfile),
            "toml" => Some(Self::Toml),
            "hcl" | "terraform" | "tf" => Some(Self::Hcl),
            "dart" => Some(Self::Dart),
            "zig" => Some(Self::Zig),
            "protobuf" | "proto" => Some(Self::Protobuf),
            "hs" | "haskell" => Some(Self::Haskell),
            "nix" => Some(Self::Nix),
            "solidity" | "sol" => Some(Self::Solidity),
            _ => None,
        }
    }

    pub(crate) fn from_extension(extension: &str) -> Option<Self> {
        match extension.trim().to_ascii_lowercase().as_str() {
            "rs" => Some(Self::Rust),
            "py" | "py3" | "pyi" | "bzl" => Some(Self::Python),
            "js" | "jsx" | "cjs" | "mjs" => Some(Self::JavaScript),
            "ts" | "cts" | "mts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "md" | "mdx" => Some(Self::Markdown),
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" | "cu" | "ino" => Some(Self::Cpp),
            "cs" => Some(Self::Csharp),
            "css" => Some(Self::Css),
            "html" | "htm" | "xhtml" => Some(Self::Html),
            "json" | "jsonc" => Some(Self::Json),
            "yml" | "yaml" => Some(Self::Yaml),
            "rb" | "erb" | "rbw" | "gemspec" => Some(Self::Ruby),
            "php" => Some(Self::Php),
            "kt" | "kts" | "ktm" => Some(Self::Kotlin),
            "swift" => Some(Self::Swift),
            "lua" => Some(Self::Lua),
            "sh" | "bash" | "zsh" | "bats" | "ksh" => Some(Self::Bash),
            "sql" => Some(Self::Sql),
            "scala" | "sc" | "sbt" => Some(Self::Scala),
            "ex" | "exs" => Some(Self::Elixir),
            "dockerfile" => Some(Self::Dockerfile),
            "toml" => Some(Self::Toml),
            "hcl" | "tf" | "tfvars" => Some(Self::Hcl),
            "dart" => Some(Self::Dart),
            "zig" => Some(Self::Zig),
            "proto" => Some(Self::Protobuf),
            "hs" => Some(Self::Haskell),
            "nix" => Some(Self::Nix),
            "sol" => Some(Self::Solidity),
            _ => None,
        }
    }

    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        // Handle compound extensions that Path::extension() cannot resolve.
        let file_name = path.file_name()?.to_str()?;
        let lower = file_name.to_ascii_lowercase();
        if lower.ends_with(".sh.in") {
            return Some(Self::Bash);
        }
        if lower.ends_with(".c++") {
            return Some(Self::Cpp);
        }

        if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
            return Self::from_extension(extension);
        }
        // Handle extensionless files by matching the file name directly.
        match lower.as_str() {
            "dockerfile" => Some(Self::Dockerfile),
            _ => None,
        }
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
            Self::Markdown => "md",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Csharp => "csharp",
            Self::Css => "css",
            Self::Html => "html",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Ruby => "ruby",
            Self::Php => "php",
            Self::Kotlin => "kotlin",
            Self::Swift => "swift",
            Self::Lua => "lua",
            Self::Bash => "bash",
            Self::Sql => "sql",
            Self::Scala => "scala",
            Self::Elixir => "elixir",
            Self::Dockerfile => "dockerfile",
            Self::Toml => "toml",
            Self::Hcl => "hcl",
            Self::Dart => "dart",
            Self::Zig => "zig",
            Self::Protobuf => "proto",
            Self::Haskell => "haskell",
            Self::Nix => "nix",
            Self::Solidity => "solidity",
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
            Self::Markdown => "Markdown",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Csharp => "C#",
            Self::Css => "CSS",
            Self::Html => "HTML",
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Ruby => "Ruby",
            Self::Php => "PHP",
            Self::Kotlin => "Kotlin",
            Self::Swift => "Swift",
            Self::Lua => "Lua",
            Self::Bash => "Bash",
            Self::Sql => "SQL",
            Self::Scala => "Scala",
            Self::Elixir => "Elixir",
            Self::Dockerfile => "Dockerfile",
            Self::Toml => "TOML",
            Self::Hcl => "HCL",
            Self::Dart => "Dart",
            Self::Zig => "Zig",
            Self::Protobuf => "Protobuf",
            Self::Haskell => "Haskell",
            Self::Nix => "Nix",
            Self::Solidity => "Solidity",
        }
    }

    /// Returns `true` when a local tree-sitter parser is available for preflight.
    /// Languages without a local parser delegate directly to the ast-grep binary,
    /// which has its own built-in tree-sitter parsers for all supported languages.
    pub(crate) fn has_local_parser(self) -> bool {
        matches!(
            self,
            Self::Rust
                | Self::Python
                | Self::JavaScript
                | Self::TypeScript
                | Self::Tsx
                | Self::Go
                | Self::Java
                | Self::Bash
        )
    }

    pub(crate) fn from_workspace_language(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "rust" => Some(Self::Rust),
            "python" => Some(Self::Python),
            "javascript" => Some(Self::JavaScript),
            "typescript" => Some(Self::TypeScript),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "markdown" | "md" => Some(Self::Markdown),
            "c" => Some(Self::C),
            "c++" | "cpp" => Some(Self::Cpp),
            "c#" | "csharp" => Some(Self::Csharp),
            "css" => Some(Self::Css),
            "html" => Some(Self::Html),
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            "ruby" => Some(Self::Ruby),
            "php" => Some(Self::Php),
            "kotlin" => Some(Self::Kotlin),
            "swift" => Some(Self::Swift),
            "lua" => Some(Self::Lua),
            "bash" | "shell" => Some(Self::Bash),
            "sql" => Some(Self::Sql),
            "scala" => Some(Self::Scala),
            "elixir" => Some(Self::Elixir),
            "dockerfile" | "docker" => Some(Self::Dockerfile),
            "toml" => Some(Self::Toml),
            "hcl" | "terraform" => Some(Self::Hcl),
            "dart" => Some(Self::Dart),
            "zig" => Some(Self::Zig),
            "protobuf" | "proto" => Some(Self::Protobuf),
            "haskell" | "hs" => Some(Self::Haskell),
            "nix" => Some(Self::Nix),
            "solidity" | "sol" => Some(Self::Solidity),
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
            AstGrepLanguage::from_user_value("jsx"),
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
            AstGrepLanguage::from_user_value("golang"),
            Some(AstGrepLanguage::Go)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("java"),
            Some(AstGrepLanguage::Java)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("md"),
            Some(AstGrepLanguage::Markdown)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("markdown"),
            Some(AstGrepLanguage::Markdown)
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
            AstGrepLanguage::from_path(Path::new("tools/types.pyi")),
            Some(AstGrepLanguage::Python)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("web/app.js")),
            Some(AstGrepLanguage::JavaScript)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("web/app.jsx")),
            Some(AstGrepLanguage::JavaScript)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("web/app.mjs")),
            Some(AstGrepLanguage::JavaScript)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("web/app.ts")),
            Some(AstGrepLanguage::TypeScript)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("web/app.cts")),
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
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("docs/README.md")),
            Some(AstGrepLanguage::Markdown)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("docs/guide.mdx")),
            Some(AstGrepLanguage::Markdown)
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
            AstGrepLanguage::infer_from_positive_globs(["src/**/*.mjs"]),
            Some(AstGrepLanguage::JavaScript)
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
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Swift"),
            Some(AstGrepLanguage::Swift)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Markdown"),
            Some(AstGrepLanguage::Markdown)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("md"),
            Some(AstGrepLanguage::Markdown)
        );
        assert_eq!(AstGrepLanguage::Tsx.display_name(), "TypeScript");
        assert_eq!(AstGrepLanguage::Markdown.display_name(), "Markdown");
        assert_eq!(AstGrepLanguage::Markdown.as_str(), "md");
    }

    #[test]
    fn normalizes_new_language_aliases() {
        assert_eq!(
            AstGrepLanguage::from_user_value("c"),
            Some(AstGrepLanguage::C)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("cpp"),
            Some(AstGrepLanguage::Cpp)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("cc"),
            Some(AstGrepLanguage::Cpp)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("c++"),
            Some(AstGrepLanguage::Cpp)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("cs"),
            Some(AstGrepLanguage::Csharp)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("csharp"),
            Some(AstGrepLanguage::Csharp)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("css"),
            Some(AstGrepLanguage::Css)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("html"),
            Some(AstGrepLanguage::Html)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("htm"),
            Some(AstGrepLanguage::Html)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("json"),
            Some(AstGrepLanguage::Json)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("yaml"),
            Some(AstGrepLanguage::Yaml)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("yml"),
            Some(AstGrepLanguage::Yaml)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("rb"),
            Some(AstGrepLanguage::Ruby)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("ruby"),
            Some(AstGrepLanguage::Ruby)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("php"),
            Some(AstGrepLanguage::Php)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("kt"),
            Some(AstGrepLanguage::Kotlin)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("kotlin"),
            Some(AstGrepLanguage::Kotlin)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("swift"),
            Some(AstGrepLanguage::Swift)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("lua"),
            Some(AstGrepLanguage::Lua)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("bash"),
            Some(AstGrepLanguage::Bash)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("sh"),
            Some(AstGrepLanguage::Bash)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("shell"),
            Some(AstGrepLanguage::Bash)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("sql"),
            Some(AstGrepLanguage::Sql)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("scala"),
            Some(AstGrepLanguage::Scala)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("elixir"),
            Some(AstGrepLanguage::Elixir)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("ex"),
            Some(AstGrepLanguage::Elixir)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("exs"),
            Some(AstGrepLanguage::Elixir)
        );
    }

    #[test]
    fn infers_new_languages_from_file_paths() {
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/main.c")),
            Some(AstGrepLanguage::C)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/main.h")),
            Some(AstGrepLanguage::C)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/main.cpp")),
            Some(AstGrepLanguage::Cpp)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/main.cc")),
            Some(AstGrepLanguage::Cpp)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/main.hpp")),
            Some(AstGrepLanguage::Cpp)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/Main.cs")),
            Some(AstGrepLanguage::Csharp)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("styles/main.css")),
            Some(AstGrepLanguage::Css)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("templates/index.html")),
            Some(AstGrepLanguage::Html)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("data/config.json")),
            Some(AstGrepLanguage::Json)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("config.yml")),
            Some(AstGrepLanguage::Yaml)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("config.yaml")),
            Some(AstGrepLanguage::Yaml)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("app/models/user.rb")),
            Some(AstGrepLanguage::Ruby)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/index.php")),
            Some(AstGrepLanguage::Php)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/Main.kt")),
            Some(AstGrepLanguage::Kotlin)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("ios/App.swift")),
            Some(AstGrepLanguage::Swift)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("scripts/init.lua")),
            Some(AstGrepLanguage::Lua)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("scripts/deploy.sh")),
            Some(AstGrepLanguage::Bash)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("migrations/001.sql")),
            Some(AstGrepLanguage::Sql)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/Main.scala")),
            Some(AstGrepLanguage::Scala)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("lib/app.ex")),
            Some(AstGrepLanguage::Elixir)
        );
    }

    #[test]
    fn new_languages_display_names_and_str_values() {
        assert_eq!(AstGrepLanguage::C.display_name(), "C");
        assert_eq!(AstGrepLanguage::C.as_str(), "c");
        assert_eq!(AstGrepLanguage::Cpp.display_name(), "C++");
        assert_eq!(AstGrepLanguage::Cpp.as_str(), "cpp");
        assert_eq!(AstGrepLanguage::Csharp.display_name(), "C#");
        assert_eq!(AstGrepLanguage::Csharp.as_str(), "csharp");
        assert_eq!(AstGrepLanguage::Css.display_name(), "CSS");
        assert_eq!(AstGrepLanguage::Css.as_str(), "css");
        assert_eq!(AstGrepLanguage::Html.display_name(), "HTML");
        assert_eq!(AstGrepLanguage::Html.as_str(), "html");
        assert_eq!(AstGrepLanguage::Json.display_name(), "JSON");
        assert_eq!(AstGrepLanguage::Json.as_str(), "json");
        assert_eq!(AstGrepLanguage::Yaml.display_name(), "YAML");
        assert_eq!(AstGrepLanguage::Yaml.as_str(), "yaml");
        assert_eq!(AstGrepLanguage::Ruby.display_name(), "Ruby");
        assert_eq!(AstGrepLanguage::Ruby.as_str(), "ruby");
        assert_eq!(AstGrepLanguage::Php.display_name(), "PHP");
        assert_eq!(AstGrepLanguage::Php.as_str(), "php");
        assert_eq!(AstGrepLanguage::Kotlin.display_name(), "Kotlin");
        assert_eq!(AstGrepLanguage::Kotlin.as_str(), "kotlin");
        assert_eq!(AstGrepLanguage::Swift.display_name(), "Swift");
        assert_eq!(AstGrepLanguage::Swift.as_str(), "swift");
        assert_eq!(AstGrepLanguage::Lua.display_name(), "Lua");
        assert_eq!(AstGrepLanguage::Lua.as_str(), "lua");
        assert_eq!(AstGrepLanguage::Bash.display_name(), "Bash");
        assert_eq!(AstGrepLanguage::Bash.as_str(), "bash");
        assert_eq!(AstGrepLanguage::Sql.display_name(), "SQL");
        assert_eq!(AstGrepLanguage::Sql.as_str(), "sql");
        assert_eq!(AstGrepLanguage::Scala.display_name(), "Scala");
        assert_eq!(AstGrepLanguage::Scala.as_str(), "scala");
        assert_eq!(AstGrepLanguage::Elixir.display_name(), "Elixir");
        assert_eq!(AstGrepLanguage::Elixir.as_str(), "elixir");
    }

    #[test]
    fn new_languages_have_no_local_parser() {
        assert!(!AstGrepLanguage::C.has_local_parser());
        assert!(!AstGrepLanguage::Cpp.has_local_parser());
        assert!(!AstGrepLanguage::Csharp.has_local_parser());
        assert!(!AstGrepLanguage::Css.has_local_parser());
        assert!(!AstGrepLanguage::Html.has_local_parser());
        assert!(!AstGrepLanguage::Json.has_local_parser());
        assert!(!AstGrepLanguage::Yaml.has_local_parser());
        assert!(!AstGrepLanguage::Ruby.has_local_parser());
        assert!(!AstGrepLanguage::Php.has_local_parser());
        assert!(!AstGrepLanguage::Kotlin.has_local_parser());
        assert!(!AstGrepLanguage::Swift.has_local_parser());
        assert!(!AstGrepLanguage::Lua.has_local_parser());
        assert!(!AstGrepLanguage::Bash.has_local_parser());
        assert!(!AstGrepLanguage::Sql.has_local_parser());
        assert!(!AstGrepLanguage::Scala.has_local_parser());
        assert!(!AstGrepLanguage::Elixir.has_local_parser());
    }

    #[test]
    fn original_languages_still_have_local_parser() {
        assert!(AstGrepLanguage::Rust.has_local_parser());
        assert!(AstGrepLanguage::Python.has_local_parser());
        assert!(AstGrepLanguage::JavaScript.has_local_parser());
        assert!(AstGrepLanguage::TypeScript.has_local_parser());
        assert!(AstGrepLanguage::Tsx.has_local_parser());
        assert!(AstGrepLanguage::Go.has_local_parser());
        assert!(AstGrepLanguage::Java.has_local_parser());
        assert!(!AstGrepLanguage::Markdown.has_local_parser());
    }

    #[test]
    fn maps_new_workspace_language_names() {
        assert_eq!(
            AstGrepLanguage::from_workspace_language("C"),
            Some(AstGrepLanguage::C)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("C++"),
            Some(AstGrepLanguage::Cpp)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("cpp"),
            Some(AstGrepLanguage::Cpp)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("C#"),
            Some(AstGrepLanguage::Csharp)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("CSS"),
            Some(AstGrepLanguage::Css)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("HTML"),
            Some(AstGrepLanguage::Html)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("JSON"),
            Some(AstGrepLanguage::Json)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("YAML"),
            Some(AstGrepLanguage::Yaml)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("yml"),
            Some(AstGrepLanguage::Yaml)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Ruby"),
            Some(AstGrepLanguage::Ruby)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("PHP"),
            Some(AstGrepLanguage::Php)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Kotlin"),
            Some(AstGrepLanguage::Kotlin)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Swift"),
            Some(AstGrepLanguage::Swift)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Lua"),
            Some(AstGrepLanguage::Lua)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Bash"),
            Some(AstGrepLanguage::Bash)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("shell"),
            Some(AstGrepLanguage::Bash)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("SQL"),
            Some(AstGrepLanguage::Sql)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Scala"),
            Some(AstGrepLanguage::Scala)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Elixir"),
            Some(AstGrepLanguage::Elixir)
        );
    }

    #[test]
    fn normalizes_dockerfile_toml_hcl_aliases() {
        assert_eq!(
            AstGrepLanguage::from_user_value("dockerfile"),
            Some(AstGrepLanguage::Dockerfile)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("docker"),
            Some(AstGrepLanguage::Dockerfile)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("toml"),
            Some(AstGrepLanguage::Toml)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("hcl"),
            Some(AstGrepLanguage::Hcl)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("terraform"),
            Some(AstGrepLanguage::Hcl)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("tf"),
            Some(AstGrepLanguage::Hcl)
        );
    }

    #[test]
    fn normalizes_dart_zig_protobuf_aliases() {
        assert_eq!(
            AstGrepLanguage::from_user_value("dart"),
            Some(AstGrepLanguage::Dart)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("zig"),
            Some(AstGrepLanguage::Zig)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("protobuf"),
            Some(AstGrepLanguage::Protobuf)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("proto"),
            Some(AstGrepLanguage::Protobuf)
        );
    }

    #[test]
    fn infers_dockerfile_toml_hcl_from_file_paths() {
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("Dockerfile")),
            Some(AstGrepLanguage::Dockerfile)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("Cargo.toml")),
            Some(AstGrepLanguage::Toml)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("main.tf")),
            Some(AstGrepLanguage::Hcl)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("vars.tfvars")),
            Some(AstGrepLanguage::Hcl)
        );
    }

    #[test]
    fn infers_dart_zig_protobuf_from_file_paths() {
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("lib/main.dart")),
            Some(AstGrepLanguage::Dart)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/main.zig")),
            Some(AstGrepLanguage::Zig)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("proto/service.proto")),
            Some(AstGrepLanguage::Protobuf)
        );
    }

    #[test]
    fn dockerfile_toml_hcl_display_names_and_str_values() {
        assert_eq!(AstGrepLanguage::Dockerfile.display_name(), "Dockerfile");
        assert_eq!(AstGrepLanguage::Dockerfile.as_str(), "dockerfile");
        assert_eq!(AstGrepLanguage::Toml.display_name(), "TOML");
        assert_eq!(AstGrepLanguage::Toml.as_str(), "toml");
        assert_eq!(AstGrepLanguage::Hcl.display_name(), "HCL");
        assert_eq!(AstGrepLanguage::Hcl.as_str(), "hcl");
    }

    #[test]
    fn dart_zig_protobuf_display_names_and_str_values() {
        assert_eq!(AstGrepLanguage::Dart.display_name(), "Dart");
        assert_eq!(AstGrepLanguage::Dart.as_str(), "dart");
        assert_eq!(AstGrepLanguage::Zig.display_name(), "Zig");
        assert_eq!(AstGrepLanguage::Zig.as_str(), "zig");
        assert_eq!(AstGrepLanguage::Protobuf.display_name(), "Protobuf");
        assert_eq!(AstGrepLanguage::Protobuf.as_str(), "proto");
    }

    #[test]
    fn extended_languages_have_no_local_parser() {
        assert!(!AstGrepLanguage::Dockerfile.has_local_parser());
        assert!(!AstGrepLanguage::Toml.has_local_parser());
        assert!(!AstGrepLanguage::Hcl.has_local_parser());
        assert!(!AstGrepLanguage::Dart.has_local_parser());
        assert!(!AstGrepLanguage::Zig.has_local_parser());
        assert!(!AstGrepLanguage::Protobuf.has_local_parser());
    }

    #[test]
    fn maps_dockerfile_toml_hcl_workspace_names() {
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Dockerfile"),
            Some(AstGrepLanguage::Dockerfile)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("docker"),
            Some(AstGrepLanguage::Dockerfile)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("TOML"),
            Some(AstGrepLanguage::Toml)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("HCL"),
            Some(AstGrepLanguage::Hcl)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("terraform"),
            Some(AstGrepLanguage::Hcl)
        );
    }

    #[test]
    fn maps_dart_zig_protobuf_workspace_names() {
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Dart"),
            Some(AstGrepLanguage::Dart)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Zig"),
            Some(AstGrepLanguage::Zig)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Protobuf"),
            Some(AstGrepLanguage::Protobuf)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("proto"),
            Some(AstGrepLanguage::Protobuf)
        );
    }

    #[test]
    fn normalizes_haskell_nix_solidity_aliases() {
        assert_eq!(
            AstGrepLanguage::from_user_value("hs"),
            Some(AstGrepLanguage::Haskell)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("haskell"),
            Some(AstGrepLanguage::Haskell)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("nix"),
            Some(AstGrepLanguage::Nix)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("solidity"),
            Some(AstGrepLanguage::Solidity)
        );
        assert_eq!(
            AstGrepLanguage::from_user_value("sol"),
            Some(AstGrepLanguage::Solidity)
        );
    }

    #[test]
    fn infers_haskell_nix_solidity_from_file_paths() {
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/Main.hs")),
            Some(AstGrepLanguage::Haskell)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("default.nix")),
            Some(AstGrepLanguage::Nix)
        );
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("contracts/Token.sol")),
            Some(AstGrepLanguage::Solidity)
        );
    }

    #[test]
    fn haskell_nix_solidity_display_names_and_str_values() {
        assert_eq!(AstGrepLanguage::Haskell.display_name(), "Haskell");
        assert_eq!(AstGrepLanguage::Haskell.as_str(), "haskell");
        assert_eq!(AstGrepLanguage::Nix.display_name(), "Nix");
        assert_eq!(AstGrepLanguage::Nix.as_str(), "nix");
        assert_eq!(AstGrepLanguage::Solidity.display_name(), "Solidity");
        assert_eq!(AstGrepLanguage::Solidity.as_str(), "solidity");
    }

    #[test]
    fn haskell_nix_solidity_have_no_local_parser() {
        assert!(!AstGrepLanguage::Haskell.has_local_parser());
        assert!(!AstGrepLanguage::Nix.has_local_parser());
        assert!(!AstGrepLanguage::Solidity.has_local_parser());
    }

    #[test]
    fn maps_haskell_nix_solidity_workspace_names() {
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Haskell"),
            Some(AstGrepLanguage::Haskell)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("hs"),
            Some(AstGrepLanguage::Haskell)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Nix"),
            Some(AstGrepLanguage::Nix)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("Solidity"),
            Some(AstGrepLanguage::Solidity)
        );
        assert_eq!(
            AstGrepLanguage::from_workspace_language("sol"),
            Some(AstGrepLanguage::Solidity)
        );
    }

    #[test]
    fn infers_newly_added_extensions_for_existing_languages() {
        // Python - bzl
        assert_eq!(
            AstGrepLanguage::from_extension("bzl"),
            Some(AstGrepLanguage::Python)
        );
        // Cpp - cu, ino
        assert_eq!(
            AstGrepLanguage::from_extension("cu"),
            Some(AstGrepLanguage::Cpp)
        );
        assert_eq!(
            AstGrepLanguage::from_extension("ino"),
            Some(AstGrepLanguage::Cpp)
        );
        // Html - xhtml
        assert_eq!(
            AstGrepLanguage::from_extension("xhtml"),
            Some(AstGrepLanguage::Html)
        );
        // Ruby - rbw, gemspec
        assert_eq!(
            AstGrepLanguage::from_extension("rbw"),
            Some(AstGrepLanguage::Ruby)
        );
        assert_eq!(
            AstGrepLanguage::from_extension("gemspec"),
            Some(AstGrepLanguage::Ruby)
        );
        // Kotlin - ktm
        assert_eq!(
            AstGrepLanguage::from_extension("ktm"),
            Some(AstGrepLanguage::Kotlin)
        );
        // Bash - bats, ksh
        assert_eq!(
            AstGrepLanguage::from_extension("bats"),
            Some(AstGrepLanguage::Bash)
        );
        assert_eq!(
            AstGrepLanguage::from_extension("ksh"),
            Some(AstGrepLanguage::Bash)
        );
        // Scala - sbt
        assert_eq!(
            AstGrepLanguage::from_extension("sbt"),
            Some(AstGrepLanguage::Scala)
        );
    }

    #[test]
    fn infers_compound_extensions_from_paths() {
        // sh.in -> Bash
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("scripts/config.sh.in")),
            Some(AstGrepLanguage::Bash)
        );
        // c++ -> Cpp
        assert_eq!(
            AstGrepLanguage::from_path(Path::new("src/main.c++")),
            Some(AstGrepLanguage::Cpp)
        );
    }
}
