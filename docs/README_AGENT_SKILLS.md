# VTCode Agent Skills - Master Guide

Complete reference for using Anthropic Agent Skills with VTCode in CLI and TUI.

## 🚀 Quick Start (2 minutes)

```bash
# 1. Start interactive chat
vtcode chat

# 2. List available skills
/skills list

# 3. Load a skill
/skills load spreadsheet-generator

# 4. Use it!
# Type: Create Excel spreadsheet with Q4 financial data

# 5. Done! Download your file
```

---

## 📚 Documentation Map

### For TUI Users (I'm using `vtcode chat`)

**Start here:**
- [`AGENT_SKILLS_QUICKREF.md`](AGENT_SKILLS_QUICKREF.md) - One-page quick reference (5 min)
- [`AGENT_SKILLS_CLI_TUI.md`](AGENT_SKILLS_CLI_TUI.md) - Complete TUI guide (15 min)
- [`AGENT_SKILLS_TUI_VISUAL.md`](AGENT_SKILLS_TUI_VISUAL.md) - Visual step-by-step guide

### For Integration (I'm building with skills)

**Start here:**
- [`AGENT_SKILLS_INTEGRATION.md`](AGENT_SKILLS_INTEGRATION.md) - Integration patterns (20 min)
- [`AGENT_SKILLS_SUMMARY.md`](AGENT_SKILLS_SUMMARY.md) - Implementation overview

### For Skill Creation (I want to build custom skills)

**Start here:**
- [`SKILLS_GUIDE.md`](SKILLS_GUIDE.md) - Complete skills guide (30 min)
- [`.claude/skills/README.md`](../.claude/skills/README.md) - Skills directory guide

### For Examples (Show me code!)

**Start here:**
- [`examples/skills_spreadsheet.py`](../examples/skills_spreadsheet.py) - Excel examples
- [`examples/skills_word_document.py`](../examples/skills_word_document.py) - Word examples
- [`examples/skills_pdf_generation.py`](../examples/skills_pdf_generation.py) - PDF examples

### For Navigation (Find specific info)

**Go here:**
- [`INDEX_AGENT_SKILLS.md`](INDEX_AGENT_SKILLS.md) - Complete navigation index

---

## 🎯 Choose Your Path

### "I want to use skills in the TUI now"

```
1. Read: AGENT_SKILLS_QUICKREF.md (5 min)
   ↓
2. Run: vtcode chat
   ↓
3. Type: /skills list
   ↓
4. Follow: AGENT_SKILLS_CLI_TUI.md for workflows
```

### "I want to understand how skills work"

```
1. Read: AGENT_SKILLS_INTEGRATION.md
   ↓
2. Review: Architecture section
   ↓
3. See: Code examples in AGENT_SKILLS_INTEGRATION.md
   ↓
4. Explore: SKILLS_GUIDE.md for advanced topics
```

### "I want to create custom skills"

```
1. Read: SKILLS_GUIDE.md → Skill Structure
   ↓
2. Run: vtcode skills create ~/.vtcode/skills/my-skill
   ↓
3. Edit: .claude/skills/README.md for reference
   ↓
4. Validate: vtcode skills validate ./my-skill
```

### "Show me working examples"

```
1. Run: python examples/skills_spreadsheet.py
   ↓
2. Review: docs/skills/SPREADSHEET_EXAMPLE.md
   ↓
3. Try: examples/skills_word_document.py
   ↓
4. Explore: examples/skills_pdf_generation.py
```

---

## 📋 Slash Commands Cheat Sheet

```bash
/skills list              # List all available skills
/skills info <name>       # Show skill documentation
/skills load <name>       # Load skill for session
/skills unload <name>     # Unload skill (free tokens)
/skills use <name> "..."  # Execute skill immediately
```

---

## 🎬 Common Workflows

### Workflow 1: Create Financial Spreadsheet

```bash
$ vtcode chat
/skills load spreadsheet-generator
# Type: Create Excel with Q4 revenue, expenses, profit margins
# Agent creates file ✓
```

**Time:** 2-3 minutes  
**Reference:** AGENT_SKILLS_CLI_TUI.md → Example 1

### Workflow 2: Generate Project Document

```bash
$ vtcode chat
/skills load doc-generator
# Type: Create proposal with scope, timeline, budget
# Agent creates file ✓
```

**Time:** 3-4 minutes  
**Reference:** AGENT_SKILLS_CLI_TUI.md → Example 2

### Workflow 3: Code Architecture Review

