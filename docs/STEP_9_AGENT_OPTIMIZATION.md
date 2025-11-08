# Step 9: Agent Behavior Optimization

The final step in the MCP code execution architecture: using data from Steps 1-8 to automatically improve agent behavior and guide better decision-making.

## Overview

**Goal**: Build a learning system that observes agent behavior and provides real-time guidance to make agents more effective.

```
Metrics (Step 7) + Versioning (Step 8) → Learning System
                                            ↓
                    ┌──────────────────────┴──────────────────────┐
                    ↓                                              ↓
            Agent Suggestions                          Proactive Warnings
    (tool recommendations, skills)              (version incompatibility, errors)
```

## Architecture

```
Historical Data (Sessions 1-N)
    ├── Tool usage patterns
    ├── Execution success/failure rates
    ├── Code patterns (what works?)
    ├── Skill adoption rates
    └── Migration effectiveness

        ↓

Agent Behavior Analyzer
    ├── discover_usage_patterns()
    ├── predict_failures()
    ├── recommend_optimizations()
    └── learn_effective_patterns()

        ↓

Real-time Decision Engine
    ├── On tool discovery: recommend most-used tools first
    ├── On code execution: warn about high-failure patterns
    ├── On skill creation: suggest similar existing skills
    └── On tool update: predict migration difficulty
```

## Components

### 1. Agent Behavior Analyzer

```rust
pub struct AgentBehaviorAnalyzer {
    metrics_history: Vec<MetricsSummary>,  // Sessions 1-N
    skill_stats: SkillStatistics,
    tool_stats: ToolStatistics,
    failure_patterns: FailurePatterns,
}

pub struct SkillStatistics {
    pub creation_to_reuse_time: Duration,     // How long before skill is reused?
    pub avg_lifecycle: Duration,              // Average skill lifetime
    pub reuse_ratio_by_tag: HashMap<String, f64>,
    pub most_effective_skills: Vec<String>,
    pub rarely_used_skills: Vec<String>,
}

pub struct ToolStatistics {
    pub discovery_success_rate: f64,          // Does agent find it?
    pub usage_frequency: HashMap<String, u64>,
    pub typical_discovery_queries: Vec<String>,
    pub common_tool_chains: Vec<Vec<String>>, // Tools often used together
}

pub struct FailurePatterns {
    pub high_failure_tools: Vec<(String, f64)>,      // tool, failure_rate
    pub high_timeout_patterns: Vec<CodePattern>,
    pub common_error_messages: Vec<(String, u64)>,   // error, count
    pub recovery_patterns: Vec<RecoveryPattern>,
}

pub struct CodePattern {
    pub language: String,
    pub pattern: String,  // regex or AST pattern
    pub failure_rate: f64,
    pub example_failures: Vec<String>,
}

pub struct RecoveryPattern {
    pub error_type: String,
    pub recovery_action: String,     // "retry with timeout increase", etc.
    pub success_rate: f64,
}
```

### 2. Real-Time Agent Guidance System

