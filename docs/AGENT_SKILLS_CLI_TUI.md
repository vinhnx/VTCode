# Using Agent Skills in the CLI TUI

Complete guide on using Anthropic Agent Skills within VTCode's Terminal User Interface (TUI) chat environment.

## Quick Start

### 1. Start Interactive Chat

```bash
vtcode chat
```

You'll see the TUI chat interface with:
- **Input area** (bottom) - Type messages and slash commands
- **Transcript area** (top) - Shows conversation history
- **Status bar** - Shows model, provider, token usage

### 2. Access Skills Commands

Skills are available as slash commands in the TUI. Start typing a slash command:

```
Type: /sk
↓
Autocomplete suggestions appear:
  /skills - Manage skills and plugins
```

---

## Slash Commands in TUI

### List Available Skills

```
/skills list
```

**Output:**
```
Available Skills:
  • spreadsheet-generator - Generate professional Excel spreadsheets with data, charts, and formatting
  • doc-generator - Generate professional Word documents with formatted text, tables, and layouts
  • pdf-report-generator - Generate professional PDF documents with charts, styling, and complex layouts
  • strict-architecture - Enforces universal strict governance rules (500 lines, 5 funcs, 4 args)
  • bdd-workflow - BDD and TDD development workflow
  • code-orchestration - Orchestrated development with automatic task breakdown
  • forensic-debugging - CRASH-RCA forensic debugging for systematic bug investigation

Use `/skills info <name>` for details
```

### Show Skill Details

```
/skills info spreadsheet-generator
```

**Output:**
```
Skill: spreadsheet-generator
Description: Generate professional Excel spreadsheets with data, charts, and formatting
Version: 1.0.0
Author: VTCode Team

--- Instructions ---
When asked to generate a spreadsheet:
1. Understand Requirements...
[Full skill documentation displayed]

--- Available Resources ---
  • scripts/helper.py
```

### Load Skill into Session

```
/skills load spreadsheet-generator
```

**Effect:**
- Skill metadata added to system prompt (~100 tokens)
- Agent now knows about this skill and when to use it
- Full instructions loaded on-demand when needed

**Output:**
```
✓ Loaded skill: spreadsheet-generator (v1.0.0)
  Description: Generate professional Excel spreadsheets...
  Resources: 0 files

Skill is ready to use. Ask the agent to use it in your next message.
```

### Unload Skill

```
/skills unload spreadsheet-generator
```

**Effect:**
- Removes skill from current session
- Frees up context tokens
- Skill still available for other sessions

### Execute Skill with Input

```
/skills use spreadsheet-generator "Create financial dashboard"
```

**Effect:**
- Executes skill with specified input
- Generates immediate output (file, document, analysis)
- Results displayed in transcript

---

## Typical Workflow

### Workflow 1: Create Excel Spreadsheet

```
Step 1: Start chat
$ vtcode chat

Step 2: Load spreadsheet skill
/skills list
/skills info spreadsheet-generator
/skills load spreadsheet-generator

Step 3: Ask agent to use skill
User: Create an Excel spreadsheet with Q4 2024 financial data including:
      - Monthly revenue
      - Expense breakdown
      - Profit margins
      - Comparison with Q3

Step 4: Agent creates spreadsheet
Agent: I'll create the spreadsheet using the spreadsheet-generator skill...
       [Creates file]
       
Step 5: Download or use result
Result: File reference shown, ready for download
```

### Workflow 2: Generate Project Document

```
Step 1: Start chat
$ vtcode chat

Step 2: Load doc-generator skill
/skills load doc-generator

Step 3: Specify document requirements
User: Create a Word document for our Q1 2025 project proposal:
      - Executive summary (2 paragraphs)
      - Project scope and deliverables
      - Timeline (6 months)
      - Budget breakdown
      - Risk assessment

Step 4: Agent generates document
Agent: Creating project proposal document...
       [Creates formatted Word document]

Step 5: Use document
Result: File ready for download or sharing
```

### Workflow 3: Code Architecture Review

```
Step 1: Start chat
$ vtcode chat

Step 2: Load strict-architecture skill
/skills load strict-architecture

Step 3: Ask for review
User: Review src/main.rs for strict architecture rules:
      - 500 lines per file maximum
      - 5 functions per file maximum
      - 4 arguments per function

Step 4: Agent analyzes code
Agent: Analyzing against strict-architecture rules...
       [Identifies violations]

Step 5: Get recommendations
Result: Specific refactoring suggestions provided
```

