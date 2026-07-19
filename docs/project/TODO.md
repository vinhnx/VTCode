reference and explore research /Users/vinhnguyenxuan/Developer/learn-by-doing/claude-code-main and apply learning to improve vtcode codebase.

===

completley remove the .env file support completely and only use keyring

.env removal — this is a broad breaking change. The current /secret command already directs users to secure storage instead of .env. Removing .env support entirely should be a separate decision with its own migration plan.

check stash "env migration"
