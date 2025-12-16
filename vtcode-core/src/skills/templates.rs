//! Skill Template System
//!
//! Provides templates for common skill patterns, enabling rapid skill development
//! and standardization across the VTCode ecosystem.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Skill template types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TemplateType {
    /// Traditional VTCode skill with SKILL.md
    Traditional,
    /// CLI tool integration skill
    CliTool,
    /// Code generation skill
    CodeGenerator,
    /// Data processing skill
    DataProcessor,
    /// Testing utility skill
    TestingUtility,
    /// Documentation generator
    DocumentationGenerator,
    /// Build automation skill
    BuildAutomation,
    /// Custom template
    Custom(String),
}

/// Template variable for customization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateVariable {
    /// Variable name
    pub name: String,

    /// Variable description
    pub description: String,

    /// Default value
    pub default_value: Option<String>,

    /// Whether variable is required
    pub required: bool,

    /// Validation pattern (regex)
    pub validation_pattern: Option<String>,

    /// Example values
    pub examples: Vec<String>,
}

/// Skill template configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTemplate {
    /// Template name
    pub name: String,

    /// Template type
    pub template_type: TemplateType,

    /// Template description
    pub description: String,

    /// Template version
    pub version: String,

    /// Variables for customization
    pub variables: Vec<TemplateVariable>,

    /// File structure template
    pub file_structure: FileStructure,

    /// Default metadata
    pub default_metadata: HashMap<String, String>,

    /// Instructions template
    pub instructions_template: String,

    /// Example usage
    pub example_usage: String,
}

/// File structure template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileStructure {
    /// Directories to create
    pub directories: Vec<String>,

    /// Files to create with templates
    pub files: HashMap<String, String>,

    /// Executable files (scripts, tools)
    pub executables: HashMap<String, String>,
}

/// Template engine for skill generation
pub struct TemplateEngine {
    templates: HashMap<String, SkillTemplate>,
}

impl TemplateEngine {
    /// Create new template engine with built-in templates
    pub fn new() -> Self {
        let mut engine = Self {
            templates: HashMap::new(),
        };

        // Register built-in templates
        engine.register_builtin_templates();
        engine
    }

    /// Register built-in templates
    fn register_builtin_templates(&mut self) {
        // Traditional skill template
        self.templates.insert(
            "traditional".to_string(),
            Self::create_traditional_template(),
        );

        // CLI tool template
        self.templates
            .insert("cli-tool".to_string(), Self::create_cli_tool_template());

        // Code generator template
        self.templates.insert(
            "code-generator".to_string(),
            Self::create_code_generator_template(),
        );

        // Data processor template
        self.templates.insert(
            "data-processor".to_string(),
            Self::create_data_processor_template(),
        );

        // Testing utility template
        self.templates.insert(
            "testing-utility".to_string(),
            Self::create_testing_utility_template(),
        );
    }

