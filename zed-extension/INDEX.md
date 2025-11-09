# VTCode Zed Extension - Documentation Index

Complete navigation guide for the VTCode Zed extension documentation.

## Quick Navigation

### 🚀 Getting Started
- **[SETUP_GUIDE.md](SETUP_GUIDE.md)** - Installation and setup instructions
- **[QUICK_START.md](QUICK_START.md)** - 5-minute quick start guide
- **[README.md](README.md)** - Extension overview and features

### 📚 Documentation
- **[extension-features.md](extension-features.md)** - Detailed feature documentation
- **[IMPLEMENTATION_ROADMAP.md](IMPLEMENTATION_ROADMAP.md)** - Development roadmap
- **[DEVELOPMENT.md](DEVELOPMENT.md)** - Development setup and workflow

### 📋 License
- **[LICENSE](LICENSE)** - MIT License

---

## By Use Case

### I Want to...

#### ...Install the Extension
→ Start with **[SETUP_GUIDE.md](SETUP_GUIDE.md)**
- Prerequisite checks
- Installation steps (3 options)
- Verification checklist
- Troubleshooting

#### ...Get Started in 5 Minutes
→ Read **[QUICK_START.md](QUICK_START.md)**
- Install prerequisites (1 min)
- Install extension (1 min)
- Create basic configuration (1 min)
- First use examples (2 min)

#### ...Understand What Features Are Available
→ Check **[extension-features.md](extension-features.md)**
- Current features
- Planned features
- Architecture overview
- Configuration structure
- Workflow examples

#### ...Set Up Development Environment
→ Follow **[DEVELOPMENT.md](DEVELOPMENT.md)**
- Prerequisites verification
- Building the extension
- Installing as dev extension
- Testing and debugging
- Project structure
- Troubleshooting

#### ...Understand Future Development Plans
→ Review **[IMPLEMENTATION_ROADMAP.md](IMPLEMENTATION_ROADMAP.md)**
- Current status
- 3-phase development plan
- Implementation details
- Timeline estimates
- Success criteria

#### ...Find General Information
→ Read **[README.md](README.md)**
- Features overview
- Installation methods
- Basic usage
- Configuration
- Requirements
- Support information

---

## Document Purposes

| Document | Purpose | Audience | Time |
|----------|---------|----------|------|
| **SETUP_GUIDE.md** | Complete installation and troubleshooting | Users | 15-30 min |
| **QUICK_START.md** | Minimal viable setup | New users | 5 min |
| **README.md** | General overview | Everyone | 5-10 min |
| **extension-features.md** | Feature documentation | Users, developers | 10-15 min |
| **DEVELOPMENT.md** | Development workflow | Contributors | 20-30 min |
| **IMPLEMENTATION_ROADMAP.md** | Future plans and tasks | Contributors, maintainers | 15-20 min |
| **INDEX.md** | This file - navigation | Everyone | 5 min |

---

## Reading Paths

### Path 1: Just Install and Use (15 minutes)
1. [SETUP_GUIDE.md](SETUP_GUIDE.md) - Installation
2. [QUICK_START.md](QUICK_START.md) - First steps
3. [README.md](README.md) - Reference

### Path 2: Learn Everything (45 minutes)
1. [README.md](README.md) - Overview
2. [SETUP_GUIDE.md](SETUP_GUIDE.md) - Installation
3. [QUICK_START.md](QUICK_START.md) - Hands-on
4. [extension-features.md](extension-features.md) - Deep dive

### Path 3: Contribute Code (60 minutes)
1. [README.md](README.md) - Overview
2. [DEVELOPMENT.md](DEVELOPMENT.md) - Setup
3. [IMPLEMENTATION_ROADMAP.md](IMPLEMENTATION_ROADMAP.md) - Tasks
4. Review `src/lib.rs` - Source code

### Path 4: Understand Architecture (30 minutes)
1. [extension-features.md](extension-features.md) - Architecture diagram
2. [IMPLEMENTATION_ROADMAP.md](IMPLEMENTATION_ROADMAP.md) - Design details
3. [src/lib.rs](src/lib.rs) - Current implementation

---

## Key Sections by Document

### SETUP_GUIDE.md
- Prerequisites (Zed, Rust, VTCode CLI)
- 5 installation methods
- Configuration examples
- Troubleshooting matrix
- Environment variables
- Performance tuning

### QUICK_START.md
- 1-minute VTCode CLI install
- 1-minute extension install
- 1-minute configuration
- 2 minutes first use
- Configuration patterns
- Tips & tricks

### README.md
- Feature overview
- Installation instructions
- Quick start
- Commands list
- Configuration options
- Requirements
- Contributing info

### extension-features.md
- Current & planned features
- Architecture diagram
- Configuration structure
- Workflow examples
- Dependencies
- Performance notes
- Security model

### DEVELOPMENT.md
- Prerequisites (Rust setup)
- Build commands
- Installation as dev extension
- Testing procedures
- Project structure
- Build & publish instructions
- Troubleshooting for devs

