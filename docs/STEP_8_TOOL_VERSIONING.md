# Step 8: Tool Versioning & Compatibility

After building observability (Step 7), the next critical capability is **tool versioning**. As tools evolve, skills written against old tool signatures become incompatible. This step implements versioning and migration strategies.

## Problem

**Scenario**: A skill was created using `list_files(path, recursive)`. The tool is updated to `list_files(path, recursive, max_depth)`. The skill breaks silently.

```python
# Old skill (now broken)
files = list_files(path="/src", recursive=True)  # Missing required max_depth?
```

**Solution**: Implement semantic versioning with compatibility checking and automatic migration.

## Architecture

```
Tool Registry
├── Tool Definition (vX.Y.Z)
│   ├── input_schema
│   ├── output_schema
│   ├── breaking_changes: [] (if any)
│   └── deprecations: [] (if any)
│
└── Skill Compatibility
    ├── declares: tool@vX.Y (skill was written for)
    ├── check_compatibility()
    ├── get_migration_path()
    └── apply_migration()
```

## Tool Versioning

### Tool Version Structure

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct ToolVersion {
    pub name: String,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub released: DateTime<Utc>,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    pub breaking_changes: Vec<BreakingChange>,
    pub deprecations: Vec<Deprecation>,
    pub migration_guide: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BreakingChange {
    pub field: String,
    pub old_type: String,
    pub new_type: String,
    pub reason: String,
    pub migration_code: String,  // How to update code
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Deprecation {
    pub field: String,
    pub replacement: Option<String>,
    pub removed_in: String,  // e.g., "v2.0.0"
    pub guidance: String,
}
```

### Tool Versioning Examples

#### Example 1: Breaking Change (Input Parameter)

```json
{
  "name": "list_files",
  "version": "2.0.0",
  "breaking_changes": [
    {
      "field": "recursive",
      "old_type": "boolean",
      "new_type": "enum(NONE|DIRECT|RECURSIVE)",
      "reason": "Support variable recursion depth",
      "migration_code": "recursive=True → depth=RECURSIVE, recursive=False → depth=NONE"
    }
  ]
}
```

**Migration**:
```python
# Old skill code
files = list_files(path="/src", recursive=True)

# Auto-migrated to
files = list_files(path="/src", depth="RECURSIVE")
```

#### Example 2: Deprecated Parameter

```json
{
  "name": "grep_file",
  "version": "1.5.0",
  "deprecations": [
    {
      "field": "case_sensitive",
      "replacement": "regex_flags",
      "removed_in": "v2.0.0",
      "guidance": "Use regex_flags='i' for case-insensitive instead"
    }
  ]
}
```

**Tool Registry tracks**:
```json
{
  "read_file": {
    "current_version": "1.2.3",
    "versions": {
      "1.0.0": {...},
      "1.1.0": {...},
      "1.2.0": {...},
      "1.2.3": {...}
    },
    "deprecated_versions": ["1.0.0", "1.1.0"],  // No longer used
    "supported_versions": ["1.2.0", "1.2.3"]    // Still supported
  }
}
```

## Skill Compatibility Management

### Skill Versioning Metadata

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub version: String,
    pub language: String,
    pub tool_dependencies: Vec<ToolDependency>,
    pub created_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub compatible_with: Vec<String>,  // e.g., ["read_file@1.x", "write_file@1.x"]
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ToolDependency {
    pub name: String,
    pub version: String,  // e.g., "1.2" (accepts 1.2.x)
    pub usage: Vec<String>,  // Which functions use this tool?
}
```

### Compatibility Checking

```rust
pub struct SkillCompatibilityChecker {
    skill: Skill,
    tool_registry: ToolRegistry,
    metrics: Arc<MetricsCollector>,
}

impl SkillCompatibilityChecker {
    /// Check if skill works with current tools
    pub fn check_compatibility(&self) -> CompatibilityReport {
        let mut report = CompatibilityReport {
            compatible: true,
            warnings: vec![],
            errors: vec![],
            migrations: vec![],
        };

        for dep in &self.skill.metadata.tool_dependencies {
            let tool = self.tool_registry.get(&dep.name)?;
            
            match self.check_version_compatibility(&dep.version, &tool.version) {
                VersionCompatibility::Compatible => {
                    // All good
                }
                VersionCompatibility::Warning(msg) => {
                    report.compatible = true;
                    report.warnings.push(msg);
                    self.metrics.record_compatibility_warning(&self.skill.name);
                }
                VersionCompatibility::RequiresMigration => {
                    report.compatible = false;
                    report.migrations.push(self.generate_migration(&dep)?);
                    self.metrics.record_compatibility_migration(&self.skill.name);
                }
                VersionCompatibility::Incompatible(msg) => {
                    report.compatible = false;
                    report.errors.push(msg);
                    self.metrics.record_compatibility_error(&self.skill.name);
                }
            }
        }

        report
    }

    /// Generate migration code to make skill compatible
    pub fn generate_migration(&self, dep: &ToolDependency) -> Result<Migration> {
        // Parse skill code with tree-sitter
        let tree = tree_sitter::parse(&self.skill.code)?;
        
        // Find all calls to this tool
        let call_sites = self.find_tool_calls(&tree, &dep.name)?;
        
        // Generate transformations
        let mut transformations = vec![];
        for call_site in call_sites {
            if let Some(migration) = self.get_breaking_change_migration(dep, call_site)? {
                transformations.push(migration);
            }
        }
        
        Ok(Migration {
            skill_name: self.skill.name.clone(),
            tool: dep.name.clone(),
            from_version: dep.version.clone(),
            to_version: self.tool_registry.get(&dep.name)?.version.clone(),
            transformations,
        })
    }

    /// Check semantic versioning compatibility
    fn check_version_compatibility(&self, required: &str, available: &str) -> VersionCompatibility {
        // required = "1.2" (accepts 1.2.x)
        // available = "1.2.3" (current version)
        
        let (req_major, req_minor) = parse_version_range(required)?;
        let (avail_major, avail_minor, _) = parse_version(available)?;
        
        match (req_major == avail_major, req_minor == avail_minor) {
            (true, true) => VersionCompatibility::Compatible,
            (true, false) if avail_minor > req_minor => VersionCompatibility::Warning("Tool version higher than required".into()),
            _ => VersionCompatibility::RequiresMigration,
        }
    }
}

pub enum VersionCompatibility {
    Compatible,
    Warning(String),
    RequiresMigration,
    Incompatible(String),
}

#[derive(Serialize, Deserialize)]
pub struct CompatibilityReport {
    pub compatible: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub migrations: Vec<Migration>,
}

#[derive(Serialize, Deserialize)]
pub struct Migration {
    pub skill_name: String,
    pub tool: String,
    pub from_version: String,
    pub to_version: String,
    pub transformations: Vec<CodeTransformation>,
}

#[derive(Serialize, Deserialize)]
pub struct CodeTransformation {
    pub line_number: usize,
    pub old_code: String,
    pub new_code: String,
    pub reason: String,
}
```

## Tool Registry Integration

### Tool Definition with Version

```rust
#[derive(Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub version: ToolVersion,  // NEW: track versions
    pub input_schema: JsonSchema,
    pub output_schema: JsonSchema,
    pub previous_versions: Vec<ToolVersion>,  // Historical
    pub compatibility_layer: Option<CompatibilityLayer>,  // For backwards compat
}

/// Maps old signatures to new ones transparently
pub struct CompatibilityLayer {
    pub migrations: Vec<VersionMigration>,
}

pub struct VersionMigration {
    pub from_version: String,
    pub transformation: Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value>>,
}
```

### Tool Registry Methods

```rust
impl ToolRegistry {
    /// Register a tool with version
    pub fn register_versioned_tool(
        &mut self,
        definition: ToolDefinition,
    ) -> Result<()> {
        // Store all versions
        // Validate breaking changes
        // Record in metrics
        Ok(())
    }

    /// Get tool with version compatibility check
    pub fn get_compatible_tool(
        &self,
        name: &str,
        required_version: &str,
    ) -> Result<ToolDefinition> {
        let tool = self.get(name)?;
        
        if self.is_compatible(&tool.version, required_version) {
            Ok(tool)
        } else {
            Err(anyhow::anyhow!(
                "Tool {} version {} not compatible with requirement {}",
                name, tool.version, required_version
            ))
        }
    }

    /// Get migration path between versions
    pub fn get_migration_path(
        &self,
        name: &str,
        from_version: &str,
        to_version: &str,
    ) -> Result<VersionMigrationPath> {
        // Generate step-by-step migrations
        // Check for breaking changes
        // Return migration instructions
        Ok(VersionMigrationPath {
            steps: vec![],
        })
    }
}
```

## Automatic Migration Execution

### Skill Executor with Migration

```rust
pub struct SkillExecutorWithMigration {
    skill: Skill,
    executor: SkillExecutor,
    checker: SkillCompatibilityChecker,
    code_transformer: CodeTransformer,
}

impl SkillExecutorWithMigration {
    pub async fn execute(&self) -> Result<SkillOutput> {
        // 1. Check compatibility
        let compat_report = self.checker.check_compatibility();
        
        if compat_report.compatible {
            // Direct execution
            return self.executor.execute().await;
        }

        // 2. Apply migrations
        let migrated_code = self.apply_migrations(&compat_report.migrations)?;
        
        // 3. Execute migrated skill
        let mut migrated_skill = self.skill.clone();
        migrated_skill.code = migrated_code;
        
        self.executor.with_skill(migrated_skill).execute().await
    }

    fn apply_migrations(&self, migrations: &[Migration]) -> Result<String> {
        let mut code = self.skill.code.clone();
        
        for migration in migrations {
            for transformation in &migration.transformations {
                code = self.code_transformer.apply_transformation(
                    &code,
                    transformation,
                )?;
            }
        }
        
        Ok(code)
    }
}
```

## Version Registry Format

Store version history in `.vtcode/tools/versions/`:

```
.vtcode/tools/versions/
├── read_file/
│   ├── 1.0.0.json
│   ├── 1.1.0.json
│   ├── 1.2.0.json
│   └── 1.2.3.json (current)
├── write_file/
│   └── 1.1.0.json
└── INDEX.json (index of all versions)
```

### VERSION INDEX.json

```json
{
  "last_updated": "2024-11-08T10:30:45Z",
  "tools": {
    "read_file": {
      "current": "1.2.3",
      "supported": ["1.2.0", "1.2.3"],
      "deprecated": ["1.0.0", "1.1.0"],
      "breaking_changes": {
        "1.2.0": ["added required parameter: encoding"]
      }
    },
    "list_files": {
      "current": "2.0.0",
      "supported": ["2.0.0"],
      "deprecated": ["1.0.0", "1.5.0"],
      "breaking_changes": {
        "2.0.0": ["recursive: boolean → depth: enum"]
      }
    }
  }
}
```

## Metrics Integration

Track versioning effectiveness:

```rust
pub struct VersioningMetrics {
    pub total_compatibility_checks: u64,
    pub compatible_skills: u64,
    pub incompatible_skills: u64,
    pub migrations_executed: u64,
    pub migration_success_rate: f64,
    pub average_migration_time_ms: u64,
}
```

## Usage Workflows

### Workflow 1: Skill Creation (with Versions)

```python
# Agent creates skill
code = '''
def find_rs_files(path):
    files = list_files(path=path, recursive=True)
    return [f for f in files if f.endswith(".rs")]
'''

save_skill(
    name="find_rs_files",
    code=code,
    language="python3",
    tool_dependencies=[
        {
            "name": "list_files",
            "version": "1.2.3",  # Pin to specific version
            "usage": ["find_rs_files"]
        }
    ]
)
```

### Workflow 2: Tool Update

```python
# Tool is updated to 2.0.0
register_versioned_tool(
    name="list_files",
    version="2.0.0",
    breaking_changes=[
        {
            "field": "recursive",
            "old_type": "boolean",
            "new_type": "enum(NONE|DIRECT|RECURSIVE)",
            "migration": "recursive=True → depth=RECURSIVE"
        }
    ]
)

# Metrics recorded:
# - Tool version changed from 1.2.3 to 2.0.0
# - N skills need migration
# - Compatibility warnings generated
```

### Workflow 3: Skill Migration

```python
# Agent loads skill that needs migration
skill = load_skill("find_rs_files")

# Check compatibility
checker = SkillCompatibilityChecker(skill, registry)
report = checker.check_compatibility()

if not report.compatible:
    # Auto-migrate or ask user
    migrated_skill = apply_migrations(skill, report.migrations)
    
    # Save migration metadata
    migrated_skill.migrations = report.migrations
    save_skill(migrated_skill)
    
    # Metrics
    metrics.record_skill_migrated("find_rs_files", "list_files", "1.2.3->2.0.0")

# Execute
result = executor.execute(migrated_skill)
```

## Testing

```bash
# Test versioning logic
cargo test -p vtcode-core versioning --lib

# Test compatibility checking
cargo test -p vtcode-core compatibility --lib

# Test migration generation
cargo test -p vtcode-core migration --lib

# Test version registry
cargo test -p vtcode-core version_registry --lib
```

## Benefits

1. **Safe Tool Evolution**: Break old APIs with confidence knowing skills will migrate automatically
2. **Explicit Dependencies**: Skills declare which tool versions they need
3. **Historical Tracking**: Every version change is recorded and auditable
4. **Automatic Repair**: Incompatible skills are auto-migrated when possible
5. **Clear Migration Paths**: Developers understand what changed and why
6. **Observability**: Metrics track version adoption and migration success

## Next Step: Step 9

With versioning in place, we can implement **Agent Behavior Optimization**:
- Learn which tools agents actually use
- Predict execution failures and guide agents toward success
- Optimize code execution patterns based on historical data
- Build a learning system that improves agent performance over time
