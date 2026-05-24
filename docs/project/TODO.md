as of 2026-04-26 there are no formal benchmarks, so evaluate it on your own codebases.

The plan is to have a benchmark suite that runs on a regular basis, but it is not yet implemented.

--

Improve tool policy permission, currently it keep asking for similar permissions repeatedly, which is annoying and inefficient. The tool should remember the permissions granted and not ask for them again unless they are changed or revoked. --> aim for autonomous permission management that minimizes user prompts while ensuring security.

.vtcode/tool-policy.json

---

keep .vtcode/tool-policy.json out of version control to protect sensitive information and avoid conflicts. Use environment variables or secure vaults for managing permissions and credentials instead.

---

prevent auto scroll TUI when user explicit scroll to a buffer, as it can be disruptive. Implement a mechanism to detect user-initiated scroll actions and temporarily disable auto-scrolling until the user scrolls back to the bottom or re-enables it.

---

implement hide header option for TUI buffers to provide a cleaner interface when the header is not necessary. This can be toggled on or off based on user preference or context, allowing for a more customizable and focused user experience.

1. if hide: keep only > VT Code ({version}). fixed buffer header with version info, and remove the rest of the header content to reduce clutter. This allows users to focus on the main content while still having access to version information at a glance.
2. keep the full TUI header as is. eg:
   ╭> VT Code (0.108.1)─────────────────────────────────────────────────────────────────╮
   │DeepSeek deepseek-v4-flash (128K) medium | Auto | Full-auto | Memory: On | MCP: │
   ╰────────────────────────────────────────────────────────────────────────────────────╯