### IMPLEMENTATION_ROADMAP.md
- Current status checklist
- Phase 1-3 breakdown
- Implementation details
- File structure
- Timeline & dependencies
- Success criteria
- Contributing guidelines

---

## Quick Reference

### Installation Command
```bash
# Fastest way to get started
cargo install vtcode
# Then use "Install Dev Extension" in Zed
```

### Key Files
- **Configuration**: `vtcode.toml` (in your workspace)
- **Extension Config**: `extension.toml`
- **Source Code**: `src/lib.rs`
- **License**: `LICENSE` (MIT)

### Common Commands
```bash
# Build extension
cargo build --release

# Check compilation
cargo check

# View help
vtcode --help
```

### API Credentials
```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
```

---

## File Organization

```
zed-extension/
├── Documentation Files (*.md)
│   ├── INDEX.md (this file)
│   ├── README.md (overview)
│   ├── SETUP_GUIDE.md (installation)
│   ├── QUICK_START.md (5-min guide)
│   ├── DEVELOPMENT.md (dev setup)
│   ├── IMPLEMENTATION_ROADMAP.md (roadmap)
│   └── extension-features.md (features)
│
├── Configuration Files
│   ├── extension.toml (extension metadata)
│   ├── Cargo.toml (Rust config)
│   └── languages/vtcode/config.toml (language support)
│
├── Source Code
│   └── src/lib.rs (extension code)
│
├── Build Artifacts
│   └── target/
│       ├── debug/
│       └── release/libvtcode.dylib
│
└── Project Files
    ├── LICENSE (MIT)
    └── .gitignore
```

---

## Support & Resources

### Official Resources
- **GitHub**: [vinhnx/vtcode](https://github.com/vinhnx/vtcode)
- **Issues**: [Report bugs](https://github.com/vinhnx/vtcode/issues)
- **Discussions**: [Ask questions](https://github.com/vinhnx/vtcode/discussions)

### External Resources
- **Zed Docs**: [zed.dev/docs](https://zed.dev/docs)
- **Rust Book**: [doc.rust-lang.org](https://doc.rust-lang.org)
- **VTCode CLI**: [GitHub repo](https://github.com/vinhnx/vtcode)

### Tools & Utilities
- **Rust Installation**: [rustup.rs](https://rustup.rs)
- **Zed Download**: [zed.dev/download](https://zed.dev/download)
- **Cargo Package Manager**: [crates.io](https://crates.io)

---

## Frequently Asked Questions

### Q: Where do I start?
**A**: Go to [SETUP_GUIDE.md](SETUP_GUIDE.md) if installing, or [QUICK_START.md](QUICK_START.md) for a fast overview.

### Q: How do I troubleshoot installation issues?
**A**: See the "Troubleshooting" section in [SETUP_GUIDE.md](SETUP_GUIDE.md).

### Q: What should I read for development?
**A**: Follow the "Path 3" reading path above, starting with [DEVELOPMENT.md](DEVELOPMENT.md).

### Q: What features are planned?
**A**: Check [IMPLEMENTATION_ROADMAP.md](IMPLEMENTATION_ROADMAP.md) for the full roadmap.

### Q: Can I contribute?
**A**: Yes! See contributing guidelines in [DEVELOPMENT.md](DEVELOPMENT.md).

### Q: Is this extension official?
**A**: It's the official Zed extension for VTCode, maintained as part of the VTCode project.

---

## Document Versions

| Document | Last Updated | Status |
|----------|--------------|--------|
| INDEX.md | Nov 2024 | Current |
| README.md | Nov 2024 | Current |
| SETUP_GUIDE.md | Nov 2024 | Current |
| QUICK_START.md | Nov 2024 | Current |
| DEVELOPMENT.md | Nov 2024 | Current |
| IMPLEMENTATION_ROADMAP.md | Nov 2024 | Current |
| extension-features.md | Nov 2024 | Current |

---

## Quick Links

### Installation & Setup
- 🚀 [Get Started in 5 Minutes](QUICK_START.md)
- 📖 [Full Setup Guide](SETUP_GUIDE.md)
- ⚙️ [Troubleshooting](SETUP_GUIDE.md#troubleshooting-installation)

### Understanding the Extension
- 📚 [Features & Capabilities](extension-features.md)
- 🏗️ [Architecture](extension-features.md#architecture)
- 📋 [Configuration Options](extension-features.md#configuration-structure)

### Development
- 🛠️ [Development Setup](DEVELOPMENT.md)
- 🗺️ [Implementation Roadmap](IMPLEMENTATION_ROADMAP.md)
- 💻 [Source Code](src/lib.rs)

### Community & Support
- 🐛 [Report Issues](https://github.com/vinhnx/vtcode/issues)
- 💬 [Discussions](https://github.com/vinhnx/vtcode/discussions)
- ⭐ [Star on GitHub](https://github.com/vinhnx/vtcode)

---

**Need help?** Check the relevant document above or open an issue on GitHub!

**Last Updated**: November 2024  
**Extension Version**: 0.1.0  
**Status**: Ready for Installation