    /// Create traditional skill template
    fn create_traditional_template() -> SkillTemplate {
        SkillTemplate {
            name: "Traditional Skill".to_string(),
            template_type: TemplateType::Traditional,
            description: "Standard VTCode skill with SKILL.md format".to_string(),
            version: "1.0.0".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "skill_name".to_string(),
                    description: "Name of the skill".to_string(),
                    default_value: None,
                    required: true,
                    validation_pattern: Some(r"^[a-z][a-z0-9-]*$".to_string()),
                    examples: vec!["file-manager".to_string(), "code-analyzer".to_string()],
                },
                TemplateVariable {
                    name: "description".to_string(),
                    description: "Skill description".to_string(),
                    default_value: None,
                    required: true,
                    validation_pattern: None,
                    examples: vec!["Manages files and directories".to_string()],
                },
                TemplateVariable {
                    name: "author".to_string(),
                    description: "Skill author".to_string(),
                    default_value: Some("VTCode User".to_string()),
                    required: false,
                    validation_pattern: None,
                    examples: vec![],
                },
                TemplateVariable {
                    name: "version".to_string(),
                    description: "Skill version".to_string(),
                    default_value: Some("1.0.0".to_string()),
                    required: false,
                    validation_pattern: Some(r"^\d+\.\d+\.\d+$".to_string()),
                    examples: vec![],
                },
            ],
            file_structure: FileStructure {
                directories: vec!["scripts".to_string(), "templates".to_string()],
                files: HashMap::from([
                    (
                        "SKILL.md".to_string(),
                        include_str!("../../templates/traditional/SKILL.md.template").to_string(),
                    ),
                    (
                        "README.md".to_string(),
                        include_str!("../../templates/traditional/README.md.template").to_string(),
                    ),
                ]),
                executables: HashMap::from([(
                    "scripts/helper.py".to_string(),
                    include_str!("../../templates/traditional/scripts/helper.py.template")
                        .to_string(),
                )]),
            },
            default_metadata: HashMap::from([
                ("category".to_string(), "utility".to_string()),
                ("tags".to_string(), "general,purpose".to_string()),
            ]),
            instructions_template: include_str!(
                "../../templates/traditional/instructions.md.template"
            )
            .to_string(),
            example_usage: include_str!("../../templates/traditional/example_usage.md.template")
                .to_string(),
        }
    }

    /// Create CLI tool template
    fn create_cli_tool_template() -> SkillTemplate {
        SkillTemplate {
            name: "CLI Tool Integration".to_string(),
            template_type: TemplateType::CliTool,
            description: "Integrate external CLI tools as VTCode skills".to_string(),
            version: "1.0.0".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "tool_name".to_string(),
                    description: "Name of the CLI tool".to_string(),
                    default_value: None,
                    required: true,
                    validation_pattern: Some(r"^[a-z][a-z0-9-]*$".to_string()),
                    examples: vec!["curl-wrapper".to_string(), "git-helper".to_string()],
                },
                TemplateVariable {
                    name: "tool_description".to_string(),
                    description: "Description of what the tool does".to_string(),
                    default_value: None,
                    required: true,
                    validation_pattern: None,
                    examples: vec!["Wrapper around curl for HTTP requests".to_string()],
                },
                TemplateVariable {
                    name: "tool_command".to_string(),
                    description: "The CLI command to execute".to_string(),
                    default_value: None,
                    required: true,
                    validation_pattern: None,
                    examples: vec!["curl".to_string(), "git".to_string(), "python".to_string()],
                },
                TemplateVariable {
                    name: "supports_json".to_string(),
                    description: "Whether the tool supports JSON I/O".to_string(),
                    default_value: Some("false".to_string()),
                    required: false,
                    validation_pattern: Some(r"^(true|false)$".to_string()),
                    examples: vec![],
                },
            ],
            file_structure: FileStructure {
                directories: vec![],
                files: HashMap::from([
                    (
                        "tool.json".to_string(),
                        include_str!("../../templates/cli-tool/tool.json.template").to_string(),
                    ),
                    (
                        "README.md".to_string(),
                        include_str!("../../templates/cli-tool/README.md.template").to_string(),
                    ),
                ]),
                executables: HashMap::from([(
                    "tool.sh".to_string(),
                    include_str!("../../templates/cli-tool/tool.sh.template").to_string(),
                )]),
            },
            default_metadata: HashMap::from([
                ("category".to_string(), "cli-tool".to_string()),
                ("tags".to_string(), "external,tool".to_string()),
            ]),
            instructions_template: include_str!(
                "../../templates/cli-tool/instructions.md.template"
            )
            .to_string(),
            example_usage: include_str!("../../templates/cli-tool/example_usage.md.template")
                .to_string(),
        }
    }

    /// Create code generator template
    fn create_code_generator_template() -> SkillTemplate {
        SkillTemplate {
            name: "Code Generator".to_string(),
            template_type: TemplateType::CodeGenerator,
            description: "Generate code from templates and patterns".to_string(),
            version: "1.0.0".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "generator_name".to_string(),
                    description: "Name of the code generator".to_string(),
                    default_value: None,
                    required: true,
                    validation_pattern: Some(r"^[a-z][a-z0-9-]*$".to_string()),
                    examples: vec!["api-generator".to_string(), "test-generator".to_string()],
                },
                TemplateVariable {
                    name: "target_language".to_string(),
                    description: "Target programming language".to_string(),
                    default_value: Some("rust".to_string()),
                    required: false,
                    validation_pattern: None,
                    examples: vec!["rust".to_string(), "python".to_string(), "typescript".to_string()],
                },
                TemplateVariable {
                    name: "template_engine".to_string(),
                    description: "Template engine to use".to_string(),
                    default_value: Some("handlebars".to_string()),
                    required: false,
                    validation_pattern: None,
                    examples: vec!["handlebars".to_string(), "jinja2".to_string(), "mustache".to_string()],
                },
            ],
            file_structure: FileStructure {
                directories: vec![
                    "templates".to_string(),
                    "examples".to_string(),
                ],
                files: HashMap::from([
                    ("SKILL.md".to_string(), "# {{generator_name}}\n\nGenerate {{target_language}} code from templates.".to_string()),
                    ("generator.py".to_string(), "#!/usr/bin/env python3\n# Code generator implementation".to_string()),
                ]),
                executables: HashMap::from([
                    ("generate.sh".to_string(), "#!/bin/bash\necho 'Generating code...'".to_string()),
                ]),
            },
            default_metadata: HashMap::from([
                ("category".to_string(), "generator".to_string()),
                ("tags".to_string(), "code,generation,templates".to_string()),
            ]),
            instructions_template: "# {{generator_name}} Instructions\n\nGenerate {{target_language}} code from templates using {{template_engine}}.".to_string(),
            example_usage: "## Example Usage\n\nGenerate code using the {{generator_name}} skill with {{template_engine}} templates.".to_string(),
        }
    }

    /// Create data processor template
    fn create_data_processor_template() -> SkillTemplate {
        SkillTemplate {
            name: "Data Processor".to_string(),
            template_type: TemplateType::DataProcessor,
            description: "Process and transform data files".to_string(),
            version: "1.0.0".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "processor_name".to_string(),
                    description: "Name of the data processor".to_string(),
                    default_value: None,
                    required: true,
                    validation_pattern: Some(r"^[a-z][a-z0-9-]*$".to_string()),
                    examples: vec!["csv-processor".to_string(), "json-transformer".to_string()],
                },
                TemplateVariable {
                    name: "input_format".to_string(),
                    description: "Input data format".to_string(),
                    default_value: Some("json".to_string()),
                    required: false,
                    validation_pattern: None,
                    examples: vec!["json".to_string(), "csv".to_string(), "xml".to_string()],
                },
                TemplateVariable {
                    name: "output_format".to_string(),
                    description: "Output data format".to_string(),
                    default_value: Some("json".to_string()),
                    required: false,
                    validation_pattern: None,
                    examples: vec!["json".to_string(), "csv".to_string(), "parquet".to_string()],
                },
            ],
            file_structure: FileStructure {
                directories: vec![
                    "processors".to_string(),
                    "schemas".to_string(),
                ],
                files: HashMap::from([
                    ("SKILL.md".to_string(), "# {{processor_name}}\n\nProcess {{input_format}} data and output {{output_format}}.".to_string()),
                    ("processor.py".to_string(), "#!/usr/bin/env python3\n# Data processor implementation".to_string()),
                ]),
                executables: HashMap::from([
                    ("process.sh".to_string(), "#!/bin/bash\necho 'Processing data...'".to_string()),
                ]),
            },
            default_metadata: HashMap::from([
                ("category".to_string(), "processor".to_string()),
                ("tags".to_string(), "data,processing,transformation".to_string()),
            ]),
            instructions_template: "# {{processor_name}} Instructions\n\nProcess data from {{input_format}} to {{output_format}} format.".to_string(),
            example_usage: "## Example Usage\n\nProcess data using the {{processor_name}} skill.".to_string(),
        }
    }

    /// Create testing utility template
    fn create_testing_utility_template() -> SkillTemplate {
        SkillTemplate {
            name: "Testing Utility".to_string(),
            template_type: TemplateType::TestingUtility,
            description: "Testing and quality assurance utilities".to_string(),
            version: "1.0.0".to_string(),
            variables: vec![
                TemplateVariable {
                    name: "tester_name".to_string(),
                    description: "Name of the testing utility".to_string(),
                    default_value: None,
                    required: true,
                    validation_pattern: Some(r"^[a-z][a-z0-9-]*$".to_string()),
                    examples: vec!["unit-tester".to_string(), "integration-runner".to_string()],
                },
                TemplateVariable {
                    name: "test_framework".to_string(),
                    description: "Testing framework to use".to_string(),
                    default_value: Some("pytest".to_string()),
                    required: false,
                    validation_pattern: None,
                    examples: vec!["pytest".to_string(), "jest".to_string(), "cargo-test".to_string()],
                },
                TemplateVariable {
                    name: "coverage_tool".to_string(),
                    description: "Code coverage tool".to_string(),
                    default_value: Some("coverage".to_string()),
                    required: false,
                    validation_pattern: None,
                    examples: vec!["coverage".to_string(), "istanbul".to_string(), "tarpaulin".to_string()],
                },
            ],
            file_structure: FileStructure {
                directories: vec![
                    "tests".to_string(),
                    "configs".to_string(),
                ],
                files: HashMap::from([
                    ("SKILL.md".to_string(), "# {{tester_name}}\n\n{{test_framework}} testing utility for quality assurance.".to_string()),
                    ("test_runner.py".to_string(), "#!/usr/bin/env python3\n# Test runner implementation".to_string()),
                ]),
                executables: HashMap::from([
                    ("run_tests.sh".to_string(), "#!/bin/bash\necho 'Running tests...'".to_string()),
                ]),
            },
            default_metadata: HashMap::from([
                ("category".to_string(), "testing".to_string()),
                ("tags".to_string(), "testing,quality,assurance".to_string()),
            ]),
            instructions_template: "# {{tester_name}} Instructions\n\nRun tests using {{test_framework}} with {{coverage_tool}} coverage.".to_string(),
            example_usage: "## Example Usage\n\nRun tests using the {{tester_name}} skill with {{test_framework}}.".to_string(),
        }
    }

    /// Get available template names
    pub fn get_template_names(&self) -> Vec<String> {
        self.templates.keys().cloned().collect()
    }

    /// Get template by name
    pub fn get_template(&self, name: &str) -> Option<&SkillTemplate> {
        self.templates.get(name)
    }

    /// Generate skill from template
    pub fn generate_skill(
        &self,
        template_name: &str,
        variables: HashMap<String, String>,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        let template = self
            .get_template(template_name)
            .ok_or_else(|| anyhow!("Template '{}' not found", template_name))?;

        // Validate required variables
        self.validate_variables(template, &variables)?;

        // Create output directory
        let skill_name = variables
            .get("skill_name")
            .or_else(|| variables.get("tool_name"))
            .or_else(|| variables.get("generator_name"))
            .or_else(|| variables.get("processor_name"))
            .or_else(|| variables.get("tester_name"))
            .ok_or_else(|| anyhow!("No skill name variable found"))?;

        let skill_dir = output_dir.join(skill_name);
        std::fs::create_dir_all(&skill_dir)?;

        info!(
            "Generating skill '{}' from template '{}' in {}",
            skill_name,
            template_name,
            skill_dir.display()
        );

        // Create directory structure
        for dir in &template.file_structure.directories {
            let dir_path = skill_dir.join(dir);
            std::fs::create_dir_all(&dir_path)?;
        }

        // Generate files
        for (file_path, content_template) in &template.file_structure.files {
            let content = self.render_template(content_template, &variables)?;
            let full_path = skill_dir.join(file_path);

            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            std::fs::write(&full_path, content)?;
            debug!("Generated file: {}", full_path.display());
        }

        // Generate executable files
        for (file_path, content_template) in &template.file_structure.executables {
            let content = self.render_template(content_template, &variables)?;
            let full_path = skill_dir.join(file_path);

            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            std::fs::write(&full_path, content)?;

            // Make executable on Unix systems
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&full_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&full_path, perms)?;
            }

            debug!("Generated executable: {}", full_path.display());
        }

        // Generate SKILL.md if not already generated
        if !template.file_structure.files.contains_key("SKILL.md") {
            let skill_md = self.generate_skill_md(template, &variables)?;
            let skill_md_path = skill_dir.join("SKILL.md");
            std::fs::write(&skill_md_path, skill_md)?;
            debug!("Generated SKILL.md: {}", skill_md_path.display());
        }

        info!(
            "Successfully generated skill '{}' in {}",
            skill_name,
            skill_dir.display()
        );
        Ok(skill_dir)
    }

    /// Validate template variables
    fn validate_variables(
        &self,
        template: &SkillTemplate,
        variables: &HashMap<String, String>,
    ) -> Result<()> {
        let mut missing_required = vec![];
        let mut invalid_values = vec![];

        for variable in &template.variables {
            if let Some(value) = variables.get(&variable.name) {
                // Validate pattern if specified
                if let Some(pattern) = &variable.validation_pattern {
                    let regex = regex::Regex::new(pattern).with_context(|| {
                        format!(
                            "Invalid validation pattern for variable '{}'",
                            variable.name
                        )
                    })?;

                    if !regex.is_match(value) {
                        invalid_values.push(format!(
                            "Variable '{}' value '{}' does not match pattern '{}'",
                            variable.name, value, pattern
                        ));
                    }
                }
            } else if variable.required && variable.default_value.is_none() {
                missing_required.push(variable.name.clone());
            }
        }

        if !missing_required.is_empty() {
            return Err(anyhow!(
                "Missing required variables: {}",
                missing_required.join(", ")
            ));
        }

        if !invalid_values.is_empty() {
            return Err(anyhow!(
                "Invalid variable values: {}",
                invalid_values.join(", ")
            ));
        }

        Ok(())
    }

    /// Render template with variables
    fn render_template(
        &self,
        template: &str,
        variables: &HashMap<String, String>,
    ) -> Result<String> {
        let mut rendered = template.to_string();

        // Simple variable substitution: {{variable_name}}
        for (key, value) in variables {
            let placeholder = format!("{{{{{}}}}}", key);
            rendered = rendered.replace(&placeholder, value);
        }

        // Replace default values for missing variables
        // This would need template-specific logic

        Ok(rendered)
    }

    /// Generate SKILL.md content
    fn generate_skill_md(
        &self,
        template: &SkillTemplate,
        variables: &HashMap<String, String>,
    ) -> Result<String> {
        let mut content = String::new();

        // YAML frontmatter
        content.push_str("---\n");

        // Add metadata from template
        for (key, default_value) in &template.default_metadata {
            let value = variables.get(key).unwrap_or(default_value);
            content.push_str(&format!("{}: {}\n", key, value));
        }

        // Add required fields
        let skill_name = variables
            .get("skill_name")
            .or_else(|| variables.get("tool_name"))
            .or_else(|| variables.get("generator_name"))
            .or_else(|| variables.get("processor_name"))
            .or_else(|| variables.get("tester_name"))
            .ok_or_else(|| anyhow!("No skill name found"))?;

        let default_desc = "A VTCode skill".to_string();
        let description = variables
            .get("description")
            .or_else(|| variables.get("tool_description"))
            .unwrap_or(&default_desc);

        content.push_str(&format!("name: {}\n", skill_name));
        content.push_str(&format!("description: {}\n", description));

        // Add optional fields
        if let Some(author) = variables.get("author") {
            content.push_str(&format!("author: {}\n", author));
        }

        if let Some(version) = variables.get("version") {
            content.push_str(&format!("version: {}\n", version));
        }

        content.push_str("---\n\n");

        // Add instructions
        content.push_str(&template.instructions_template);
        content.push('\n');

        // Add example usage
        content.push_str("\n## Example Usage\n\n");
        content.push_str(&template.example_usage);

        // Render variables in the content
        self.render_template(&content, variables)
    }

    /// Register custom template
    pub fn register_template(&mut self, template: SkillTemplate) -> Result<()> {
        info!("Registering custom template: {}", template.name);
        self.templates.insert(template.name.clone(), template);
        Ok(())
    }

    /// Load template from file
    pub fn load_template_from_file(&mut self, path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read template file: {}", path.display()))?;

        let template: SkillTemplate = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse template from: {}", path.display()))?;

        self.register_template(template)
    }

    /// Save template to file
    pub fn save_template_to_file(&self, template_name: &str, path: &Path) -> Result<()> {
        let template = self
            .get_template(template_name)
            .ok_or_else(|| anyhow!("Template '{}' not found", template_name))?;

        let content = serde_json::to_string_pretty(template)
            .with_context(|| format!("Failed to serialize template '{}'", template_name))?;

        std::fs::write(path, content)
            .with_context(|| format!("Failed to write template to: {}", path.display()))?;

        info!("Saved template '{}' to {}", template_name, path.display());
        Ok(())
    }

    /// Get template statistics
    pub fn get_template_stats(&self) -> TemplateStats {
        let mut stats = TemplateStats::default();

        for template in self.templates.values() {
            stats.total_templates += 1;

            match &template.template_type {
                TemplateType::Traditional => stats.traditional_templates += 1,
                TemplateType::CliTool => stats.cli_tool_templates += 1,
                TemplateType::CodeGenerator => stats.code_generator_templates += 1,
                TemplateType::DataProcessor => stats.data_processor_templates += 1,
                TemplateType::TestingUtility => stats.testing_utility_templates += 1,
                TemplateType::DocumentationGenerator => {
                    stats.documentation_generator_templates += 1
                }
                TemplateType::BuildAutomation => stats.build_automation_templates += 1,
                TemplateType::Custom(_) => stats.custom_templates += 1,
            }

            stats.total_variables += template.variables.len() as u32;
        }

        stats
    }
}