---

## Keyboard Shortcuts in TUI

### Navigation

| Key | Action |
|-----|--------|
| `↑` / `↓` | Scroll transcript up/down |
| `PgUp` / `PgDn` | Scroll transcript by page |
| `Home` / `End` | Jump to top/bottom |
| `Tab` | Autocomplete slash command |
| `Ctrl+L` | Clear screen |

### Editing Input

| Key | Action |
|-----|--------|
| `Ctrl+A` | Jump to start of line |
| `Ctrl+E` | Jump to end of line |
| `Ctrl+K` | Clear to end of line |
| `Ctrl+U` | Clear entire line |
| `Ctrl+W` | Delete previous word |

### Submission

| Key | Action |
|-----|--------|
| `Enter` | Submit message |
| `Shift+Enter` | New line in multi-line input |
| `Ctrl+C` | Cancel/stop response |
| `Ctrl+D` | Exit chat (when input empty) |

---

## Skills Context in TUI

### Progressive Disclosure

When you load a skill:

**Immediately (100 tokens):**
- Skill metadata (name, description)
- Agent knows skill exists
- No context cost for unused skills

**On-demand (<5K tokens):**
- Full SKILL.md instructions
- Workflows and guidelines
- Loaded only when agent decides to use skill

**Never:**
- Skill resources (scripts, templates)
- Only executed when needed
- No context loading

### Using Multiple Skills

```
/skills load spreadsheet-generator
/skills load doc-generator
/skills load pdf-report-generator

User: Create a comprehensive quarterly report with:
      1. Excel spreadsheet with financial data
      2. Word document with analysis
      3. PDF summary report

Agent: I'll use all three skills to create your report...
```

---

## Examples

### Example 1: Financial Analysis

```
Command Sequence:

1. /skills load spreadsheet-generator
2. User: Create a sales analysis spreadsheet showing:
         - Monthly sales by region (4 regions)
         - Growth percentage
         - Year-over-year comparison
         - Summary dashboard

Agent Response:
Creating spreadsheet with regional sales data...
[Creates Excel file]
File created successfully. Ready for download.
```

### Example 2: Documentation Project

```
Command Sequence:

1. /skills load doc-generator
2. User: Generate API documentation for our REST service:
         - Overview and authentication
         - Endpoint specifications (list, get, post, update, delete)
         - Request/response examples
         - Error codes and meanings
         - Rate limiting details

Agent Response:
Generating comprehensive API documentation...
[Creates Word document]
Documentation created with all endpoints documented.
```

### Example 3: Code Quality Review

```
Command Sequence:

1. /skills load strict-architecture
2. User: Review our codebase for strict architecture compliance

Agent Response:
Analyzing code against strict-architecture rules:
- 500 lines per file maximum
- 5 functions per file maximum  
- 4 arguments per function

Findings:
✗ src/main.rs: 650 lines (exceeds 500)
✗ src/handlers.rs: 8 functions (exceeds 5)
✓ src/utils.rs: 4 functions, all ≤4 args

Recommendations:
1. Split src/main.rs into modules
2. Extract handlers into separate files
...
```

---

## Tips & Tricks

### 1. Quick Skill Discovery

```
/skills list | grep -E "generator|workflow"
```

Shows only document generation and workflow skills.

### 2. Skip Loading Details

Instead of:
```
/skills info spreadsheet-generator
/skills load spreadsheet-generator
```

You can directly:
```
/skills use spreadsheet-generator "Create financial data"
```

### 3. Multi-step Processing

```
/skills load spreadsheet-generator
User: Create spreadsheet step 1: [data]
Agent: [creates step 1]

User: Add charts and formatting
Agent: [updates spreadsheet]

/skills load pdf-report-generator
User: Convert to PDF report
Agent: [creates PDF version]
```

### 4. Check Loaded Skills

```
User: What skills do you have loaded?
Agent: Currently loaded:
  - spreadsheet-generator
  - doc-generator
  - strict-architecture
```

### 5. Unload to Save Context

```
/skills unload spreadsheet-generator
/skills load doc-generator
```