```bash
$ vtcode chat
/skills load strict-architecture
# Type: Review code for 500-line, 5-func, 4-arg rules
# Agent analyzes and provides recommendations ✓
```

**Time:** 2-3 minutes  
**Reference:** AGENT_SKILLS_CLI_TUI.md → Example 3

### Workflow 4: Create Comprehensive Report

```bash
$ vtcode chat
/skills load spreadsheet-generator
/skills load doc-generator
/skills load pdf-report-generator
# Type: Create quarterly report with all three files
# Agent creates all three ✓
```

**Time:** 5-10 minutes  
**Reference:** AGENT_SKILLS_TUI_VISUAL.md → Multi-Skill Workflow

---

## 🛠️ Available Skills

### Document Generation (Anthropic Agent Skills)

| Skill | Type | Use For | Command |
|-------|------|---------|---------|
| `spreadsheet-generator` | Excel/xlsx | Dashboards, data analysis, financial reports | `/skills load spreadsheet-generator` |
| `doc-generator` | Word/docx | Proposals, reports, technical docs | `/skills load doc-generator` |
| `pdf-report-generator` | PDF | Invoices, certificates, reports | `/skills load pdf-report-generator` |

### Development Skills

| Skill | Type | Use For | Command |
|-------|------|---------|---------|
| `strict-architecture` | Code Review | Architecture validation (500 lines, 5 funcs, 4 args) | `/skills load strict-architecture` |
| `bdd-workflow` | Process | TDD/BDD feature development | `/skills load bdd-workflow` |
| `code-orchestration` | Process | Orchestrated development | `/skills load code-orchestration` |
| `forensic-debugging` | Process | CRASH-RCA bug investigation | `/skills load forensic-debugging` |

---

## ⌨️ Keyboard Shortcuts

### Quick Access in TUI

| Key | Action |
|-----|--------|
| `/sk` + `Tab` | Autocomplete `/skills` command |
| `↑` / `↓` | Scroll transcript |
| `Enter` | Send message |
| `Shift+Enter` | New line |
| `Ctrl+C` | Cancel response |
| `Ctrl+L` | Clear screen |

**Full reference:** AGENT_SKILLS_CLI_TUI.md → Keyboard Shortcuts

---

## 🔄 Progressive Disclosure

How skills efficiently use context:

```
LOAD SKILL
/skills load spreadsheet-generator
    ↓
IMMEDIATELY (~100 tokens)
  • Metadata: name, description
  • Agent knows skill exists
  • No context cost when unused
    ↓
WHEN AGENT USES SKILL (<5K tokens)
  • Full instructions loaded
  • Workflows provided
  • Only used when needed
    ↓
RESOURCES (on-demand)
  • Scripts executed separately
  • Templates accessed as needed
  • Never loaded into context
```

**Result:** Skills are extremely context-efficient!

---

## 📖 Documentation Files

All documentation files located in `docs/`:

| File | Size | Purpose | Audience |
|------|------|---------|----------|
| `AGENT_SKILLS_QUICKREF.md` | 6.2K | One-page quick reference | Everyone |
| `AGENT_SKILLS_CLI_TUI.md` | 12K | Complete TUI guide | TUI users |
| `AGENT_SKILLS_TUI_VISUAL.md` | 24K | Visual step-by-step guide | Visual learners |
| `AGENT_SKILLS_INTEGRATION.md` | 12K | Integration patterns | Developers |
| `AGENT_SKILLS_SUMMARY.md` | 10K | Implementation overview | Architects |
| `SKILLS_GUIDE.md` | 16K | Complete skills guide | Skill creators |
| `INDEX_AGENT_SKILLS.md` | 11K | Navigation index | Researchers |
| `README_AGENT_SKILLS.md` | This file | Master guide | Everyone |

---

## 🚀 Getting Started Paths

### Path 1: I Just Want to Use Skills (5 minutes)
```
1. Read: AGENT_SKILLS_QUICKREF.md
2. Run: vtcode chat
3. Type: /skills load spreadsheet-generator
4. Create: Excel spreadsheet
5. Done!
```

### Path 2: I Want to Learn Properly (30 minutes)
```
1. Read: AGENT_SKILLS_QUICKREF.md
2. Read: AGENT_SKILLS_CLI_TUI.md
3. Watch: AGENT_SKILLS_TUI_VISUAL.md
4. Practice: Create spreadsheet, document, PDF
5. Master: Use multiple skills together
```

