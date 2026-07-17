# Output Patterns Reference

This reference provides patterns for structuring skill outputs. Use these patterns
to make your skills produce consistent, useful results.

## Core Principles

1. **Predictable Structure** - Users should know what to expect
2. **Actionable Content** - Output should be directly usable
3. **Context-Appropriate** - Match output to the task type
4. **Progressive Detail** - Start with summary, allow drill-down

## Pattern: Status Report

Best for: Operations with clear success/failure states.

```
[STATUS] Operation completed successfully

Summary:
- Processed: 42 files
- Succeeded: 40
- Skipped: 2 (already up to date)
- Failed: 0

Details:
- Input: /path/to/source
- Output: /path/to/destination
- Duration: 3.2s
```

### Variations

**Minimal (for scripts):**

```
OK: 42 files processed
```

**Verbose (for debugging):**

```
[2024-01-15 10:30:45] Starting operation...
[2024-01-15 10:30:45] Processing file 1/42: example.txt
[2024-01-15 10:30:46] Processing file 2/42: data.json
...
[2024-01-15 10:30:48] Completed: 42/42 files
[2024-01-15 10:30:48] Total duration: 3.2s
```

## Pattern: Generated Content

Best for: Code generation, document creation.

```markdown
## Generated: [Artifact Name]

### Preview

[First 10-20 lines or summary]

### Location

`/path/to/generated/file.ext`

### Next Steps

1. Review the generated content
2. Make any necessary adjustments
3. [Context-specific action]
```

### Code Generation Example

````markdown
## Generated: API Client

### Preview

```python
class APIClient:
    def __init__(self, base_url: str, api_key: str):
        self.base_url = base_url
        self.api_key = api_key

    def get(self, endpoint: str) -> dict:
        ...
```
````

### Location

`src/api_client.py`

### Next Steps

1. Add error handling for your specific use cases
2. Configure authentication in `.env`
3. Run tests with `pytest tests/test_api_client.py`

````

## Pattern: Analysis Results

Best for: Data analysis, code review, audits.

```markdown
## Analysis: [Subject]

### Summary
[1-2 sentence overview]

### Key Findings
1. **[Finding 1]**: [Brief description]
2. **[Finding 2]**: [Brief description]
3. **[Finding 3]**: [Brief description]

### Recommendations
- [Actionable recommendation 1]
- [Actionable recommendation 2]

### Details
[Expandable/optional detailed analysis]
````

### Code Review Example

```markdown
## Analysis: Authentication Module

### Summary

The authentication module has good structure but needs security improvements.

### Key Findings

1. **Password hashing**: Using outdated MD5 algorithm
2. **Session management**: No expiration set on tokens
3. **Input validation**: Missing sanitization on username field

### Recommendations

-   Upgrade to bcrypt or Argon2 for password hashing
-   Add 24-hour expiration to JWT tokens
-   Add input validation using the sanitize_input utility

### Details

See `references/security-audit.md` for full analysis.
```

## Pattern: Interactive Choice

Best for: Situations requiring user decision.

```markdown
## Decision Required: [Topic]

### Context

[Brief explanation of why decision is needed]

### Options

**Option A: [Name]**

-   Pros: [Benefits]
-   Cons: [Drawbacks]
-   Best for: [Use case]

**Option B: [Name]**

-   Pros: [Benefits]
-   Cons: [Drawbacks]
-   Best for: [Use case]

### Recommendation

[Your suggested option with rationale]

### To proceed

Reply with "A" or "B", or ask for more details.
```

## Pattern: Tutorial/Walkthrough

Best for: Teaching or guiding through processes.

````markdown
## Tutorial: [Topic]

### Prerequisites

-   [Requirement 1]
-   [Requirement 2]

### Step 1: [Action]

[Explanation]

```command
example command
```
````

You should see:

```
expected output
```

### Step 2: [Action]

[Explanation]

...

### Verification

[How to confirm success]

### Troubleshooting

-   **Issue**: [Common problem]
    **Solution**: [Fix]

````

## Pattern: Error Report

Best for: When operations fail or partially succeed.

```markdown
## Error: [Brief Description]

### What Happened
[Clear explanation of the failure]

### Cause
[Why it happened, if known]

### Impact
[What was affected]

### Resolution
1. [Step to fix]
2. [Step to verify]

### Prevention
[How to avoid in future, if applicable]
````

### Example

```markdown
## Error: Database Connection Failed

### What Happened

Unable to connect to PostgreSQL database at `db.example.com:5432`.

### Cause

Connection timeout after 30 seconds. The database server appears unreachable.

### Impact

-   Cannot execute queries
-   Data sync operation aborted
-   No data loss (read-only operation)

### Resolution

1. Check database server status: `pg_isready -h db.example.com`
2. Verify network connectivity: `ping db.example.com`
3. Check firewall rules for port 5432
4. Retry operation once connection is restored

### Prevention

Consider adding connection retry logic with exponential backoff.
```

## Pattern: Diff/Change Report

Best for: Modifications to existing content.

````markdown
## Changes: [File/Resource Name]

### Summary

[Number] changes made to [resource].

### Modifications

**Line 42**: Updated import statement

```diff
- from old_module import function
+ from new_module import function
```
````

**Lines 56-60**: Added error handling

```diff
+ try:
+     result = process_data(input)
+ except ProcessingError as e:
+     logger.error(f"Processing failed: {e}")
+     raise
```

### Before/After Comparison

[Optional full comparison for major changes]

````

## Combining Patterns

Complex outputs often combine multiple patterns:

```markdown
## Operation Complete: Data Migration

### Status
[Status Report Pattern]
- Migrated: 1,234 records
- Skipped: 56 (duplicates)
- Failed: 2

### Changes Made
[Diff Pattern]
- Added `migrated_at` timestamp to all records
- Converted `status` field from string to enum

### Errors Encountered
[Error Pattern]
2 records failed validation:
- Record #789: Invalid email format
- Record #1011: Missing required field

### Next Steps
[Interactive Choice Pattern]
How would you like to handle the failed records?
A) Skip and continue
B) Fix manually and retry
C) Roll back entire migration
````
