# Step 6: MCP Integration and Testing

Building on the 5 completed implementation steps, this guide covers end-to-end integration testing and validation of the MCP code execution architecture.

## Overview

The 5 implementation steps form an integrated system:

```
Agent
  ↓
[Tool Discovery] → search_tools() finds relevant operations
  ↓
[Code Execution] → execute_code() runs agent-written Python/JS
  ↓
[SDK Generation] → MCP tools available as library functions
  ↓
[Data Filtering] → Filter/aggregate results in code sandbox
  ↓
[Skill Persistence] → save_skill() stores reusable patterns
  ↓
[PII Protection] → tokenizer detects & redacts sensitive data
```

## Test Strategy

### 1. Unit Tests (Module Level)

Each component has focused unit tests:

```bash
# Tool Discovery
cargo test -p vtcode-core tool_discovery

# Code Executor
cargo test -p vtcode-core code_executor

# Skill Manager
cargo test -p vtcode-core skill_manager

# PII Tokenizer
cargo test -p vtcode-core pii_tokenizer

# Tool Registry (builtins)
cargo test -p vtcode-core registry
```

### 2. Integration Tests (Cross-Module)

#### Test 1: Discovery → Execution → Filtering

**Objective**: Verify tool discovery feeds into code execution with SDK generation.

```python
# Test: discover tools, then use them in code
def test_discovery_to_execution():
    # 1. Agent discovers file tools
    tools = search_tools(keyword="file", detail_level="name-only")
    assert "list_files" in tools
    assert "read_file" in tools
    
    # 2. Agent writes code using discovered tools
    code = '''
    files = list_files(path="/workspace", recursive=True)
    test_files = [f for f in files if "test" in f]
    result = {"count": len(test_files), "files": test_files[:5]}
    '''
    
    # 3. Execute code (SDK auto-generated from available tools)
    output = execute_code(code=code, language="python3")
    
    # 4. Verify filtering occurred in sandbox
    assert "count" in output["result"]
    assert output["exit_code"] == 0
```

#### Test 2: Execution → Skill Persistence → Reuse

**Objective**: Verify code execution produces skills that can be saved and reused.

```python
def test_execution_to_skill_reuse():
    # 1. Agent executes and tests code
    code = '''
    def filter_by_ext(path, ext):
        files = list_files(path=path, recursive=True)
        return [f for f in files if f.endswith(ext)]
    result = {"test": filter_by_ext("/workspace", ".rs")}
    '''
    
    output = execute_code(code=code, language="python3")
    assert output["exit_code"] == 0
    
    # 2. Save as skill
    save_skill(
        name="filter_by_extension",
        code=code,
        language="python3",
        description="Filter files by extension",
        inputs=[
            {"name": "path", "type": "str", "required": True},
            {"name": "ext", "type": "str", "required": True}
        ],
        output="List of matching files",
        tags=["file-operations", "filtering"]
    )
    
    # 3. Later: load and use skill
    skill = load_skill("filter_by_extension")
    assert skill.name == "filter_by_extension"
    assert skill.language == "python3"
    
    # 4. Reuse skill in new code
    new_code = skill.code + "\nresult = filter_by_ext('/src', '.py')"
    output = execute_code(code=new_code, language="python3")
    assert output["exit_code"] == 0
```

#### Test 3: PII Protection in Full Pipeline

**Objective**: Verify PII tokenization protects sensitive data end-to-end.

```python
def test_pii_protection_pipeline():
    # 1. Agent writes code with user data
    code = '''
    user_data = {
        "email": "john@example.com",
        "ssn": "123-45-6789",
        "api_key": "sk-abc123xyz789"
    }
    # Code operates on data
    emails = [user_data["email"]]
    result = {"processed": True, "count": len(emails)}
    '''
    
    # 2. Execute with PII protection enabled
    output = execute_code(
        code=code,
        language="python3",
        enable_pii_protection=True
    )
    
    # 3. Verify result doesn't contain sensitive data
    assert output["exit_code"] == 0
    assert "123-45-6789" not in output["stdout"]
    assert "sk-abc123xyz789" not in output["stderr"]
    assert "__PII_ssn_" in output["stdout"] or output["stdout"] == ""
```

