use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Tool search algorithm for Anthropic's advanced-tool-use beta
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolSearchAlgorithm {
    /// Regex-based search using Python re.search() syntax
    #[default]
    Regex,
    /// BM25-based natural language search
    Bm25,
}

impl std::fmt::Display for ToolSearchAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Regex => write!(f, "regex"),
            Self::Bm25 => write!(f, "bm25"),
        }
    }
}

impl std::str::FromStr for ToolSearchAlgorithm {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "regex" => Ok(Self::Regex),
            "bm25" => Ok(Self::Bm25),
            _ => Err(format!("Unknown tool search algorithm: {}", s)),
        }
    }
}

/// Universal tool definition that matches OpenAI/Anthropic/Gemini specifications
/// Based on official API documentation from Context7
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// The type of tool: "function", "apply_patch" (GPT-5.1), "shell" (GPT-5.1), or "custom" (GPT-5 freeform)
    /// Also supports Anthropic tool types like:
    /// - "tool_search_tool_regex_20251119", "tool_search_tool_bm25_20251119"
    /// - "web_search_20260209" (and other web_search_* revisions)
    #[serde(rename = "type")]
    pub tool_type: String,

    /// Function definition containing name, description, and parameters
    /// Used for "function", "apply_patch", and "custom" types
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionDefinition>,

    /// Provider-native web search configuration payload (e.g. Z.AI `web_search` tool).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search: Option<Value>,

    /// Shell tool configuration (GPT-5.1 specific)
    /// Describes shell command capabilities and constraints
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<ShellToolDefinition>,

    /// Grammar definition for context-free grammar constraints (GPT-5 specific)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grammar: Option<GrammarDefinition>,

    /// When true and using Anthropic, mark the tool as strict for structured tool use validation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,

    /// When true, the tool is deferred and only loaded when discovered via tool search (Anthropic advanced-tool-use beta)
    /// This enables dynamic tool discovery for large tool catalogs (10k+ tools)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_loading: Option<bool>,
}

/// Shell tool definition for GPT-5.1 shell tool type
/// Allows controlled command-line interface interactions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellToolDefinition {
    /// Description of shell tool capabilities
    pub description: String,

    /// List of allowed commands (whitelist for safety)
    pub allowed_commands: Vec<String>,

    /// List of forbidden commands (blacklist for safety)
    pub forbidden_patterns: Vec<String>,

    /// Maximum command timeout in seconds
    pub timeout_seconds: u32,
}

/// Grammar definition for GPT-5 context-free grammar (CFG) constraints
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrammarDefinition {
    /// The syntax of the grammar: "lark" or "regex"
    pub syntax: String,

    /// The grammar definition in the specified syntax
    pub definition: String,
}

impl Default for GrammarDefinition {
    fn default() -> Self {
        Self {
            syntax: "lark".into(),
            definition: String::new(),
        }
    }
}

impl Default for ShellToolDefinition {
    fn default() -> Self {
        Self {
            description: "Execute shell commands in the workspace".into(),
            allowed_commands: vec![
                "ls".into(),
                "find".into(),
                "grep".into(),
                "cargo".into(),
                "git".into(),
                "python".into(),
                "node".into(),
            ],
            forbidden_patterns: vec!["rm -rf".into(), "sudo".into(), "passwd".into()],
            timeout_seconds: 30,
        }
    }
}

/// Function definition within a tool
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// The name of the function to be called
    pub name: String,

    /// A description of what the function does
    pub description: String,

    /// The parameters the function accepts, described as a JSON Schema object
    pub parameters: Value,
}

pub(crate) fn sanitize_tool_description(description: &str) -> String {
    let mut result = String::with_capacity(description.len());
    let mut first = true;
    for line in description.lines() {
        if !first {
            result.push('\n');
        }
        result.push_str(line.trim_end());
        first = false;
    }
    result.trim().to_owned()
}

