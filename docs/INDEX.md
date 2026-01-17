# VT Code Documentation Index

## Context Manager Implementation (NEW - Priority)

Complete analysis and implementation guide for adopting OpenAI Codex's conversation history patterns.

### 📍 Start Here
- **[README_CONTEXT_MANAGER.md](./README_CONTEXT_MANAGER.md)** - Overview and navigation guide

### 📚 Full Documentation
- **[CODEX_PATTERNS_SUMMARY.md](./CODEX_PATTERNS_SUMMARY.md)** - Executive summary (10 min read)
- **[CONTEXT_MANAGER_ANALYSIS.md](./CONTEXT_MANAGER_ANALYSIS.md)** - Deep analysis (20 min read)
- **[CONTEXT_MANAGER_IMPLEMENTATION.md](./CONTEXT_MANAGER_IMPLEMENTATION.md)** - Code ready to implement (30 min read)
- **[CONTEXT_MANAGER_QUICKSTART.md](./CONTEXT_MANAGER_QUICKSTART.md)** - Day-by-day checklist (15 min read)

### 📋 Status
✅ Analysis complete
✅ Code ready to implement
✅ Tests written
📅 Timeline: 2 weeks for Phase 1

---

## Other Documentation

### Architecture & Design
- **[ARCHITECTURE.md](./ARCHITECTURE.md)** - System architecture overview
- **[SECURITY_MODEL.md](./SECURITY_MODEL.md)** - Security patterns and guidelines

### Tools & Integration
- **[MCP_INTEGRATION_GUIDE.md](./MCP_INTEGRATION_GUIDE.md)** - Model Context Protocol integration
- **[MCP_IMPROVEMENTS.md](./MCP_IMPROVEMENTS.md)** - Planned MCP enhancements
- **[MCP_ROADMAP.md](./MCP_ROADMAP.md)** - MCP implementation roadmap
- **[anthropic-api.md](./anthropic-api.md)** - Anthropic API compatibility server

### Configuration
- **[config/CONFIGURATION_PRECEDENCE.md](./config/CONFIGURATION_PRECEDENCE.md)** - Config loading order

### Development
- **[development/testing.md](./development/testing.md)** - Testing guide
- **[CONTRIBUTING.md](./CONTRIBUTING.md)** - Contribution guidelines

---

## Code Patterns & Examples

### Codex Pattern Analysis (Previous Studies)
- **[CODEX_PATTERNS_SUMMARY.md](./CODEX_PATTERNS_SUMMARY.md)** - Skills and architecture patterns
- **[CODEX_SKILLS_PATTERNS_APPLIED.md](./CODEX_SKILLS_PATTERNS_APPLIED.md)** - Skill implementation patterns

---

## Quick Navigation by Role

### 👨‍💼 Tech Leads / Decision Makers
1. [CODEX_PATTERNS_SUMMARY.md](./CODEX_PATTERNS_SUMMARY.md) - Should we adopt this?
2. [README_CONTEXT_MANAGER.md](./README_CONTEXT_MANAGER.md) - Implementation checklist

### 👨‍💻 Developers (Ready to Code)
1. [CONTEXT_MANAGER_QUICKSTART.md](./CONTEXT_MANAGER_QUICKSTART.md) - Start here
2. [CONTEXT_MANAGER_IMPLEMENTATION.md](./CONTEXT_MANAGER_IMPLEMENTATION.md) - Code patterns
3. `cargo test history_invariant_tests` - Run tests

### 🏛️ Architects / Deep Dive
1. [CONTEXT_MANAGER_ANALYSIS.md](./CONTEXT_MANAGER_ANALYSIS.md) - Comparison matrix
2. [ARCHITECTURE.md](./ARCHITECTURE.md) - System design
3. [README_CONTEXT_MANAGER.md](./README_CONTEXT_MANAGER.md) - Integration points

### 🔧 DevOps / Integration
1. [config/CONFIGURATION_PRECEDENCE.md](./config/CONFIGURATION_PRECEDENCE.md) - Configuration
2. [MCP_INTEGRATION_GUIDE.md](./MCP_INTEGRATION_GUIDE.md) - MCP setup
3. [development/testing.md](./development/testing.md) - CI/CD testing

---

## Key Files Location

### Documentation
- All `.md` files: `./docs/` (including these)

### Source Code
- Main binary: `./src/`
- Core library: `./vtcode-core/src/`
- Workspace: 11 crates in root

### Configuration
- `vtcode.toml` - Runtime config
- `Cargo.toml` - Build config
- `.mcp.json` - MCP server config

---

## Recent Additions

- ✅ **Context Manager Analysis** (Dec 31, 2025) - Complete Codex pattern study
- ✅ **Implementation Guide** (Dec 31, 2025) - Ready-to-code patterns
- ✅ **Quickstart Checklist** (Dec 31, 2025) - 1-2 week timeline

---

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for:
- Code standards (use CLAUDE.md)
- Pull request process
- Commit conventions

---

## Getting Help

1. **For implementation**: See CONTEXT_MANAGER_QUICKSTART.md
2. **For design questions**: See CONTEXT_MANAGER_ANALYSIS.md
3. **For approval**: See CODEX_PATTERNS_SUMMARY.md
4. **For code patterns**: See CONTEXT_MANAGER_IMPLEMENTATION.md

---

**Last Updated**: December 31, 2025
**Current Focus**: Context Manager implementation (Phase 1 ready)