#### Test 4: Large Data Filtering

**Objective**: Verify agent can process 10k+ results in code without context explosion.

```python
def test_large_dataset_filtering():
    # 1. Simulate large file listing
    code = '''
    files = list_files(path="/workspace", recursive=True)
    
    # Process 10k+ results in code
    rust_files = [f for f in files if f.endswith(".rs")]
    src_files = [f for f in rust_files if "/src/" in f]
    test_files = [f for f in src_files if "test" in f.lower()]
    
    # Aggregate statistics
    stats = {
        "total_files": len(files),
        "rust_files": len(rust_files),
        "src_files": len(src_files),
        "test_files": len(test_files),
        "sample": test_files[:10]
    }
    result = stats
    '''
    
    output = execute_code(code=code, language="python3")
    
    # Verify: model only sees aggregated stats, not all files
    assert "sample" in output["result"]
    assert len(output["result"]["sample"]) <= 10
```

#### Test 5: Tool Error Handling in Code

**Objective**: Verify proper error handling when tools fail during code execution.

```python
def test_tool_error_handling():
    code = '''
    try:
        # This should fail (nonexistent path)
        files = list_files(path="/nonexistent/path/xyz")
        result = {"error": False}
    except Exception as e:
        result = {"error": True, "message": str(e)}
    '''
    
    output = execute_code(code=code, language="python3")
    assert output["exit_code"] == 0  # Code executed
    assert output["result"]["error"] == True  # Tool raised exception
```

### 3. Performance Tests

#### Test 6: Discovery Performance

```bash
# Verify progressive discovery reduces context usage
time cargo test -p vtcode-core tool_discovery::tests::test_discovery_context_efficiency
```

**Success Criteria**:
- Exact match discovery: < 50ms
- Fuzzy match discovery: < 200ms
- Results limited to top 10 matches (context saving)

#### Test 7: Code Execution Performance

```bash
# Python execution speed
time cargo test -p vtcode-core code_executor::tests::test_python_execution_speed

# JavaScript execution speed
time cargo test -p vtcode-core code_executor::tests::test_javascript_execution_speed
```

**Success Criteria**:
- Cold start (first execution): < 2s
- Warm start: < 500ms
- Timeout enforcement: ✅

#### Test 8: SDK Generation Speed

```bash
# Verify SDK generation doesn't block code execution
time cargo test -p vtcode-core code_executor::tests::test_sdk_generation_overhead
```

**Success Criteria**:
- SDK generation: < 100ms
- IPC handler creation: < 50ms

### 4. Scenario Tests (End-to-End)

#### Scenario 1: Code Analysis Pipeline

Agent needs to find all TODO comments across codebase:

```python
def test_scenario_find_todos():
    code = '''
    # 1. Discover grep tool
    # (in real usage: search_tools(keyword="grep"))
    
    # 2. Use grep to find TODOs
    matches = grep_file(pattern="TODO", path="src")
    
    # 3. Filter and aggregate in code
    todos_by_file = {}
    for match in matches.get("matches", []):
        file = match["file"]
        if file not in todos_by_file:
            todos_by_file[file] = []
        todos_by_file[file].append(match["line"])
    
    # 4. Return only summary (not all matches)
    result = {
        "files_with_todos": len(todos_by_file),
        "total_todos": sum(len(lines) for lines in todos_by_file.values()),
        "sample_files": list(todos_by_file.keys())[:5]
    }
    '''
    
    output = execute_code(code=code, language="python3")
    assert output["exit_code"] == 0
    assert "files_with_todos" in output["result"]
    assert "sample_files" in output["result"]
```

#### Scenario 2: Data Export with PII Redaction

Agent exports user data while protecting PII:

