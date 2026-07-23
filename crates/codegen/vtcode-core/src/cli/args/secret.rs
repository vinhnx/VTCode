use clap::{Args, Subcommand, ValueEnum};

/// Supported provider names for secret management
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum SecretProvider {
    #[value(name = "openai")]
    OpenAI,
    #[value(name = "anthropic")]
    Anthropic,
    #[value(name = "gemini")]
    Gemini,
    #[value(name = "deepseek")]
    DeepSeek,
    #[value(name = "openrouter")]
    OpenRouter,
    #[value(name = "stepfun")]
    StepFun,
    #[value(name = "zai")]
    Zai,
    #[value(name = "moonshot")]
    Moonshot,
    #[value(name = "minimax")]
    MiniMax,
    #[value(name = "mistral")]
    Mistral,
    #[value(name = "huggingface")]
    HuggingFace,
    #[value(name = "mimo")]
    MiMo,
    #[value(name = "opencode-zen")]
    OpenCodeZen,
    #[value(name = "opencode-go")]
    OpenCodeGo,
    #[value(name = "qwen")]
    Qwen,
    #[value(name = "evolink")]
    Evolink,
    #[value(name = "poolside")]
    Poolside,
    #[value(name = "ollama")]
    Ollama,
    #[value(name = "ollama-cloud")]
    OllamaCloud,
    #[value(name = "lmstudio")]
    LMStudio,
    #[value(name = "copilot")]
    Copilot,
}

impl SecretProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            SecretProvider::OpenAI => "openai",
            SecretProvider::Anthropic => "anthropic",
            SecretProvider::Gemini => "gemini",
            SecretProvider::DeepSeek => "deepseek",
            SecretProvider::OpenRouter => "openrouter",
            SecretProvider::StepFun => "stepfun",
            SecretProvider::Zai => "zai",
            SecretProvider::Moonshot => "moonshot",
            SecretProvider::MiniMax => "minimax",
            SecretProvider::Mistral => "mistral",
            SecretProvider::HuggingFace => "huggingface",
            SecretProvider::MiMo => "mimo",
            SecretProvider::OpenCodeZen => "opencode-zen",
            SecretProvider::OpenCodeGo => "opencode-go",
            SecretProvider::Qwen => "qwen",
            SecretProvider::Evolink => "evolink",
            SecretProvider::Poolside => "poolside",
            SecretProvider::Ollama => "ollama",
            SecretProvider::OllamaCloud => "ollama-cloud",
            SecretProvider::LMStudio => "lmstudio",
            SecretProvider::Copilot => "copilot",
        }
    }
}

/// Secret management subcommands
#[derive(Debug, Subcommand, Clone)]
pub enum SecretSubcommand {
    /// List secret status for all providers
    #[command(name = "list", visible_alias = "ls")]
    List,

    /// Show status for a specific provider
    #[command(name = "status", visible_alias = "info")]
    Status {
        /// Provider name (e.g. openai, anthropic, stepfun)
        provider_name: Option<SecretProvider>,
    },

    /// Store an API key in secure storage
    #[command(name = "add", visible_alias = "set")]
    Add {
        /// Provider name (e.g. openai, anthropic, stepfun)
        provider_name: SecretProvider,
    },

    /// Remove a stored API key from secure storage
    #[command(name = "delete", visible_alias = "remove")]
    Delete {
        /// Provider name (e.g. openai, anthropic, stepfun)
        provider_name: SecretProvider,
    },

    /// Migrate API keys from workspace .env to secure storage
    #[command(name = "migrate")]
    Migrate(MigrateArgs),
}

/// Arguments for `vtcode secret migrate`.
#[derive(Debug, Args, Clone)]
pub struct MigrateArgs {
    /// Provider name (e.g. openai, anthropic). Omit to migrate all found keys.
    pub provider_name: Option<SecretProvider>,

    /// Migrate all found keys without prompting
    #[arg(long)]
    pub all: bool,

    /// Preview migration without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Skip confirmation prompts
    #[arg(long)]
    pub force: bool,
}

/// Top-level secret command args (allows bare `vtcode secret` to default to list)
#[derive(Debug, Args, Clone)]
pub struct SecretArgs {
    #[command(subcommand)]
    pub command: Option<SecretSubcommand>,
}