Frees up tokens for longer conversations.

---

## Common Patterns

### Pattern: Create and Download

```
/skills load doc-generator
User: Create a proposal document
Agent: Creating proposal...
       [Generates document]
       
User: Ready! Your document is prepared for download.
```

### Pattern: Iterate and Refine

```
/skills load spreadsheet-generator
User: Create financial dashboard
Agent: Creating spreadsheet...

User: Add a profit margin column
Agent: Updating spreadsheet...

User: Change colors to blue theme
Agent: Formatting spreadsheet...
```

### Pattern: Multi-skill Workflow

```
/skills load spreadsheet-generator
/skills load doc-generator
/skills load pdf-report-generator

User: Create a comprehensive quarterly report
Agent: Step 1: Creating data spreadsheet...
       Step 2: Creating analysis document...
       Step 3: Generating PDF summary...
       Done! All three files ready.
```

---

## Troubleshooting in TUI

### Skill Not Found

**Problem:**
```
/skills info my-skill
Error: Skill not found
```

**Solution:**
```
/skills list     # Check available skills
vtcode skills config  # Check search paths
```

### Slash Command Not Working

**Problem:**
```
/skills list doesn't work
```

**Solution:**
1. Type `/sk` and wait for autocomplete
2. Ensure you're typing `/` (slash) not other characters
3. Press `Tab` to autocomplete command
4. Check spelling

### File Not Generated

**Problem:**
Agent says it will create a file but nothing appears

**Solution:**
1. Check agent output for errors
2. Verify skill is properly loaded: `/skills list`
3. Ensure code execution is enabled
4. Check API key is valid

---

## Keyboard Shortcuts Reference

### Navigation
- `↑`/`↓` - Scroll transcript
- `PgUp`/`PgDn` - Page scroll
- `Tab` - Autocomplete

### Input Editing
- `Ctrl+A` - Start of line
- `Ctrl+E` - End of line
- `Ctrl+W` - Delete word

### Submission
- `Enter` - Send message
- `Shift+Enter` - New line
- `Ctrl+C` - Cancel

---

## Advanced Usage

### Batch Processing

```
/skills load spreadsheet-generator

User: I need 3 reports:
      1. Q4 2024 financial summary
      2. Sales by region analysis  
      3. Customer growth metrics

Agent: Creating all three spreadsheets...
       [Processes all three]
       Three files ready.
```

### Conditional Skill Loading

```
User: Can you create an Excel analysis?

Agent: Yes! Loading spreadsheet-generator...
/skills load spreadsheet-generator
[Automatically loads when needed]
```

### Error Recovery

```
User: Create a PDF report
Agent: Error: PDF skill not loaded

/skills load pdf-report-generator
User: Try again
Agent: [Successfully creates PDF]
```

---

## Session Management

### Preserve Skills Between Messages

Skills stay loaded for the session:

```
/skills load spreadsheet-generator
User: Create file 1
Agent: [creates]

User: Create file 2  # Skill still loaded!
Agent: [creates]
```

### Switch Skills

```
/skills unload spreadsheet-generator
/skills load doc-generator
User: Now create a document
Agent: [creates with doc-generator]
```

### New Session

```
/new              # Starts fresh session
[Skills not carried over]
/skills list      # All skills available again
```

---

## Support & Help

### In TUI

```
/help             # Show all slash commands
/skills           # Show skill commands help
```

### Documentation

- Quick Reference: `docs/AGENT_SKILLS_QUICKREF.md`
- Integration Guide: `docs/AGENT_SKILLS_INTEGRATION.md`
- Complete Skills Guide: `docs/SKILLS_GUIDE.md`

### Examples

```bash
python examples/skills_spreadsheet.py
python examples/skills_word_document.py
python examples/skills_pdf_generation.py
```

---

## Summary

In the TUI chat:

1. **Start:** `vtcode chat`
2. **List skills:** `/skills list`
3. **Learn about skill:** `/skills info <name>`
4. **Load skill:** `/skills load <name>`
5. **Use skill:** Ask agent in chat
6. **Unload:** `/skills unload <name>`

Skills use progressive disclosure - metadata loaded immediately, full instructions on-demand, resources never loaded.

---

**Last Updated:** December 13, 2024
