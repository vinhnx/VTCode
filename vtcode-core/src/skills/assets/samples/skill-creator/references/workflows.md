# Skill Workflows Reference

This reference provides workflow patterns for different types of skills. Use these as
templates when designing your skill's structure.

## Workflow Decision Tree Pattern

Best for: Skills with multiple modes of operation (read vs write, simple vs complex).

```
## Workflow Decision Tree

Start here to find the right workflow:

1. What operation are you performing?
   ├─ Reading/Analyzing → Go to [Reading Workflow](#reading-workflow)
   ├─ Creating new → Go to [Creation Workflow](#creation-workflow)
   └─ Modifying existing → Go to [Editing Workflow](#editing-workflow)

2. What's the complexity?
   ├─ Simple/Single item → Use quick commands
   └─ Complex/Batch → Use full workflow
```

### Example: Document Processing Skill

```markdown
## Workflow Decision Tree

1. What type of operation?
   ├─ Reading → [Reading Documents](#reading-documents)
   ├─ Creating → [Creating Documents](#creating-documents)
   └─ Editing → [Editing Documents](#editing-documents)

2. Single or batch?
   ├─ Single document → Direct commands
   └─ Multiple documents → [Batch Processing](#batch-processing)
```

## Sequential Steps Pattern

Best for: Skills with clear step-by-step processes.

```markdown
## Quick Start

1. **Prepare** - Gather requirements
2. **Initialize** - Set up workspace
3. **Execute** - Run main operation
4. **Verify** - Check results
5. **Finalize** - Clean up and save

## Detailed Steps

### Step 1: Prepare

[Detailed preparation instructions]

### Step 2: Initialize

[Detailed initialization instructions]

...
```

### Example: API Integration Skill

```markdown
## Integration Workflow

### Step 1: Authentication

1. Obtain API credentials
2. Configure environment variables
3. Test connection

### Step 2: Data Mapping

1. Identify source fields
2. Map to target schema
3. Handle transformations

### Step 3: Execution

1. Validate data
2. Execute API calls
3. Handle responses

### Step 4: Verification

1. Check response codes
2. Validate data integrity
3. Log results
```

## Task-Based Pattern

Best for: Skills that provide multiple independent operations.

```markdown
## Available Operations

### Operation A: [Name]

**When to use**: [Specific trigger/scenario]
**Command**: `script_name.py --operation a`

### Operation B: [Name]

**When to use**: [Specific trigger/scenario]
**Command**: `script_name.py --operation b`
```

### Example: File Conversion Skill

```markdown
## Conversions

### PDF to Text

**When to use**: Extract text content from PDFs
**Script**: `scripts/convert.py --from pdf --to txt`

### Images to PDF

**When to use**: Combine images into a PDF document
**Script**: `scripts/convert.py --from images --to pdf`

### Batch Convert

**When to use**: Convert multiple files at once
**Script**: `scripts/batch_convert.py --input-dir ./files --output-format docx`
```

## Conditional Logic Pattern

Best for: Skills with context-dependent behavior.

```markdown
## Handling Logic

### If [Condition A]

Do X, then Y

### If [Condition B]

Do Z instead

### Edge Cases

-   [Edge case 1]: Handle with [solution]
-   [Edge case 2]: Handle with [solution]
```

### Example: Error Handling Skill

```markdown
## Error Response Logic

### If API returns 4xx error

1. Parse error message
2. Check against known issues
3. Suggest corrective action
4. Retry if appropriate

### If API returns 5xx error

1. Log error details
2. Wait with exponential backoff
3. Retry up to 3 times
4. Escalate if still failing

### If timeout occurs

1. Check network connectivity
2. Reduce batch size
3. Retry with longer timeout
```

## Progressive Complexity Pattern

Best for: Skills that serve both beginners and experts.

```markdown
## Quick Start (Beginner)

[Simple, opinionated workflow]

## Standard Usage

[Common patterns with options]

## Advanced Usage

[Full control, all options]

## Expert Mode

[Low-level access, edge cases]
```

### Example: Deployment Skill

````markdown
## Deployment

### Quick Deploy (Beginner)

```bash
scripts/deploy.py --quick
```
````

Uses sensible defaults for standard deployments.

### Standard Deploy

```bash
scripts/deploy.py --env staging --notify
```

Specify environment and notification preferences.

### Advanced Deploy

```bash
scripts/deploy.py --env production --canary 10 --rollback-on-error --health-check-timeout 300
```

Full control over deployment parameters.

### Expert: Custom Pipeline

See `references/custom-pipeline.md` for building custom deployment pipelines.

````

## Combining Patterns

Most effective skills combine multiple patterns:

1. **Decision Tree** at the top to route users
2. **Sequential Steps** for complex workflows
3. **Task-Based** sections for independent operations
4. **Progressive Complexity** within each section

### Example Structure

```markdown
## Workflow Decision Tree
[Route to appropriate section]

## Quick Start
[Beginner-friendly path]

## Core Workflows
### Workflow A
[Sequential steps]

### Workflow B
[Sequential steps]

## Individual Tasks
### Task 1
[Task-based]

### Task 2
[Task-based]

## Advanced Topics
[Progressive complexity]
````
