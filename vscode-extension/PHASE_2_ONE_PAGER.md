# Phase 2 One-Pager - Quick Reference

**Print This Out • Keep It Handy • Share With Team**

---

## The Goal

Transform VSCode extension from monolithic → modular architecture  
**Timeline**: 4 weeks | **Impact**: Foundation for Phase 3-4 | **Risk**: Low

---

## What Gets Built

```
WEEK 3: Commands        WEEK 4: Participants     WEEK 5: State Mgmt
├─ ask                  ├─ @workspace            ├─ Message types
├─ askSelection         ├─ @code                 ├─ Message store
├─ analyze              ├─ @terminal             └─ Conversation mgr
├─ updatePlan           ├─ @git
├─ config               
├─ trust                WEEK 6: Tests + Docs
└─ refresh              ├─ Unit tests (>90%)
                        ├─ Integration tests
                        └─ Architecture docs
```

---

## Success Criteria

| Metric | Before | After |
|--------|--------|-------|
| extension.ts | 500+ lines | <200 lines |
| Commands | Hardcoded | 7 modular classes |
| Test coverage | ~60% | >90% |
| Extensibility | Low | High |
| Participants | None | 4 built-in |

---

## Timeline

```
Week 3 (Nov 11-15): ▓▓▓▓░ Command system
Week 4 (Nov 18-22): ░▓▓▓▓░ Participant system  
Week 5 (Nov 25-29): ░░▓▓░ State management
Week 6 (Dec 02-06): ░░░▓▓ Testing + Docs
                    ─────────────────
                    25 dev days total
```

---

## For Each Role

### 👤 Decision Maker
1. Read: PHASE_2_SUMMARY.md (5 min)
2. Action: Approve resources
3. When: November 11

### 👨‍💻 Developer
1. Read: PHASE_2_QUICK_START.md (15 min)
2. Get assigned task from GitHub
3. Code using patterns shown in Quick Start
4. Create PR when done

### 🏗️ Tech Lead
1. Read: PHASE_2_IMPLEMENTATION_PLAN.md (30 min)
2. Run: PHASE_2_LAUNCH.md checklist
3. Create: GitHub issues from templates
4. Lead: Weekly standups

### 📊 Project Manager
1. Create: GitHub issues and milestones
2. Track: Weekly progress
3. Report: Status to stakeholders
4. Remove: Blockers

---

## Files You'll Need

| File | Purpose | Who |
|------|---------|-----|
| PHASE_2_SUMMARY.md | Overview | Everyone |
| PHASE_2_LAUNCH.md | Kickoff | Tech Lead |
| PHASE_2_QUICK_START.md | Dev Guide | Developers |
| PHASE_2_IMPLEMENTATION_PLAN.md | Specs | Tech Lead |
| PHASE_2_GITHUB_ISSUES.md | Issues | Tech Lead |
| IMPROVEMENT_ROADMAP_INDEX.md | Navigation | Everyone |

---

## Code Pattern (Same for All)

```typescript
// 1. Implement interface
class MyCommand implements ICommand {
  readonly id = 'vtcode.mycommand'
  readonly title = 'My Command'
  
  async execute(context) {
    // Do work
  }
  
  canExecute(context) {
    return true
  }
}

// 2. Register in registry
registry.register(new MyCommand())

// 3. Write tests
it('should execute', async () => {
  await cmd.execute(context)
  // Verify
})
```

---

## Weekly Goals

**Week 3**: All 7 commands extracted  
**Week 4**: 4 participants + @ mentions working  
**Week 5**: Message state + ConversationManager done  
**Week 6**: All tests passing + documentation complete  

---

## Red Flags (Stop & Ask Tech Lead)

🚩 Don't know what to do next  
🚩 Breaking existing functionality  
🚩 Can't write tests  
🚩 Code doesn't match patterns  
🚩 Running behind schedule  
🚩 Confused about requirements  