```python
def test_scenario_export_with_pii():
    code = '''
    # Read potentially sensitive files
    user_data = read_file(path="/workspace/.env")
    
    # Tokenize PII before logging/exporting
    safe_version = tokenize_pii(user_data)
    
    # Work with tokenized data
    lines = safe_version.split("\\n")
    env_vars = [l for l in lines if "=" in l]
    
    result = {
        "total_env_vars": len(env_vars),
        "sample": [l.split("=")[0] for l in env_vars[:5]]
    }
    '''
    
    output = execute_code(
        code=code,
        language="python3",
        enable_pii_protection=True
    )
    
    assert output["exit_code"] == 0
    # Verify no plaintext secrets in output
    assert "api_key=" not in output["stdout"]
```

#### Scenario 3: Skill Library Development

Agent builds and reuses a skill library:

```python
def test_scenario_skill_library():
    # 1. Develop first skill: file filtering
    filter_skill = '''
    def find_files(path, pattern):
        files = list_files(path=path, recursive=True)
        return [f for f in files if pattern in f]
    '''
    
    save_skill(
        name="find_files",
        code=filter_skill,
        language="python3",
        tags=["files", "core"]
    )
    
    # 2. Develop second skill using first
    analyze_skill = filter_skill + '''
    def analyze_rs_files(path):
        rs_files = find_files(path, ".rs")
        return {
            "count": len(rs_files),
            "dirs": len(set(f.rsplit("/", 1)[0] for f in rs_files))
        }
    '''
    
    save_skill(
        name="analyze_rs_files",
        code=analyze_skill,
        language="python3",
        tags=["rust", "analysis"]
    )
    
    # 3. List and search skills
    skills = list_skills()
    assert any(s.name == "find_files" for s in skills)
    
    rs_skills = search_skills(tag="rust")
    assert any(s.name == "analyze_rs_files" for s in rs_skills)
    
    # 4. Use skill in workflow
    skill = load_skill("analyze_rs_files")
    output = execute_code(
        code=skill.code + "\nresult = analyze_rs_files('/src')",
        language="python3"
    )
    assert "count" in output["result"]
```

## Test Execution

Run all integration tests:

```bash
# Full integration suite
cargo test -p vtcode-core exec --lib -- --test-threads=1

# With output
cargo test -p vtcode-core exec --lib -- --test-threads=1 --nocapture

# Specific scenario
cargo test -p vtcode-core code_executor::tests::test_scenario_find_todos -- --nocapture
```

## Validation Checklist

Before marking Step 6 complete:

- [ ] All 5 step unit tests pass
- [ ] Integration tests (1-5) pass
- [ ] Performance tests meet criteria
- [ ] Scenario tests (1-3) pass
- [ ] No regressions in existing functionality
- [ ] Documentation updated with test results
- [ ] Code coverage > 80% for exec module
- [ ] Memory usage stable (no leaks)
- [ ] PII protection verified with real patterns
- [ ] Skill library format stable and versioned

## Known Limitations & Future Work

### Step 7: Observability & Metrics (Future)

Metrics to track:
- Tool discovery hit rate (does agent find what it needs?)
- Code execution success rate by language
- Average token savings per operation (actual vs full disclosure)
- Skill reuse ratio (saved skills / new code written)
- PII redaction effectiveness

### Step 8: Tool Versioning (Future)

- Tool schema versioning with compatibility checks
- SDK generation caching for stability
- Skill compatibility validation against current tools

### Step 9: Agent Behavior Optimization (Future)

- Learn which tools agents use most → prioritize in discovery
- Analyze code execution patterns → pre-generate SDKs
- Track skill effectiveness → recommend combinations
- Guide agent toward better data filtering strategies

## References

- Anthropic MCP Blog: [Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)
- vtcode MCP Implementation: `vtcode-core/src/exec/`, `vtcode-core/src/mcp/`
- Test Examples: `vtcode-core/src/exec/*/tests.rs`