/// Template statistics
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TemplateStats {
    pub total_templates: u32,
    pub traditional_templates: u32,
    pub cli_tool_templates: u32,
    pub code_generator_templates: u32,
    pub data_processor_templates: u32,
    pub testing_utility_templates: u32,
    pub documentation_generator_templates: u32,
    pub build_automation_templates: u32,
    pub custom_templates: u32,
    pub total_variables: u32,
}

/// Skill template builder for programmatic template creation
pub struct SkillTemplateBuilder {
    name: String,
    template_type: TemplateType,
    description: String,
    version: String,
    variables: Vec<TemplateVariable>,
    file_structure: FileStructure,
    default_metadata: HashMap<String, String>,
    instructions_template: String,
    example_usage: String,
}

impl SkillTemplateBuilder {
    /// Create new template builder
    pub fn new(name: impl Into<String>, template_type: TemplateType) -> Self {
        Self {
            name: name.into(),
            template_type,
            description: String::new(),
            version: "1.0.0".to_string(),
            variables: vec![],
            file_structure: FileStructure {
                directories: vec![],
                files: HashMap::new(),
                executables: HashMap::new(),
            },
            default_metadata: HashMap::new(),
            instructions_template: String::new(),
            example_usage: String::new(),
        }
    }

    /// Set description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set version
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Add variable
    pub fn variable(mut self, variable: TemplateVariable) -> Self {
        self.variables.push(variable);
        self
    }