### Path 3: I Want to Integrate Skills (1 hour)
```
1. Understand: AGENT_SKILLS_SUMMARY.md
2. Learn: AGENT_SKILLS_INTEGRATION.md
3. Code: Review examples/skills_*.py
4. Implement: Use skills in your code
5. Deploy: Test in your application
```

### Path 4: I Want to Create Custom Skills (2 hours)
```
1. Overview: SKILLS_GUIDE.md
2. Structure: SKILLS_GUIDE.md → Skill Structure
3. Create: vtcode skills create ~/.vtcode/skills/my-skill
4. Define: Edit SKILL.md
5. Validate: vtcode skills validate ./my-skill
6. Test: Load in TUI and use
```

---

## 🎓 Learning Resources

### Official Resources
- [Anthropic Agent Skills Overview](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview)
- [Agent Skills Quickstart](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/quickstart)
- [Skills Cookbook](https://github.com/anthropics/claude-cookbooks/tree/main/skills)

### VTCode Resources
- `.claude/skills/README.md` - Skills directory overview
- `examples/` - Working Python examples
- `docs/skills/` - Specific skill documentation

---

## ✅ Verification

### Verify Skills Are Available

```bash
vtcode skills list
# Should show: spreadsheet-generator, doc-generator, etc.
```

### Verify TUI Integration

```bash
vtcode chat
/skills list
# Should show all available skills
```

### Verify Examples Work

```bash
export ANTHROPIC_API_KEY=sk-...
python examples/skills_spreadsheet.py
# Should create Excel file
```

---

## 🆘 Troubleshooting

### Skill Not Found
```bash
/skills list              # Check available
/skills config            # Check search paths
vtcode skills validate <path>  # Validate SKILL.md
```

### Slash Command Not Working
```bash
/help                     # See all commands
/skills                   # See skill commands
Tab key                   # Trigger autocomplete
```

### File Not Generated
```
1. Check: Agent output for errors
2. Verify: /skills list shows skill loaded
3. Enable: Code execution
4. Try: Simpler request
```

**Full guide:** AGENT_SKILLS_INTEGRATION.md → Troubleshooting

---

## 📊 Feature Summary

✅ **Anthropic Agent Skills Integration**
- Excel (xlsx) generation
- Word (docx) generation
- PDF generation
- PowerPoint (pptx) generation

✅ **Custom Skills Support**
- Create custom skills with SKILL.md
- Skill discovery and loading
- Progressive disclosure (efficient context)

✅ **CLI & TUI Integration**
- `/skills` slash commands
- Interactive skill loading
- Multi-skill workflows
- Keyboard shortcuts

✅ **Development Skills**
- Code architecture review (strict-architecture)
- BDD/TDD workflows
- Code orchestration
- Forensic debugging

---

## 🎯 Next Steps

**Choose one:**

1. **Quick Start** (5 min)
   → Read `AGENT_SKILLS_QUICKREF.md`
   → Run `vtcode chat`
   → Try `/skills load spreadsheet-generator`

2. **Deep Dive** (30 min)
   → Read `AGENT_SKILLS_CLI_TUI.md`
   → Study `AGENT_SKILLS_TUI_VISUAL.md`
   → Practice workflows

3. **Integration** (1 hour)
   → Read `AGENT_SKILLS_INTEGRATION.md`
   → Review `examples/skills_*.py`
   → Implement in your code

4. **Custom Skills** (2 hours)
   → Read `SKILLS_GUIDE.md`
   → Create custom skill with `vtcode skills create`
   → Validate and test

---

## 📞 Support

- **Quick Questions?** → `AGENT_SKILLS_QUICKREF.md`
- **How do I use in TUI?** → `AGENT_SKILLS_CLI_TUI.md`
- **How do I integrate?** → `AGENT_SKILLS_INTEGRATION.md`
- **How do I create skills?** → `SKILLS_GUIDE.md`
- **Need navigation?** → `INDEX_AGENT_SKILLS.md`

---

## 📦 What's Included

✅ 3 new Agent Skills in `.claude/skills/`
✅ 7 documentation files (89KB total)
✅ 3 working Python examples
✅ 3 example documentation files
✅ Complete CLI/TUI integration
✅ Progressive disclosure optimization
✅ Keyboard shortcuts and workflows
✅ Troubleshooting guides

---

## 🎉 You're Ready!

Everything is set up. Start with:

```bash
vtcode chat
/skills list
/skills load spreadsheet-generator
# Type: Create Excel spreadsheet...
```

**Enjoy!**

---

**Last Updated:** December 13, 2024  
**VTCode Agent Skills - Complete Implementation**
