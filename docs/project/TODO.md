reference and explore research /Users/vinhnguyenxuan/Developer/learn-by-doing/claude-code-main and apply learning to improve vtcode codebase.

===

completley remove the .env file support completely and only use keyring

.env removal — this is a broad breaking change. The current /secret command already directs users to secure storage instead of .env. Removing .env support entirely should be a separate decision with its own migration plan.

==

When your application needs a large number of tools, declaring all of them up front in the top-level tools field of every request leads to Tool Definition Bloat: every request carries the descriptions and parameter schemas of all tools, driving up token usage, and the more candidate tools there are, the more likely the model picks the wrong tool or constructs invalid call arguments.

Dynamically Loaded Tools let you inject tools on demand during a conversation: start with only a few core tools, and insert additional tools into messages when the conversation actually needs them — reducing token usage and improving tool-selection accuracy at the same time. For the reasoning behind this design (lazy loading, tool registry) and combined practices.

https://platform.kimi.ai/docs/guide/kimi-k3-tool-calling-best-practice
