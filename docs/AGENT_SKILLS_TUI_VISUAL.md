# VTCode TUI Skills - Visual Guide

Step-by-step visual guide for using Agent Skills in the Terminal User Interface.

## Screen Layout

```
┌──────────────────────────────────────────────────────────────────────────┐
│                         VTCode Chat Session                              │
├──────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  [Conversation Transcript Area - scrollable]                            │
│                                                                          │
│  User: /skills list                                                    │
│  Agent: Available Skills:                                              │
│    • spreadsheet-generator - Excel spreadsheets                        │
│    • doc-generator - Word documents                                    │
│    • pdf-report-generator - PDF files                                  │
│                                                                          │
│  User: /skills load spreadsheet-generator                             │
│  Agent: ✓ Loaded skill: spreadsheet-generator (v1.0.0)               │
│                                                                          │
├──────────────────────────────────────────────────────────────────────────┤
│ Input: Create Excel spreadsheet with Q4 financial data                  │
│ [Autocomplete available: Tab to complete, Shift+Enter for new line]   │
└──────────────────────────────────────────────────────────────────────────┘
                              [Model: claude...] [TUI Mode]
```

---

## Step 1: Start Chat Interface

```bash
$ vtcode chat
```

**You see:**
```
┌────────────────────────────────────────────────────────┐
│                VTCode Interactive Chat                 │
│                                                        │
│  Ready to chat!                                       │
│  Type /help for commands                             │
│                                                        │
├────────────────────────────────────────────────────────┤
│ Input: _                                              │
└────────────────────────────────────────────────────────┘
```

---

## Step 2: Access Skills Help

```
Type: /
↓ See autocomplete suggestions
↓ Type: skills
↓ See: /skills - Manage skills and plugins
↓ Press Tab to autocomplete
```

**Visual:**
```
Input: /sk█
         ↓
    Suggestions:
    • /status
    • /sessions
    • /skills  ← This one!
```

---

## Step 3: List Available Skills

```
Type: /skills list
Press: Enter
```

**Output:**
```
┌────────────────────────────────────────────────────────┐
│                                                        │
│  User: /skills list                                   │
│                                                        │
│  Agent: Available Skills:                             │
│                                                        │
│  • spreadsheet-generator                              │
│    Generate professional Excel spreadsheets with      │
│    data, charts, and formatting                       │
│                                                        │
│  • doc-generator                                      │
│    Generate professional Word documents with          │
│    formatted text, tables, and layouts                │
│                                                        │
│  • pdf-report-generator                               │
│    Generate professional PDF documents with           │
│    charts, styling, and complex layouts               │
│                                                        │
│  • strict-architecture                                │
│    Enforces universal strict governance rules         │
│                                                        │
│  Use '/skills info <name>' for details                │
│                                                        │
├────────────────────────────────────────────────────────┤
│ Input: _                                              │
└────────────────────────────────────────────────────────┘
```

---

## Step 4: Get Skill Details

```
Type: /skills info spreadsheet-generator
Press: Enter
```

**Output:**
```
┌────────────────────────────────────────────────────────┐
│                                                        │
│  User: /skills info spreadsheet-generator             │
│                                                        │
│  Agent: Skill: spreadsheet-generator                  │
│  Description: Generate professional Excel             │
│  Version: 1.0.0                                       │
│  Author: VTCode Team                                  │
│                                                        │
│  --- Instructions ---                                 │
│  When asked to generate a spreadsheet:                │
│                                                        │
│  1. Understand Requirements                           │
│     Parse request for content, structure, format      │
│                                                        │
│  2. Plan the Spreadsheet                              │
│     Sketch layout and column structure                │
│                                                        │
│  3. Create with Code Execution                        │
│     Use Anthropic xlsx Agent Skill                    │
│                                                        │
│  4. Extract File Reference                            │
│     Return file ID for download                       │
│                                                        │
│  --- Available Resources ---                          │
│  • scripts/helper.py                                  │
│                                                        │
├────────────────────────────────────────────────────────┤
│ Input: _                                              │
└────────────────────────────────────────────────────────┘
```

---

## Step 5: Load Skill

```
Type: /skills load spreadsheet-generator
Press: Enter
```

**Output:**
```
┌────────────────────────────────────────────────────────┐
│                                                        │
│  User: /skills load spreadsheet-generator             │
│                                                        │
│  Agent: ✓ Loaded skill: spreadsheet-generator (v1.0.0)│
│          Description: Generate professional Excel...  │
│          Resources: 0 files                           │
│                                                        │
│          Skill is ready to use. Use it in your next   │
│          message.                                     │
│                                                        │
├────────────────────────────────────────────────────────┤
│ Input: _                                              │
└────────────────────────────────────────────────────────┘
```

