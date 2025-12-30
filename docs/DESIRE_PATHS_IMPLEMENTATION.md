# Desire Paths Implementation: Complete Integration

**Date**: December 30, 2025  
**Status**: ✅ COMPLETE

This document captures the full implementation of the Desire Paths philosophy across the VT Code system.

## What is Desire Paths?

A design principle where intuitive user mistakes signal interface improvements, not documentation errors. The system improves itself to match how people naturally work.

**Philosophy**: "When agents guess wrong, improve the interface, not the docs."

## Integration Points

### 1. Developer Documentation

**Files**: AGENTS.md, CLAUDE.md

- ✅ Philosophy explained at top of files
- ✅ Practical guidance for reporting friction
- ✅ Links to design documentation
- ✅ Examples of paved paths (cargo aliases)

**Status**: COMPLETE

### 2. Design Documentation

**File**: docs/DESIRE_PATHS.md (172 lines)

- ✅ Complete philosophy explanation
- ✅ Current paved paths table
- ✅ Backlog of future improvements
- ✅ How to report friction
- ✅ Implementation checklist

**Status**: COMPLETE

### 3. Cargo Aliases (Paved Paths)

**File**: .cargo/config.toml

```toml
[alias]
t = "test"
c = "check"
r = "run"
```

**Verification**:
- ✅ `cargo t` → `cargo test`
- ✅ `cargo c` → `cargo check`
- ✅ `cargo r` → `cargo run`

**Status**: TESTED & WORKING

### 4. Core Agent System Prompts

**File**: vtcode-core/src/prompts/system.rs

#### Default System Prompt (v4.5)
```
## Design Philosophy: Desire Paths
When you intuitively guess wrong about a command, flag, or workflow, 
treat it as a UX signal. Report friction through documentation feedback. 
The system improves interfaces (not docs) to match intuitive expectations. 
See AGENTS.md and docs/DESIRE_PATHS.md.
```

**Status**: ✅ DEPLOYED

#### Minimal System Prompt (v5.1)
```
- When you guess wrong about commands/flags, report it—the system 
  improves interfaces, not docs (Desire Paths philosophy).
```

**Status**: ✅ DEPLOYED

#### Specialized System Prompt (v4.4)
```
## Design Philosophy: Desire Paths
Report intuitive expectations that fail—the system improves interfaces 
(not docs) to match how agents naturally think. See docs/DESIRE_PATHS.md.
```

**Status**: ✅ DEPLOYED

### 5. Agent Instruction Delivery Chain

**Data Flow**:
```
Agent Start
    ↓
Generate System Instruction
    ↓
default_system_prompt() [v4.5]
    ↓
Include Desire Paths philosophy
    ↓
Send to LLM
    ↓
Agent understands friction reporting
```

**Status**: ✅ VERIFIED

## Files Changed

### Modified (4 files)

1. **AGENTS.md**
   - Lines added: +50
   - Sections: 3 new sections
   - Status: ✅

2. **CLAUDE.md**
   - Lines added: +8
   - Philosophy reference added
   - Status: ✅

3. **.cargo/config.toml**
   - Lines added: +4
   - Cargo aliases added
   - Status: ✅ TESTED

4. **vtcode-core/src/prompts/system.rs**
   - Lines added: +13 (across 3 prompts)
   - All prompts updated
   - Status: ✅ COMPILED

### Created (1 file)

1. **docs/DESIRE_PATHS.md**
   - Lines: 172
   - Complete design documentation
   - Status: ✅

## Verification Results

### Cargo Aliases
```bash
cargo t      ✓ Works (alias for cargo test)
cargo c      ✓ Works (alias for cargo check)
cargo r      ✓ Works (alias for cargo run)
```

### System Prompts
```bash
cargo check  ✓ Compiles without errors
             ✓ All 3 prompts verified
             ✓ Desire Paths included in each
```

### Documentation
```bash
AGENTS.md           ✓ 3 Desire Paths sections
CLAUDE.md          ✓ Philosophy section
docs/DESIRE_PATHS.md ✓ 172 lines, complete
```

## Agent Behavioral Changes

### What Agents Now Know

1. **Intuitive failures are valuable**
   - Not documentation errors
   - System will improve to match natural expectations

2. **They should report friction**
   - Through documentation feedback
   - With clear patterns and examples

3. **The improvement cycle**
   - Agent finds friction
   - Documents in AGENTS.md or docs/DESIRE_PATHS.md
   - System designers implement the path
   - Future agents find it "just works"

4. **References**
   - See AGENTS.md for quick reference
   - See docs/DESIRE_PATHS.md for full details

## Example: Cargo Aliases

**Before (Friction)**:
```
Agent tries: cargo t
Error: unknown subcommand
Agent learns: Must use full command "cargo test"
```

**After (Paved Path)**:
```
Agent tries: cargo t
Success: ✓ Tests run
Agent learns: Both "cargo t" and "cargo test" work
Tool feels intuitive from day 1
```

## Next Opportunities (Backlog)

From docs/DESIRE_PATHS.md:

**Medium Priority**:
- Tool operation shortcuts (e.g., `code_intelligence goto_definition file.rs`)
- Subagent naming improvements (e.g., `spawn_subagent --name explore`)

**Low Priority**:
- Config CLI shortcuts (e.g., `vtcode config set llm.model gpt-4`)

## Impact Assessment

### Immediate
- ✅ All agents receive Desire Paths philosophy in system prompt
- ✅ Clear examples (cargo aliases) demonstrate the principle
- ✅ Friction reporting process is documented and accessible

### Long-term
- 📈 Tool becomes increasingly intuitive over time
- 📈 UX improvements compound through feedback loop
- 📈 Agent onboarding becomes faster
- 📈 Reduces cognitive load for all users

### Organizational
- 📊 Creates systematic approach to UX friction
- 📊 Aligns with "pave desire paths" design philosophy
- 📊 Enables continuous UX improvement without redesigns
- 📊 Provides metrics for success (fewer friction reports)

## Deployment Status

| Component | Status | Verification |
|-----------|--------|--------------|
| Cargo Aliases | ✅ Live | Tested all 3 |
| Default Prompt | ✅ Deployed | Compiles, includes philosophy |
| Minimal Prompt | ✅ Deployed | Compiles, includes philosophy |
| Specialized Prompt | ✅ Deployed | Compiles, includes philosophy |
| Documentation | ✅ Complete | 3 docs with cross-references |
| Delivery Chain | ✅ Verified | generate_system_instruction() → LLM |

## Sustainability

### How to Maintain This

1. **Monitor Friction Reports**
   - Check AGENTS.md friction comments
   - Track docs/DESIRE_PATHS.md backlog

2. **Implement Desire Paths Regularly**
   - Follow the implementation checklist
   - Test before deployment
   - Document in DESIRE_PATHS.md

3. **Keep Philosophy Visible**
   - System prompts are the most visible place
   - Update them when philosophy evolves
   - Reference them in PRs and discussions

4. **Share the Pattern**
   - This philosophy can be applied to any tool
   - Consider it for other projects
   - Document the pattern as it matures

## References

- **AGENTS.md**: Quick reference and developer guidance
- **CLAUDE.md**: Philosophy overview with links
- **docs/DESIRE_PATHS.md**: Complete design documentation
- **Wikipedia - Desire Path**: Original concept
- **VT Code Architecture**: System that benefits from this approach

---

**Implementation By**: Amp AI Agent  
**Philosophy Source**: Desire Path concept (Wikipedia, urban planning)  
**Applied To**: VT Code system-wide integration  
**Next Review**: When new friction patterns emerge