```rust
pub struct AgentGuidanceSystem {
    analyzer: AgentBehaviorAnalyzer,
    metrics: Arc<MetricsCollector>,
}

impl AgentGuidanceSystem {
    /// Before tool discovery, recommend order
    pub fn rank_tools_for_discovery(&self, keyword: &str) -> Vec<ToolRecommendation> {
        let tools = self.analyzer.tool_stats.usage_frequency;
        
        tools
            .iter()
            .filter(|(name, _)| is_relevant(name, keyword))
            .map(|(name, frequency)| {
                let success_rate = self.analyzer.tool_stats
                    .discovery_success_rate_for_tool(name);
                
                ToolRecommendation {
                    name: name.clone(),
                    priority: frequency * success_rate,
                    reason: format!("Used {} times with {:.0}% success", 
                                  frequency, success_rate * 100.0),
                }
            })
            .sorted_by(|a, b| b.priority.partial_cmp(&a.priority).unwrap())
            .collect()
    }

    /// Before code execution, warn about risky patterns
    pub fn analyze_code_risk(&self, code: &str, language: &str) -> ExecutionRiskAssessment {
        let mut risk = ExecutionRiskAssessment {
            severity: RiskSeverity::Low,
            warnings: vec![],
            suggestions: vec![],
        };

        // Check for high-failure patterns
        for pattern in &self.analyzer.failure_patterns.high_timeout_patterns {
            if pattern.language == language && self.matches_pattern(code, &pattern.pattern) {
                risk.severity = RiskSeverity::High;
                risk.warnings.push(format!(
                    "Pattern '{}' has {:.0}% failure rate",
                    pattern.pattern, pattern.failure_rate * 100.0
                ));
                risk.suggestions.push(
                    "Consider adding timeout override or breaking into smaller operations".into()
                );
            }
        }

        // Check for missing tools
        let required_tools = self.extract_tool_calls(code)?;
        for tool in &required_tools {
            if !self.is_available_in_sandbox(tool) {
                risk.severity = RiskSeverity::High;
                risk.warnings.push(format!("Tool '{}' not available in sandbox", tool));
            }
        }

        // Suggest existing skills that solve similar problems
        if let Some(similar_skills) = self.find_similar_skills(code) {
            risk.suggestions.push(format!(
                "Consider using existing skill: {}",
                similar_skills.first().unwrap()
            ));
        }

        risk
    }

    /// Suggest skills before agent creates new ones
    pub fn suggest_existing_skills(&self, task_description: &str) -> Vec<SkillSuggestion> {
        let skills = &self.analyzer.skill_stats;
        
        // Find skills that solve similar problems
        skills
            .most_effective_skills
            .iter()
            .filter_map(|skill_name| {
                let similarity = self.semantic_similarity(task_description, skill_name)?;
                if similarity > 0.7 {
                    Some(SkillSuggestion {
                        skill_name: skill_name.clone(),
                        similarity_score: similarity,
                        usage_count: skills.usage_count(skill_name)?,
                        last_used: skills.last_used(skill_name)?,
                    })
                } else {
                    None
                }
            })
            .sorted_by(|a, b| b.similarity_score.partial_cmp(&a.similarity_score).unwrap())
            .collect()
    }

    /// Predict tool update impacts
    pub fn predict_migration_difficulty(
        &self,
        tool_name: &str,
        from_version: &str,
        to_version: &str,
    ) -> MigrationDifficulty {
        // Count affected skills
        let affected_skills = self.find_skills_using(tool_name, from_version);
        
        // Check for breaking changes
        let breaking_changes = self.get_breaking_changes(tool_name, from_version, to_version);
        
        // Check success rate of similar migrations
        let historical_success_rate = self
            .analyzer
            .failure_patterns
            .migration_success_rate_for_tool(tool_name);

        MigrationDifficulty {
            affected_skill_count: affected_skills.len(),
            breaking_change_count: breaking_changes.len(),
            estimated_success_rate: historical_success_rate,
            estimated_migration_time_ms: affected_skills.len() as u64 * 500,  // 500ms per skill
            recommendations: self.generate_migration_recommendations(breaking_changes),
        }
    }
}

#[derive(Serialize)]
pub struct ToolRecommendation {
    pub name: String,
    pub priority: f64,
    pub reason: String,
}

#[derive(Serialize)]
pub struct ExecutionRiskAssessment {
    pub severity: RiskSeverity,
    pub warnings: Vec<String>,
    pub suggestions: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Serialize)]
pub struct SkillSuggestion {
    pub skill_name: String,
    pub similarity_score: f64,
    pub usage_count: u64,
    pub last_used: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct MigrationDifficulty {
    pub affected_skill_count: usize,
    pub breaking_change_count: usize,
    pub estimated_success_rate: f64,
    pub estimated_migration_time_ms: u64,
    pub recommendations: Vec<String>,
}
```

## Learning Strategies

### 1. Tool Usage Pattern Learning