**→ Ask immediately. Don't guess.**

---

## Quality Checklist (Every PR)

- [ ] Follows ICommand or ChatParticipant pattern
- [ ] 3+ test cases (happy + edge cases)
- [ ] JSDoc on public APIs
- [ ] No breaking changes
- [ ] No console warnings
- [ ] Execution <10ms

---

## Resources

**Discord/Slack**: `#phase-2` channel  
**GitHub**: Use issues for discussions  
**Docs**: Everything in PHASE_2_*.md files  
**Code**: Reference PHASE_2_QUICK_START.md examples  

---

## Key Dates

| Date | Event |
|------|-------|
| Nov 8 (Fri) | Phase 2 docs ready |
| Nov 11 (Mon) | Kickoff meeting + Day 1 starts |
| Nov 15 (Fri) | Week 3 review |
| Nov 22 (Fri) | Week 4 review |
| Nov 29 (Fri) | Week 5 review |
| Dec 6 (Fri) | Phase 2 complete ✅ |
| Dec 9 (Mon) | Phase 3 starts |

---

## Commands to Know

```bash
# Test locally
npm test

# Lint
npm run lint

# Compile TypeScript
npm run compile

# Build for production
npm run package

# Watch for changes
npm run watch
```

---

## Issues You'll Work On

**Week 3**: #1-6 (Command system)  
**Week 4**: #7-11 (Participant system)  
**Week 5**: #12-13 (State management)  
**Week 6**: #14-17 (Testing + docs)  

See PHASE_2_GITHUB_ISSUES.md for full list.

---

## Metrics (Track Weekly)

- [ ] Tasks completed vs. planned
- [ ] Test coverage % (target: >90%)
- [ ] GitHub issues closed
- [ ] PR reviews completed
- [ ] Blockers resolved

---

## Architecture at a Glance

```
ChatView (UI)
    ↓
CommandRegistry → Commands (7 modules)
    ↓
ParticipantRegistry → Participants (4 modules)
    ↓
ConversationManager → MessageStore
    ↓
VtcodeBackend (agent)
```

---

## If You Get Stuck

1. **Check**: PHASE_2_QUICK_START.md examples
2. **Read**: PHASE_2_IMPLEMENTATION_PLAN.md specs
3. **Ask**: Tech lead in Slack/Discord
4. **Create**: GitHub issue with details

**Don't**: Guess or work around problems

---

## Common Mistakes to Avoid

❌ Hardcoding VS Code APIs in commands  
✅ Inject via constructor

❌ Making participants do too much  
✅ Focus on one context type

❌ Skipping error handling  
✅ Use try-catch + logging

❌ Tests after code  
✅ Write tests alongside code

---

## Definition of Done (Per Task)

✅ Code written  
✅ Tests passing  
✅ Linter passing  
✅ Documented  
✅ Code reviewed  
✅ Merged to main  

---

## Phase After Phase 2

**Phase 3** (Weeks 7-9): Chat features
- Conversation persistence
- Tool approval UI
- Streaming improvements

**Phase 4** (Weeks 10-12): Polish + release
- Performance optimization
- Marketplace preparation

---

## Final Thoughts

- **Don't rush** - Quality > speed
- **Ask questions** - Better than wrong answers
- **Write tests** - Save time debugging later
- **Communicate** - Keep team informed
- **You got this** - Phase 1 team proved capability

---

## Contact

- **Tech Lead**: [Name] - Architecture questions
- **Slack Channel**: #phase-2 - Team updates
- **GitHub Issues**: For discussions

---

**Status**: 🚀 Ready to Launch  
**Start Date**: November 11, 2025  
**Duration**: 4 weeks  
**Effort**: 25 dev days  
**Impact**: Foundation for all future improvements

---

**→ First action: Read PHASE_2_SUMMARY.md**

---

Version 1.0 | Created Nov 8, 2025 | Ready to Print
