as of 2026-04-26 there are no formal benchmarks, so evaluate it on your own codebases.

The plan is to have a benchmark suite that runs on a regular basis, but it is not yet implemented.

--

Improve tool policy permission, currently it keep asking for similar permissions repeatedly, which is annoying and inefficient. The tool should remember the permissions granted and not ask for them again unless they are changed or revoked. --> aim for autonomous permission management that minimizes user prompts while ensuring security.

.vtcode/tool-policy.json

---

keep .vtcode/tool-policy.json out of version control to protect sensitive information and avoid conflicts. Use environment variables or secure vaults for managing permissions and credentials instead.

---

prevent auto scroll TUI when user explicit scroll to a buffer, as it can be disruptive. Implement a mechanism to detect user-initiated scroll actions and temporarily disable auto-scrolling until the user scrolls back to the bottom or re-enables it.
