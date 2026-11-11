use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CodeLanguage {
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

impl CodeLanguage {
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

    /// The comment and string syntax of this language.
    ///
    /// The lexical scanner uses this to tell an identifier occurrence apart
    /// from the same text inside a string literal or a comment.
    pub(crate) fn lexical_syntax(self) -> vtcode_commons::lexical::LanguageSyntax {
        use vtcode_commons::lexical::LanguageSyntax;

        match self {
            Self::Rust => LanguageSyntax {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &["\""],
                backslash_escapes: true,
            },
            Self::Python => LanguageSyntax {
                line_comments: &["#"],
                block_comments: &[],
                string_delimiters: &["\"\"\"", "'", "\""],
                backslash_escapes: true,
            },
            Self::JavaScript | Self::TypeScript | Self::Tsx => LanguageSyntax {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &["`", "\"", "'"],
                backslash_escapes: true,
            },
            Self::Go => LanguageSyntax {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &["`", "\""],
                backslash_escapes: true,
            },
            Self::Java => LanguageSyntax {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &["\""],
                backslash_escapes: true,
            },
            Self::C | Self::Cpp => LanguageSyntax {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &["\""],
                backslash_escapes: true,
            },
            Self::Csharp => LanguageSyntax {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &["@\"", "\""],
                backslash_escapes: true,
            },
            Self::Kotlin | Self::Scala | Self::Swift | Self::Dart | Self::Solidity => LanguageSyntax {
                line_comments: &["//"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &["\"\"\"", "\""],
                backslash_escapes: true,
            },
            Self::Php => LanguageSyntax {
                line_comments: &["//", "#"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &["\"", "'"],
                backslash_escapes: true,
            },
            Self::Ruby => LanguageSyntax {
                line_comments: &["#"],
                block_comments: &[("=begin", "=end")],
                string_delimiters: &["\""],
                backslash_escapes: true,
            },
            Self::Bash | Self::Dockerfile | Self::Nix | Self::Hcl => LanguageSyntax {
                line_comments: &["#"],
                block_comments: &[],
                string_delimiters: &["\"", "'"],
                backslash_escapes: true,
            },
            Self::Lua => LanguageSyntax {
                line_comments: &["--"],
                block_comments: &[("--[[", "]]")],
                string_delimiters: &["\"", "'"],
                backslash_escapes: true,
            },
            Self::Haskell => LanguageSyntax {
                line_comments: &["--"],
                block_comments: &[("{-", "-}")],
                string_delimiters: &["\""],
                backslash_escapes: true,
            },
            Self::Elixir => LanguageSyntax {
                line_comments: &["#"],
                block_comments: &[],
                string_delimiters: &["\"\"\"", "\"", "'"],
                backslash_escapes: true,
            },
            Self::Sql => LanguageSyntax {
                line_comments: &["--"],
                block_comments: &[("/*", "*/")],
                string_delimiters: &["'", "\""],
                backslash_escapes: true,
            },
            Self::Zig => LanguageSyntax {
                line_comments: &["//"],
                block_comments: &[],
                string_delimiters: &["\""],
                backslash_escapes: true,
            },
            _ => LanguageSyntax::default(),
        }
    }

    /// Whether this language supports usage classification.
    ///
    /// A usage is an exact identifier occurrence that is not the declaration
    /// of that symbol. Classifying one needs the language's declaration
    /// patterns, so a language without them cannot report usages and its hits
    /// stay text results.
    pub(crate) fn supports_usage_classification(self) -> bool {
        matches!(
            self,
            Self::Rust
                | Self::Python
                | Self::JavaScript
                | Self::TypeScript
                | Self::Tsx
                | Self::Go
                | Self::Java
                | Self::C
                | Self::Cpp
        )
    }
    /// Canonical positive globs for backend prefiltering. Candidates must still
    /// be checked with [`Self::from_path`], which remains authoritative.
    pub(crate) fn path_globs(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["*.rs"],
            Self::Python => &["*.py", "*.py3", "*.pyi", "*.bzl"],
            Self::JavaScript => &["*.js", "*.jsx", "*.cjs", "*.mjs"],
            Self::TypeScript => &["*.ts", "*.cts", "*.mts"],
            Self::Tsx => &["*.tsx"],
            Self::Go => &["*.go"],
            Self::Java => &["*.java"],
            Self::Markdown => &["*.md", "*.mdx"],
            Self::C => &["*.c", "*.h"],
            Self::Cpp => &[
                "*.cpp", "*.cc", "*.cxx", "*.hpp", "*.hxx", "*.hh", "*.cu", "*.ino", "*.c++",
            ],
            Self::Csharp => &["*.cs"],
            Self::Css => &["*.css"],
            Self::Html => &["*.html", "*.htm", "*.xhtml"],
            Self::Json => &["*.json", "*.jsonc"],
            Self::Yaml => &["*.yml", "*.yaml"],
            Self::Ruby => &["*.rb", "*.erb", "*.rbw", "*.gemspec"],
            Self::Php => &["*.php"],
            Self::Kotlin => &["*.kt", "*.kts", "*.ktm"],
            Self::Swift => &["*.swift"],
            Self::Lua => &["*.lua"],
            Self::Bash => &["*.sh", "*.bash", "*.zsh", "*.bats", "*.ksh", "*.sh.in"],
            Self::Sql => &["*.sql"],
            Self::Scala => &["*.scala", "*.sc", "*.sbt"],
            Self::Elixir => &["*.ex", "*.exs"],
            Self::Dockerfile => &["Dockerfile", "dockerfile"],
            Self::Toml => &["*.toml"],
            Self::Hcl => &["*.hcl", "*.tf", "*.tfvars"],
            Self::Dart => &["*.dart"],
            Self::Zig => &["*.zig"],
            Self::Protobuf => &["*.proto"],
            Self::Haskell => &["*.hs"],
            Self::Nix => &["*.nix"],
            Self::Solidity => &["*.sol"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CodeLanguage;
    use std::path::Path;

    #[test]
    fn normalizes_common_language_aliases() {
        assert_eq!(CodeLanguage::from_user_value("rs"), Some(CodeLanguage::Rust));
        assert_eq!(CodeLanguage::from_user_value("python3"), Some(CodeLanguage::Python));
        assert_eq!(CodeLanguage::from_user_value("js"), Some(CodeLanguage::JavaScript));
        assert_eq!(CodeLanguage::from_user_value("jsx"), Some(CodeLanguage::JavaScript));
        assert_eq!(CodeLanguage::from_user_value("ts"), Some(CodeLanguage::TypeScript));
        assert_eq!(CodeLanguage::from_user_value("tsx"), Some(CodeLanguage::Tsx));
        assert_eq!(CodeLanguage::from_user_value("golang"), Some(CodeLanguage::Go));
        assert_eq!(CodeLanguage::from_user_value("java"), Some(CodeLanguage::Java));
        assert_eq!(CodeLanguage::from_user_value("md"), Some(CodeLanguage::Markdown));
        assert_eq!(CodeLanguage::from_user_value("markdown"), Some(CodeLanguage::Markdown));
    }

    #[test]
    fn infers_language_from_supported_file_paths() {
        assert_eq!(CodeLanguage::from_path(Path::new("src/lib.rs")), Some(CodeLanguage::Rust));
        assert_eq!(CodeLanguage::from_path(Path::new("tools/script.py")), Some(CodeLanguage::Python));
        assert_eq!(CodeLanguage::from_path(Path::new("tools/types.pyi")), Some(CodeLanguage::Python));
        assert_eq!(CodeLanguage::from_path(Path::new("web/app.js")), Some(CodeLanguage::JavaScript));
        assert_eq!(CodeLanguage::from_path(Path::new("web/app.jsx")), Some(CodeLanguage::JavaScript));
        assert_eq!(CodeLanguage::from_path(Path::new("web/app.mjs")), Some(CodeLanguage::JavaScript));
        assert_eq!(CodeLanguage::from_path(Path::new("web/app.ts")), Some(CodeLanguage::TypeScript));
        assert_eq!(CodeLanguage::from_path(Path::new("web/app.cts")), Some(CodeLanguage::TypeScript));
        assert_eq!(CodeLanguage::from_path(Path::new("web/app.tsx")), Some(CodeLanguage::Tsx));
        assert_eq!(CodeLanguage::from_path(Path::new("cmd/main.go")), Some(CodeLanguage::Go));
        assert_eq!(CodeLanguage::from_path(Path::new("src/Main.java")), Some(CodeLanguage::Java));
        assert_eq!(CodeLanguage::from_path(Path::new("docs/README.md")), Some(CodeLanguage::Markdown));
        assert_eq!(CodeLanguage::from_path(Path::new("docs/guide.mdx")), Some(CodeLanguage::Markdown));
    }

    #[test]
    fn rejects_unknown_language_values() {
        assert_eq!(CodeLanguage::from_user_value("mojo"), None);
    }

    #[test]
    fn normalizes_new_language_aliases() {
        assert_eq!(CodeLanguage::from_user_value("c"), Some(CodeLanguage::C));
        assert_eq!(CodeLanguage::from_user_value("cpp"), Some(CodeLanguage::Cpp));
        assert_eq!(CodeLanguage::from_user_value("cc"), Some(CodeLanguage::Cpp));
        assert_eq!(CodeLanguage::from_user_value("c++"), Some(CodeLanguage::Cpp));
        assert_eq!(CodeLanguage::from_user_value("cs"), Some(CodeLanguage::Csharp));
        assert_eq!(CodeLanguage::from_user_value("csharp"), Some(CodeLanguage::Csharp));
        assert_eq!(CodeLanguage::from_user_value("css"), Some(CodeLanguage::Css));
        assert_eq!(CodeLanguage::from_user_value("html"), Some(CodeLanguage::Html));
        assert_eq!(CodeLanguage::from_user_value("htm"), Some(CodeLanguage::Html));
        assert_eq!(CodeLanguage::from_user_value("json"), Some(CodeLanguage::Json));
        assert_eq!(CodeLanguage::from_user_value("yaml"), Some(CodeLanguage::Yaml));
        assert_eq!(CodeLanguage::from_user_value("yml"), Some(CodeLanguage::Yaml));
        assert_eq!(CodeLanguage::from_user_value("rb"), Some(CodeLanguage::Ruby));
        assert_eq!(CodeLanguage::from_user_value("ruby"), Some(CodeLanguage::Ruby));
        assert_eq!(CodeLanguage::from_user_value("php"), Some(CodeLanguage::Php));
        assert_eq!(CodeLanguage::from_user_value("kt"), Some(CodeLanguage::Kotlin));
        assert_eq!(CodeLanguage::from_user_value("kotlin"), Some(CodeLanguage::Kotlin));
        assert_eq!(CodeLanguage::from_user_value("swift"), Some(CodeLanguage::Swift));
        assert_eq!(CodeLanguage::from_user_value("lua"), Some(CodeLanguage::Lua));
        assert_eq!(CodeLanguage::from_user_value("bash"), Some(CodeLanguage::Bash));
        assert_eq!(CodeLanguage::from_user_value("sh"), Some(CodeLanguage::Bash));
        assert_eq!(CodeLanguage::from_user_value("shell"), Some(CodeLanguage::Bash));
        assert_eq!(CodeLanguage::from_user_value("sql"), Some(CodeLanguage::Sql));
        assert_eq!(CodeLanguage::from_user_value("scala"), Some(CodeLanguage::Scala));
        assert_eq!(CodeLanguage::from_user_value("elixir"), Some(CodeLanguage::Elixir));
        assert_eq!(CodeLanguage::from_user_value("ex"), Some(CodeLanguage::Elixir));
        assert_eq!(CodeLanguage::from_user_value("exs"), Some(CodeLanguage::Elixir));
    }

    #[test]
    fn infers_new_languages_from_file_paths() {
        assert_eq!(CodeLanguage::from_path(Path::new("src/main.c")), Some(CodeLanguage::C));
        assert_eq!(CodeLanguage::from_path(Path::new("src/main.h")), Some(CodeLanguage::C));
        assert_eq!(CodeLanguage::from_path(Path::new("src/main.cpp")), Some(CodeLanguage::Cpp));
        assert_eq!(CodeLanguage::from_path(Path::new("src/main.cc")), Some(CodeLanguage::Cpp));
        assert_eq!(CodeLanguage::from_path(Path::new("src/main.hpp")), Some(CodeLanguage::Cpp));
        assert_eq!(CodeLanguage::from_path(Path::new("src/Main.cs")), Some(CodeLanguage::Csharp));
        assert_eq!(CodeLanguage::from_path(Path::new("styles/main.css")), Some(CodeLanguage::Css));
        assert_eq!(CodeLanguage::from_path(Path::new("templates/index.html")), Some(CodeLanguage::Html));
        assert_eq!(CodeLanguage::from_path(Path::new("data/config.json")), Some(CodeLanguage::Json));
        assert_eq!(CodeLanguage::from_path(Path::new("config.yml")), Some(CodeLanguage::Yaml));
        assert_eq!(CodeLanguage::from_path(Path::new("config.yaml")), Some(CodeLanguage::Yaml));
        assert_eq!(CodeLanguage::from_path(Path::new("app/models/user.rb")), Some(CodeLanguage::Ruby));
        assert_eq!(CodeLanguage::from_path(Path::new("src/index.php")), Some(CodeLanguage::Php));
        assert_eq!(CodeLanguage::from_path(Path::new("src/Main.kt")), Some(CodeLanguage::Kotlin));
        assert_eq!(CodeLanguage::from_path(Path::new("ios/App.swift")), Some(CodeLanguage::Swift));
        assert_eq!(CodeLanguage::from_path(Path::new("scripts/init.lua")), Some(CodeLanguage::Lua));
        assert_eq!(CodeLanguage::from_path(Path::new("scripts/deploy.sh")), Some(CodeLanguage::Bash));
        assert_eq!(CodeLanguage::from_path(Path::new("migrations/001.sql")), Some(CodeLanguage::Sql));
        assert_eq!(CodeLanguage::from_path(Path::new("src/Main.scala")), Some(CodeLanguage::Scala));
        assert_eq!(CodeLanguage::from_path(Path::new("lib/app.ex")), Some(CodeLanguage::Elixir));
    }

    #[test]
    fn normalizes_dockerfile_toml_hcl_aliases() {
        assert_eq!(CodeLanguage::from_user_value("dockerfile"), Some(CodeLanguage::Dockerfile));
        assert_eq!(CodeLanguage::from_user_value("docker"), Some(CodeLanguage::Dockerfile));
        assert_eq!(CodeLanguage::from_user_value("toml"), Some(CodeLanguage::Toml));
        assert_eq!(CodeLanguage::from_user_value("hcl"), Some(CodeLanguage::Hcl));
        assert_eq!(CodeLanguage::from_user_value("terraform"), Some(CodeLanguage::Hcl));
        assert_eq!(CodeLanguage::from_user_value("tf"), Some(CodeLanguage::Hcl));
    }

    #[test]
    fn normalizes_dart_zig_protobuf_aliases() {
        assert_eq!(CodeLanguage::from_user_value("dart"), Some(CodeLanguage::Dart));
        assert_eq!(CodeLanguage::from_user_value("zig"), Some(CodeLanguage::Zig));
        assert_eq!(CodeLanguage::from_user_value("protobuf"), Some(CodeLanguage::Protobuf));
        assert_eq!(CodeLanguage::from_user_value("proto"), Some(CodeLanguage::Protobuf));
    }

    #[test]
    fn infers_dockerfile_toml_hcl_from_file_paths() {
        assert_eq!(CodeLanguage::from_path(Path::new("Dockerfile")), Some(CodeLanguage::Dockerfile));
        assert_eq!(CodeLanguage::from_path(Path::new("Cargo.toml")), Some(CodeLanguage::Toml));
        assert_eq!(CodeLanguage::from_path(Path::new("main.tf")), Some(CodeLanguage::Hcl));
        assert_eq!(CodeLanguage::from_path(Path::new("vars.tfvars")), Some(CodeLanguage::Hcl));
    }

    #[test]
    fn infers_dart_zig_protobuf_from_file_paths() {
        assert_eq!(CodeLanguage::from_path(Path::new("lib/main.dart")), Some(CodeLanguage::Dart));
        assert_eq!(CodeLanguage::from_path(Path::new("src/main.zig")), Some(CodeLanguage::Zig));
        assert_eq!(CodeLanguage::from_path(Path::new("proto/service.proto")), Some(CodeLanguage::Protobuf));
    }

    #[test]
    fn normalizes_haskell_nix_solidity_aliases() {
        assert_eq!(CodeLanguage::from_user_value("hs"), Some(CodeLanguage::Haskell));
        assert_eq!(CodeLanguage::from_user_value("haskell"), Some(CodeLanguage::Haskell));
        assert_eq!(CodeLanguage::from_user_value("nix"), Some(CodeLanguage::Nix));
        assert_eq!(CodeLanguage::from_user_value("solidity"), Some(CodeLanguage::Solidity));
        assert_eq!(CodeLanguage::from_user_value("sol"), Some(CodeLanguage::Solidity));
    }

    #[test]
    fn infers_haskell_nix_solidity_from_file_paths() {
        assert_eq!(CodeLanguage::from_path(Path::new("src/Main.hs")), Some(CodeLanguage::Haskell));
        assert_eq!(CodeLanguage::from_path(Path::new("default.nix")), Some(CodeLanguage::Nix));
        assert_eq!(CodeLanguage::from_path(Path::new("contracts/Token.sol")), Some(CodeLanguage::Solidity));
    }

    #[test]
    fn infers_newly_added_extensions_for_existing_languages() {
        // Python - bzl
        assert_eq!(CodeLanguage::from_extension("bzl"), Some(CodeLanguage::Python));
        // Cpp - cu, ino
        assert_eq!(CodeLanguage::from_extension("cu"), Some(CodeLanguage::Cpp));
        assert_eq!(CodeLanguage::from_extension("ino"), Some(CodeLanguage::Cpp));
        // Html - xhtml
        assert_eq!(CodeLanguage::from_extension("xhtml"), Some(CodeLanguage::Html));
        // Ruby - rbw, gemspec
        assert_eq!(CodeLanguage::from_extension("rbw"), Some(CodeLanguage::Ruby));
        assert_eq!(CodeLanguage::from_extension("gemspec"), Some(CodeLanguage::Ruby));
        // Kotlin - ktm
        assert_eq!(CodeLanguage::from_extension("ktm"), Some(CodeLanguage::Kotlin));
        // Bash - bats, ksh
        assert_eq!(CodeLanguage::from_extension("bats"), Some(CodeLanguage::Bash));
        assert_eq!(CodeLanguage::from_extension("ksh"), Some(CodeLanguage::Bash));
        // Scala - sbt
        assert_eq!(CodeLanguage::from_extension("sbt"), Some(CodeLanguage::Scala));
    }

    #[test]
    fn infers_compound_extensions_from_paths() {
        // sh.in -> Bash
        assert_eq!(CodeLanguage::from_path(Path::new("scripts/config.sh.in")), Some(CodeLanguage::Bash));
        // c++ -> Cpp
        assert_eq!(CodeLanguage::from_path(Path::new("src/main.c++")), Some(CodeLanguage::Cpp));
    }
}
