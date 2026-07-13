# VT Code Platform Compatibility Matrix

Inspired by [caniuse.rs](https://caniuse.rs), this document tracks feature availability and platform support across the VT Code ecosystem.

## Quick Reference

| Feature | Linux | macOS | Windows | WASM | Notes |
|---------|-------|-------|---------|------|-------|
| Core CLI | Yes | Yes | Yes | No | Full terminal agent support |
| TUI Interface | Yes | Yes | Yes | No | Requires crossterm |
| Keyring Auth | Yes | Yes | Yes | No | OS-specific backends |
| MCP Protocol | Yes | Yes | Yes | No | Model Context Protocol |
| ACP Client | Yes | Yes | Yes | Partial | Agent Client Protocol |
| File Watching | Yes | Yes | Yes | No | notify crate |
| Desktop Notifications | Yes | Yes | Yes | No | Optional feature |
| Bash Runner | Yes | Yes | Improved | No | Windows uses PowerShell with cross-platform process groups |
| PTY Sessions | Yes | Yes | Partial | No | Limited on Windows |

**Legend:** Yes = Fully Supported | Partial = Partial Support | No = Not Supported

---

## Minimum Supported Rust Version (MSRV)

**Current MSRV: Rust 1.88**

All VT Code crates require Rust 1.88 or later due to dependencies (ratatui 0.30, darling 0.23, sysinfo 0.37, zip 8.1).

### Crate Version Matrix

| Crate | Version | MSRV | Edition | Published | Notes |
|-------|---------|------|---------|-----------|-------|
| vtcode | 0.133.21 | 1.88 | 2024 | Yes | Binary crate |
| vtcode-core | 0.133.21 | 1.88 | 2024 | Yes | Main runtime |
| vtcode-config | 0.133.21 | 1.88 | 2024 | Yes | Configuration |
| vtcode-commons | 0.133.21 | 1.88 | 2024 | Yes | Shared primitives |
| vtcode-indexer | 0.133.21 | 1.88 | 2024 | Yes | File indexing + markdown storage |
| vtcode-bash-runner | 0.133.21 | 1.88 | 2024 | Yes | Shell execution |
| vtcode-exec-events | 0.133.21 | 1.88 | 2024 | Yes | Event schemas |
| vtcode-session-store | 0.133.21 | 1.88 | 2024 | No | Internal (publish=false) |
| vtcode-eval | 0.135.4 | 1.88 | 2024 | No | Internal (publish=false) |
| vtcode-acp | 0.133.21 | 1.88 | 2024 | Yes | Agent Communication Protocol |
| vtcode-auth | 0.133.21 | 1.88 | 2024 | Yes | OAuth/PKCE |
| vtcode-macros | 0.133.21 | 1.88 | 2024 | Yes | Proc macros |
| vtcode-ui | 0.133.21 | 1.88 | 2024 | Yes | TUI framework |
| vtcode-utility-tool-specs | 0.133.21 | 1.88 | 2024 | Yes | Tool schemas |
| vtcode-safety | 0.133.21 | 1.88 | 2024 | No | Internal (publish=false) |
| vtcode-a2a | 0.133.21 | 1.88 | 2024 | No | Internal (publish=false) |
| vtcode-mcp | 0.133.21 | 1.88 | 2024 | No | Internal (publish=false) |
| vtcode-llm | 0.133.21 | 1.88 | 2024 | No | Internal (publish=false) |
| vtcode-skills | 0.133.21 | 1.88 | 2024 | No | Internal (publish=false) |
| xtask | 0.133.21 | 1.88 | 2024 | No | Internal build tasks |

---

## Platform-Specific Features

### Linux

| Feature | Status | Dependencies | Notes |
|---------|--------|--------------|-------|
| Secret Service (Keyring) | Yes | `libsecret` | Requires `secret-service` DBus interface |
| Desktop Notifications | Yes | `libnotify` | Via `notify-rust` crate |
| PTY Sessions | Yes | `libc` | Full Unix PTY support |
| File Permissions | Yes | `libc` | Unix permission model |
| Signal Handling | Yes | `signal-hook` | POSIX signals |

### macOS

| Feature | Status | Dependencies | Notes |
|---------|--------|--------------|-------|
| Keychain (Keyring) | Yes | Security Framework | Native macOS keychain |
| Desktop Notifications | Yes | `mac-notification-sys` | Native NSUserNotification |
| PTY Sessions | Yes | `libc` | Full Unix PTY support |
| File Permissions | Yes | `libc` | Unix permission model |
| Signal Handling | Yes | `signal-hook` | POSIX signals |
| Touch ID Auth | No | - | Not yet implemented |

### Windows

| Feature | Status | Dependencies | Notes |
|---------|--------|--------------|-------|
| Credential Manager (Keyring) | Yes | Windows API | Via `keyring` crate |
| Desktop Notifications | Yes | Windows Runtime | Via `windows` crate |
| PTY Sessions | Partial | `conpty` | Limited via ConPTY |
| File Permissions | Partial | `windows-sys` | ACL-based, not Unix perms |
| Signal Handling | Partial | `windows-sys` | Limited Ctrl+C/Ctrl+Break |
| PowerShell Integration | Yes | - | Default shell backend |

### WebAssembly (WASM)

| Feature | Status | Notes |
|---------|--------|-------|
| Core Logic | Partial | Pure Rust logic may work |
| TUI Interface | No | Requires terminal backend |
| File System | No | No native FS access |
| Network | Partial | Via WASI or web APIs |
| Keyring | No | No OS keyring access |
| PTY/Bash | No | No process spawning |

---

## LLM Provider Compatibility

| Provider | Status | Feature Flag | Notes |
|----------|--------|--------------|-------|
| OpenAI | Yes | `openai` | Full support |
| Anthropic | Yes | `anthropic-api` | Via HTTP API |
| Google (Gemini) | Yes | `google` | Full support |
| Ollama | Yes | `ollama` | Local models |
| OpenRouter | Yes | `openrouter` | Multi-provider gateway |
| LM Studio | Yes | `lmstudio` | Local inference |
| DeepSeek | Yes | `deepseek` | Full support |
| Moonshot | Yes | `moonshot` | Full support |
| Z.AI | Yes | `zai` | GLM-5.2 support |
| Xiaomi MiMo | Yes | `mimo` | Full support |
| Evolink | Yes | `evolink` | Full support |
| StepFun | Yes | `stepfun` | Full support |
| MiniMax | Yes | `minimax` | Full support |
| Poolside | Yes | `poolside` | Full support |
| Qwen | Yes | `qwen` | Alibaba Cloud |
| HuggingFace | Yes | `huggingface` | Inference Providers |
| OpenCode Zen | Yes | `opencode-zen` | Full support |
| OpenCode Go | Yes | `opencode-go` | Full support |

---

## Feature Flags Matrix

| Feature | Default | Description | Platform Notes |
|---------|---------|-------------|----------------|
| `tool-chat` | Yes | Enable tool-based chat | All platforms |
| `a2a-server` | Yes | Agent-to-Agent protocol server | All platforms |
| `anthropic-api` | No | Anthropic API integration | All platforms |
| `desktop-notifications` | No | OS desktop notifications | Not on WASM |
| `schema` | No | JSON schema export | All platforms |
| `telemetry-tracing` | No | Tracing instrumentation | All platforms |

---

## Dependency Compatibility

### Critical Dependencies

| Dependency | Version | MSRV | Platform Support |
|------------|---------|------|------------------|
| `tokio` | 1.49 | 1.70 | All platforms |
| `crossterm` | 0.29 | 1.70 | All platforms |
| `ratatui` | 0.30 | 1.70 | All platforms |
| `serde` | 1.0 | 1.56 | All platforms |
| `reqwest` | 0.12 | 1.63 | All platforms |
| `keyring` | 3.x | 1.70 | Linux, macOS, Windows |
| `notify` | 6.1 | 1.70 | All platforms |

### Optional Dependencies

| Dependency | Version | Feature | Platform Support |
|------------|---------|---------|------------------|
| `notify-rust` | 4.12 | `desktop-notifications` | Linux, macOS, Windows |
| `schemars` | 1.2 | `schema` | All platforms |
| `axum` | 0.8 | `a2a-server` | All platforms |

---

## Known Limitations

### Cross-Platform Issues

1. **File Permissions**: Unix permission model doesn't map cleanly to Windows ACLs
2. **PTY Support**: Windows ConPTY has different behavior than Unix PTYs
3. **Signal Handling**: Windows has limited signal support (Ctrl+C, Ctrl+Break only)
4. **Keyring Fallback**: Auto-fallback from keyring to file storage when unavailable

### Platform-Specific Workarounds

| Platform | Issue | Workaround |
|----------|-------|------------|
| Linux | Missing `libsecret` | Falls back to file-based storage |
| macOS | Old macOS versions | Limited to macOS 10.13+ |
| Windows | PowerShell execution policy | May require `Set-ExecutionPolicy` |

---

## Checking Compatibility

### Check MSRV Compatibility

```bash
# Verify MSRV across all crates
cargo msrv verify

# Check for newer MSRV requirements
cargo msrv find
```

### Check Platform Support

```bash
# Check Linux build
cargo build --target x86_64-unknown-linux-gnu

# Check macOS build
cargo build --target aarch64-apple-darwin
cargo build --target x86_64-apple-darwin

# Check Windows build
cargo build --target x86_64-pc-windows-msvc
cargo build --target x86_64-pc-windows-gnu

# Check WASM support (limited)
cargo build --target wasm32-unknown-unknown --no-default-features
```

### Check Feature Compatibility

```bash
# Check with all features
cargo check --all-features

# Check with no default features
cargo check --no-default-features

# Check specific feature combinations
cargo check --features "desktop-notifications,anthropic-api"
```

---

## Contributing

When adding new features, please:

1. **Update this matrix**: Document platform support in the appropriate section
2. **Add feature flags**: Use conditional compilation for platform-specific code
3. **Test cross-platform**: Verify builds on at least 2 platforms
4. **Document limitations**: Note any platform-specific caveats

### Template for New Features

```markdown
| New Feature | Linux | macOS | Windows | WASM | Notes |
|-------------|-------|-------|---------|------|-------|
| Feature Name | Yes/Partial/No | Yes/Partial/No | Yes/Partial/No | Yes/Partial/No | Implementation notes |
```

---

## Related Resources

- [caniuse.rs](https://caniuse.rs) - Rust feature stability tracker
- [rust-lang.github.io/rust-clippy](https://rust-lang.github.io/rust-clippy) - Clippy lint documentation
- [arewewebyet.org](https://arewewebyet.org) - Rust web ecosystem compatibility
- [rust-lang.github.io/rfcs](https://rust-lang.github.io/rfcs) - Rust RFCs and feature tracking

---

*Last updated: 2026-06-28*
*VT Code Version: 0.133.21*