impl ToolDefinition {
    /// Create a new tool definition with function type
    pub fn function(name: String, description: String, parameters: Value) -> Self {
        let sanitized_description = sanitize_tool_description(&description);
        Self {
            tool_type: "function".to_owned(),
            function: Some(FunctionDefinition {
                name,
                description: sanitized_description,
                parameters,
            }),
            web_search: None,
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
        }
    }

    /// Set whether the tool should be considered strict (Anthropic structured tool use)
    pub fn with_strict(mut self, strict: bool) -> Self {
        self.strict = Some(strict);
        self
    }

    /// Set whether the tool should be deferred (Anthropic tool search)
    pub fn with_defer_loading(mut self, defer: bool) -> Self {
        self.defer_loading = Some(defer);
        self
    }

    /// Create a tool search tool definition for Anthropic's advanced-tool-use beta
    /// Supports regex and bm25 search algorithms
    pub fn tool_search(algorithm: ToolSearchAlgorithm) -> Self {
        let (tool_type, name) = match algorithm {
            ToolSearchAlgorithm::Regex => {
                ("tool_search_tool_regex_20251119", "tool_search_tool_regex")
            }
            ToolSearchAlgorithm::Bm25 => {
                ("tool_search_tool_bm25_20251119", "tool_search_tool_bm25")
            }
        };

        Self {
            tool_type: tool_type.to_owned(),
            function: Some(FunctionDefinition {
                name: name.to_owned(),
                description: "Search for tools by name, description, or parameters".to_owned(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query (regex pattern for regex variant, natural language for bm25)"
                        }
                    },
                    "required": ["query"]
                }),
            }),
            web_search: None,
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
        }
    }

    /// Create a new apply_patch tool definition (GPT-5.1 specific)
    /// The apply_patch tool lets models create, update, and delete files using VT Code structured diffs
    pub fn apply_patch(description: String) -> Self {
        let sanitized_description = sanitize_tool_description(&description);
        Self {
            tool_type: "apply_patch".to_owned(),
            function: Some(FunctionDefinition {
                name: "apply_patch".to_owned(),
                description: sanitized_description,
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "input": {
                            "type": "string",
                            "description": "Patch in VT Code format. MUST use *** Begin Patch, *** Update File: path, @@ context, -/+ lines, *** End Patch. Do NOT use unified diff (---/+++) format."
                        },
                        "patch": {
                            "type": "string",
                            "description": "Alias for input parameter"
                        }
                    },
                    "anyOf": [
                        {"required": ["input"]},
                        {"required": ["patch"]}
                    ]
                }),
            }),
            web_search: None,
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
        }
    }

    /// Create a new custom tool definition for freeform function calling (GPT-5 specific)
    /// Allows raw text payloads without JSON wrapping
    pub fn custom(name: String, description: String) -> Self {
        let sanitized_description = sanitize_tool_description(&description);
        Self {
            tool_type: "custom".to_owned(),
            function: Some(FunctionDefinition {
                name,
                description: sanitized_description,
                parameters: json!({}), // Custom tools may not need parameters
            }),
            web_search: None,
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
        }
    }

    /// Create a new grammar tool definition for context-free grammar constraints (GPT-5 specific)
    /// Ensures model output matches predefined syntax
    pub fn grammar(syntax: String, definition: String) -> Self {
        Self {
            tool_type: "grammar".to_owned(),
            function: None,
            web_search: None,
            shell: None,
            grammar: Some(GrammarDefinition { syntax, definition }),
            strict: None,
            defer_loading: None,
        }
    }

    /// Create a provider-native web search tool definition.
    pub fn web_search(config: Value) -> Self {
        Self {
            tool_type: "web_search".to_owned(),
            function: None,
            web_search: Some(config),
            shell: None,
            grammar: None,
            strict: None,
            defer_loading: None,
        }
    }

    /// Get the function name for easy access
    pub fn function_name(&self) -> &str {
        if let Some(func) = &self.function {
            &func.name
        } else {
            &self.tool_type
        }
    }

    /// Get the description for easy access
    pub fn description(&self) -> &str {
        if let Some(func) = &self.function {
            &func.description
        } else if let Some(shell) = &self.shell {
            &shell.description
        } else {
            ""
        }
    }

    /// Validate that this tool definition is properly formed
    pub fn validate(&self) -> Result<(), String> {
        match self.tool_type.as_str() {
            "function" => self.validate_function(),
            "apply_patch" => self.validate_apply_patch(),
            "shell" => self.validate_shell(),
            "custom" => self.validate_custom(),
            "grammar" => self.validate_grammar(),
            "web_search" => self.validate_web_search(),
            "tool_search_tool_regex_20251119" | "tool_search_tool_bm25_20251119" => {
                self.validate_function()
            }
            other if other.starts_with("web_search_") => Ok(()),
            other => Err(format!(
                "Unsupported tool type: {}. Supported types: function, apply_patch, shell, custom, grammar, web_search, tool_search_tool_*, web_search_*",
                other
            )),
        }
    }

    /// Returns true if this is a tool search tool type
    pub fn is_tool_search(&self) -> bool {
        matches!(
            self.tool_type.as_str(),
            "tool_search_tool_regex_20251119" | "tool_search_tool_bm25_20251119"
        )
    }

    /// Returns true when the tool is an Anthropic native web search tool revision.
    pub fn is_anthropic_web_search(&self) -> bool {
        self.tool_type.starts_with("web_search_")
    }

    fn validate_function(&self) -> Result<(), String> {
        if let Some(func) = &self.function {
            if func.name.is_empty() {
                return Err("Function name cannot be empty".to_owned());
            }
            if func.description.is_empty() {
                return Err("Function description cannot be empty".to_owned());
            }
            if !func.parameters.is_object() {
                return Err("Function parameters must be a JSON object".to_owned());
            }
            Ok(())
        } else {
            Err("Function tool missing function definition".to_owned())
        }
    }

    fn validate_apply_patch(&self) -> Result<(), String> {
        if let Some(func) = &self.function {
            if func.name != "apply_patch" {
                return Err(format!(
                    "apply_patch tool must have name 'apply_patch', got: {}",
                    func.name
                ));
            }
            if func.description.is_empty() {
                return Err("apply_patch description cannot be empty".to_owned());
            }
            Ok(())
        } else {
            Err("apply_patch tool missing function definition".to_owned())
        }
    }

    fn validate_shell(&self) -> Result<(), String> {
        if let Some(shell) = &self.shell {
            if shell.description.is_empty() {
                return Err("Shell tool description cannot be empty".to_owned());
            }
            if shell.timeout_seconds == 0 {
                return Err("Shell tool timeout must be greater than 0".to_owned());
            }
            Ok(())
        } else {
            Err("Shell tool missing shell definition".to_owned())
        }
    }

    fn validate_custom(&self) -> Result<(), String> {
        if let Some(func) = &self.function {
            if func.name.is_empty() {
                return Err("Custom tool name cannot be empty".to_owned());
            }
            if func.description.is_empty() {
                return Err("Custom tool description cannot be empty".to_owned());
            }
            Ok(())
        } else {
            Err("Custom tool missing function definition".to_owned())
        }
    }

    fn validate_grammar(&self) -> Result<(), String> {
        if let Some(grammar) = &self.grammar {
            if !["lark", "regex"].contains(&grammar.syntax.as_str()) {
                return Err("Grammar syntax must be 'lark' or 'regex'".to_owned());
            }
            if grammar.definition.is_empty() {
                return Err("Grammar definition cannot be empty".to_owned());
            }
            Ok(())
        } else {
            Err("Grammar tool missing grammar definition".to_owned())
        }
    }

    fn validate_web_search(&self) -> Result<(), String> {
        if self.web_search.is_some() {
            Ok(())
        } else {
            Err("web_search tool missing web_search configuration".to_owned())
        }
    }
}