**What Happened:**
- Skill metadata added to system prompt (~100 tokens)
- Agent now recognizes skill
- Ready to use when you ask

---

## Step 6: Use the Skill

```
Type: Create Excel spreadsheet with Q4 2024 financial data:
      - Monthly revenue from Jan-Dec
      - Expense breakdown (salaries, ops, marketing)
      - Profit margins
      - Comparison with Q3 2024
Press: Enter
```

**Agent Response:**
```
┌────────────────────────────────────────────────────────┐
│                                                        │
│  User: Create Excel spreadsheet with Q4 2024          │
│  financial data...                                    │
│                                                        │
│  Agent: I'll create a comprehensive Excel spreadsheet │
│  with Q4 2024 financial data. Let me use the          │
│  spreadsheet-generator skill...                       │
│                                                        │
│  [Thinking...] ◐                                      │
│                                                        │
│  Creating spreadsheet with revenue data...            │
│  Adding expense breakdown...                          │
│  Calculating profit margins...                        │
│  Creating Q3 comparison...                            │
│                                                        │
│  ✓ Spreadsheet created successfully!                  │
│                                                        │
│  The Excel file contains:                             │
│  - Sheet 1: Monthly Financial Data                    │
│    • Jan-Dec 2024 revenue                             │
│    • Expense categories with percentages              │
│    • Net profit calculations                          │
│                                                        │
│  - Sheet 2: Quarterly Comparison                      │
│    • Q3 vs Q4 metrics                                 │
│    • Growth percentages                               │
│    • Performance summary                              │
│                                                        │
│  File ready for download. Download ID: [file-id]     │
│                                                        │
├────────────────────────────────────────────────────────┤
│ Input: _                                              │
└────────────────────────────────────────────────────────┘
```

---

## Complete Interaction Flow

### Scenario: Create Financial Dashboard

```
┌─────────────────────────────────────────────────────────┐
│ STEP 1: Start & Explore                                │
├─────────────────────────────────────────────────────────┤
│ $ vtcode chat                                           │
│ > /skills list                                          │
│   [See all available skills]                            │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ STEP 2: Learn About Skill                              │
├─────────────────────────────────────────────────────────┤
│ > /skills info spreadsheet-generator                    │
│   [Read full documentation]                             │
│   [Understand features and use cases]                   │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ STEP 3: Load Skill                                     │
├─────────────────────────────────────────────────────────┤
│ > /skills load spreadsheet-generator                    │
│   ✓ Loaded (metadata ~100 tokens)                       │
│   [Ready to use]                                        │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ STEP 4: Use Skill                                      │
├─────────────────────────────────────────────────────────┤
│ > Create financial dashboard with:                      │
│   - Revenue by quarter                                 │
│   - Expense breakdown                                  │
│   - Profit margins                                     │
│                                                         │
│   Agent Response:                                       │
│   [Loads full instructions ~5K tokens]                  │
│   [Creates spreadsheet via code execution]              │
│   ✓ File created                                        │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ STEP 5: Download or Continue                           │
├─────────────────────────────────────────────────────────┤
│ File ready for:                                         │
│ • Download                                             │
│ • Sharing                                              │
│ • Further editing in Excel                              │
│                                                         │
│ Or continue chat:                                       │
│ > Add pie charts for expenses                           │
│ > Change formatting to blue theme                       │
│ > Create summary sheet                                  │
└─────────────────────────────────────────────────────────┘
```

---

## Keyboard Shortcuts Quick Reference

### During Input

```
┌─────────────────────────────────────────────┐
│ EDITING                                     │
├─────────────────────────────────────────────┤
│ Ctrl+A    Jump to start of line            │
│ Ctrl+E    Jump to end of line              │
│ Ctrl+K    Clear to end                     │
│ Ctrl+U    Clear entire line                │
│ Ctrl+W    Delete previous word             │
├─────────────────────────────────────────────┤
│ SUBMISSION                                  │
├─────────────────────────────────────────────┤
│ Enter        Send message                  │
│ Shift+Enter  New line (multi-line input)   │
│ Ctrl+C       Cancel/interrupt              │
│ Ctrl+D       Exit chat (when empty)        │
└─────────────────────────────────────────────┘
```

### In Transcript

```
┌─────────────────────────────────────────────┐
│ NAVIGATION                                  │
├─────────────────────────────────────────────┤
│ ↑ / ↓           Scroll up/down             │
│ Page Up/Down    Page scroll                │
│ Home            Jump to top                │
│ End             Jump to bottom             │
│ Tab             Autocomplete command       │
│ Ctrl+L          Clear screen               │
└─────────────────────────────────────────────┘
```

---

## Common Commands Cheat Sheet