```python
# Data: Historical tool usage across sessions
# Goal: Predict which tools agent needs before it searches

def analyze_tool_chains():
    """Learn which tools are used together"""
    # Session 1: grep_file → list_files → read_file
    # Session 2: list_files → read_file
    # Session 3: grep_file → list_files → read_file → write_file
    
    # Inferred chains:
    chains = {
        "grep_file → list_files → read_file": 0.80,  # confidence
        "list_files → read_file": 0.95,
        "... → write_file": 0.40,
    }
    
    return chains

# Use chains to proactively recommend tools:
# If agent uses grep_file, pre-load list_files and read_file
```

### 2. Failure Pattern Recognition

```python
def learn_failure_patterns():
    """Identify patterns in execution failures"""
    
    patterns = {
        # Pattern → failure rate
        r"list_files.*recursive=True.*path=/workspace": 0.35,  # 35% failure
        r"for .* in .* code:.*execute_code\(": 0.50,  # Loop with nested code execution
        r"grep_file.*regex.*special_chars": 0.45,  # Regex escaping issues
    }
    
    return patterns

# Use patterns to warn agent:
# "Warning: This pattern has 35% failure rate. Consider breaking into smaller chunks"
```

### 3. Skill Effectiveness Learning

```python
def evaluate_skill_effectiveness():
    """Learn which skills are worth keeping"""
    
    for skill_name, metrics in skill_metrics.items():
        effectiveness_score = (
            metrics.reuse_count * 0.4 +           # Is it reused?
            metrics.success_rate * 0.4 +          # Does it work?
            (metrics.time_to_reuse_days < 7) * 0.2  # Was it reused quickly?
        )
        
        if effectiveness_score < 0.3:
            recommend_deletion(skill_name)
        elif effectiveness_score > 0.8:
            mark_as_exemplary(skill_name)
```

### 4. Code Pattern Optimization

```python
def learn_optimal_execution_patterns():
    """Learn which code structures work best"""
    
    successful_patterns = {
        "small_chunks": {
            "avg_lines": 10,
            "success_rate": 0.98,
            "avg_duration_ms": 300,
        },
        "with_error_handling": {
            "success_rate": 0.95,
            "recovery_time_ms": 200,
        },
        "streaming_results": {
            "success_rate": 0.99,
            "memory_usage_mb": 20,
        },
    }
    
    return successful_patterns
```

## Real-Time Integration

### Hook: Before Tool Discovery

```rust
// In Tool Discovery
pub fn search_tools(&self, keyword: &str, agent_guidance: &AgentGuidanceSystem) 
    -> Result<SearchResults> {
    // Get recommended order from learning system
    let tool_rankings = agent_guidance.rank_tools_for_discovery(keyword);
    
    // Return results in recommended order
    let results = self.perform_search(keyword)?;
    
    // Reorder results
    let ordered = self.reorder_by_recommendations(results, tool_rankings);
    
    Ok(ordered)
}
```

### Hook: Before Code Execution

```rust
// In Code Executor
pub async fn execute(&self, params: &ExecutionParams, guidance: &AgentGuidanceSystem) 
    -> Result<ExecutionOutput> {
    // Assess risk before executing
    let risk = guidance.analyze_code_risk(&params.code, &params.language)?;
    
    if risk.severity == RiskSeverity::Critical {
        return Err(anyhow::anyhow!("Code execution blocked: {}", risk.warnings[0]));
    }
    
    if !risk.warnings.is_empty() {
        log_warnings(&risk.warnings);
        // Could inject warnings into agent context
    }
    
    // Execute with optimized parameters based on risk level
    let timeout = match risk.severity {
        RiskSeverity::Low => 5000,
        RiskSeverity::Medium => 3000,
        RiskSeverity::High => 1000,
        RiskSeverity::Critical => 0,  // Blocked above
    };
    
    self.execute_internal(params, timeout).await
}
```

### Hook: Before Skill Creation