    /// Add directory
    pub fn directory(mut self, dir: impl Into<String>) -> Self {
        self.file_structure.directories.push(dir.into());
        self
    }

    /// Add file
    pub fn file(mut self, path: impl Into<String>, content: impl Into<String>) -> Self {
        self.file_structure
            .files
            .insert(path.into(), content.into());
        self
    }

    /// Add executable
    pub fn executable(mut self, path: impl Into<String>, content: impl Into<String>) -> Self {
        self.file_structure
            .executables
            .insert(path.into(), content.into());
        self
    }

    /// Add default metadata
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.default_metadata.insert(key.into(), value.into());
        self
    }

    /// Set instructions template
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions_template = instructions.into();
        self
    }

    /// Set example usage
    pub fn example_usage(mut self, example: impl Into<String>) -> Self {
        self.example_usage = example.into();
        self
    }

    /// Build template
    pub fn build(self) -> SkillTemplate {
        SkillTemplate {
            name: self.name,
            template_type: self.template_type,
            description: self.description,
            version: self.version,
            variables: self.variables,
            file_structure: self.file_structure,
            default_metadata: self.default_metadata,
            instructions_template: self.instructions_template,
            example_usage: self.example_usage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_engine_creation() {
        let engine = TemplateEngine::new();
        assert!(!engine.templates.is_empty());
        assert!(engine.get_template("traditional").is_some());
        assert!(engine.get_template("cli-tool").is_some());
    }

    #[test]
    fn test_template_builder() {
        let template =
            SkillTemplateBuilder::new("test-template", TemplateType::Custom("test".to_string()))
                .description("Test template")
                .version("1.0.0")
                .directory("scripts")
                .file("README.md", "# Test README")
                .executable("test.sh", "#!/bin/bash\necho 'test'")
                .metadata("category", "test")
                .instructions("Test instructions")
                .example_usage("test example")
                .build();

        assert_eq!(template.name, "test-template");
        assert_eq!(template.description, "Test template");
        assert_eq!(template.file_structure.directories.len(), 1);
        assert_eq!(template.file_structure.files.len(), 1);
        assert_eq!(template.file_structure.executables.len(), 1);
    }

    #[test]
    fn test_variable_validation() {
        let engine = TemplateEngine::new();
        let template = engine.get_template("traditional").unwrap();

        let mut variables = HashMap::new();
        variables.insert("skill_name".to_string(), "test-skill".to_string());
        variables.insert("description".to_string(), "Test skill".to_string());

        assert!(engine.validate_variables(template, &variables).is_ok());

        // Test invalid skill name
        variables.insert("skill_name".to_string(), "Invalid Skill Name!".to_string());
        assert!(engine.validate_variables(template, &variables).is_err());
    }
}