```
┌────────────────────────────────────────────────────┐
│ SKILL COMMANDS                                     │
├────────────────────────────────────────────────────┤
│ /skills list                                       │
│   → Show all available skills                      │
│                                                    │
│ /skills info <name>                                │
│   → Show detailed skill documentation              │
│                                                    │
│ /skills load <name>                                │
│   → Load skill for this session                    │
│                                                    │
│ /skills unload <name>                              │
│   → Unload skill (frees context)                   │
│                                                    │
│ /skills use <name> <input>                         │
│   → Execute skill with input immediately           │
├────────────────────────────────────────────────────┤
│ CONTEXT COMMANDS                                   │
├────────────────────────────────────────────────────┤
│ /clear                                             │
│   → Clear transcript & history                     │
│                                                    │
│ /new                                               │
│   → Start new session (skills reset)               │
│                                                    │
│ /help                                              │
│   → Show all slash commands                        │
└────────────────────────────────────────────────────┘
```

---

## Multi-Skill Workflow

### Example: Create Complete Report

```
Step 1: Load Multiple Skills
───────────────────────────────
/skills load spreadsheet-generator
/skills load doc-generator
/skills load pdf-report-generator

Step 2: Create Spreadsheet
───────────────────────────
> Create quarterly financial spreadsheet with:
  - Revenue by month
  - Expense breakdown
  - Profit calculations

Agent: [Creates Excel file]

Step 3: Create Document
───────────────────────
> Add analysis document explaining the data:
  - Executive summary
  - Key findings
  - Recommendations

Agent: [Creates Word document]

Step 4: Create PDF
──────────────────
> Generate PDF summary of key metrics

Agent: [Creates PDF file]

Step 5: Complete!
─────────────────
Result: Three files ready
  • Financial_Data.xlsx
  • Analysis.docx
  • Summary.pdf
```

---

## Tips for Smooth Experience

### 1. Use Tab Autocomplete
```
Type: /sk[TAB]
Result: /skills list
        ↓
        Select with arrow keys, Enter to confirm
```

### 2. View Transcript While Typing
```
Scroll up during input to see previous messages
Skill metadata displayed in real-time
```

### 3. Monitor Context Usage
```
Token usage shown in status bar:
[Token: 2,450/4,096]
       ↑           ↑
    Current    Maximum
```

### 4. Unload Unused Skills
```
/skills unload old-skill
→ Frees up context tokens
→ Allows longer conversations
```

### 5. Check Session Status
```
/status
→ Shows current model
→ Shows provider (OpenAI, Claude, etc.)
→ Shows skill status
```

---

## Troubleshooting Visual Guide

### Issue: Skill Not Found

```
Problem:
  /skills load my-skill
  Error: Skill not found

Solution:
  /skills list
  ↓
  Check available skills
  ↓
  /skills info correct-name
  ↓
  /skills load correct-name
```

### Issue: Command Not Recognized

```
Problem:
  /mylcommand
  Error: unknown command

Solution:
  /help
  ↓
  See all valid commands
  ↓
  /help | grep skills
  ↓
  Use correct command name
```

### Issue: File Not Appearing

```
Problem:
  Agent says: "Creating file..."
  But no result shown

Solution:
  1. Check agent output for errors
  2. /skills list → verify skill loaded
  3. Look for error messages
  4. Try again with simpler request
```

---

## Color Indicators in TUI

```
┌────────────────────────────────────┐
│ COLORS MEANING                     │
├────────────────────────────────────┤
│ 🟢 Green    Successful action      │
│ 🔵 Blue     Information/prompts    │
│ 🟡 Yellow   Warnings               │
│ 🔴 Red      Errors                 │
│ ⚪ White    Regular text           │
└────────────────────────────────────┘
```

---

## Session Persistence

### Within Session

```
/skills load spreadsheet-generator
↓
Skills stay loaded
↓
You can use multiple times
↓
Create multiple spreadsheets
↓
Skills still loaded
```

### Between Sessions

```
Session 1:
  /skills load spreadsheet-generator
  [Use skill]
  /exit

Session 2:
  /skills load spreadsheet-generator  ← Must reload!
  [Skills don't carry over]
```

---

## Summary

### Quick Workflow

```
1. Start TUI
   $ vtcode chat

2. List Skills
   /skills list

3. Load Skill
   /skills load spreadsheet-generator

4. Use Skill
   Type your request

5. Get Result
   File ready for use!
```

### Key Points

- Progressive disclosure keeps context efficient
- Skills metadata ~100 tokens, instructions loaded on-demand
- Multiple skills can work together
- Context saved per session
- Keyboard shortcuts speed up interaction

---

**Last Updated:** December 13, 2024