```rust
// In Skill Manager
pub fn save_skill(&self, skill: &Skill, guidance: &AgentGuidanceSystem) 
    -> Result<()> {
    // Check if similar skill already exists
    if let Some(suggestions) = guidance.suggest_existing_skills(&skill.description) {
        log_info!("Consider using existing skill: {:?}", suggestions);
        // Could ask user for confirmation
    }
    
    self.save_internal(skill)?;
    Ok(())
}
```

## Metrics for Learning

Track optimization effectiveness:

```rust
pub struct OptimizationMetrics {
    pub tool_recommendation_accuracy: f64,  // Agent finds tool in top N?
    pub risk_assessment_accuracy: f64,      // Warnings were accurate?
    pub skill_suggestion_adoption: f64,     // Agent used suggestions?
    pub total_guidance_provided: u64,
    pub agent_performance_improvement: f64, // % reduction in failures
    pub token_savings_from_optimization: u64,
}
```

## Usage Examples

### Example 1: Agent Gets Tool Recommendations

```
Agent: "Find all test files"
System: Analyzing context...
Guidance: "Based on history, agents usually chain:
  1. list_files (99% discovery success)
  2. read_file (95% when exploring)
  Consider loading these first"
Agent: [searches] Found list_files and read_file! ✓
```

### Example 2: Agent Gets Risk Warning

```
Agent writes code:
  for file in files:
      result = execute_code(f"cat {file}")
  
System warning: "⚠️ Nested code execution has 50% timeout rate
  Suggestion: Batch operations or increase timeout to 5000ms"
  
Agent: [updates code] Now succeeds! ✓
```

### Example 3: Agent Gets Skill Suggestion

```
Agent: "Save this pattern for filtering Rust files"
System: "📌 Similar skill exists: 'find_rs_files' (used 12 times, 95% success)
  See example: skill load_skill('find_rs_files')"

Agent: [loads existing skill instead] Saves time ✓
```

## Implementation Phases

### Phase 1: Baseline (Immediate)
- Collect metrics without changing agent behavior
- Build analyzer to process historical data
- Generate reports of usage patterns

### Phase 2: Passive Guidance (Week 1)
- Log recommendations without forcing them
- Display in agent context/UI
- Measure adoption rates

### Phase 3: Active Guidance (Week 2)
- Reorder tool discovery results
- Inject warnings into execution
- Suggest skills before creation

### Phase 4: Predictive Optimization (Week 3)
- Predict failures and prevent them
- Auto-adjust timeouts based on patterns
- Learn new optimal patterns

## Benefits

1. **Faster Agent Success**: Tools and skills discovered in optimal order
2. **Fewer Failures**: Risk warnings prevent bad code patterns
3. **Better Skill Usage**: Agents reuse existing skills instead of recreating
4. **Adaptive Behavior**: System learns and improves over time
5. **Data-Driven Decisions**: All recommendations backed by historical data
6. **Cost Reduction**: Fewer failed executions = fewer retries = lower token usage

## Testing

```bash
# Test behavior analyzer
cargo test -p vtcode-core behavior_analyzer --lib

# Test guidance system
cargo test -p vtcode-core agent_guidance --lib

# Test learning strategies
cargo test -p vtcode-core learning_strategies --lib

# Integration test with real metrics
cargo test -p vtcode-core optimization_integration --lib
```

## Roadmap After Step 9

### Future Enhancements

**Step 10: Predictive Resource Allocation**
- Pre-allocate resources based on pattern predictions
- Reduce timeout failures by 50%

**Step 11: Multi-Agent Learning**
- Share learned patterns across agents
- Accelerate learning curve for new agents

**Step 12: Custom Agent Personalities**
- Let each agent specialize in different domains
- Optimize tool choices per domain

## References

- Machine Learning Observability: https://ml-ops.systems/
- Pattern Recognition in Software: https://www.se-radio.net/
- Agent Optimization: https://arxiv.org/abs/2305.15778
- Feedback Loops in Systems: https://donellameadows.org/
