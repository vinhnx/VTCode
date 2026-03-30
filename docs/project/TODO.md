NOTE: use deepwiki mcp to reference from codex https://deepwiki.com/openai/codex

---

extract and open source more components from vtcode-core

---

CODEX plus

main account
kiweuro
writedownapp
humidapp
vtchat.io

---

https://code.claude.com/docs/en/headless

---

hooks

https://developers.openai.com/codex/hooks

https://deepwiki.com/search/how-does-hooks-works-in-codex_68383f0e-ec03-44eb-be92-69a26aa3d1e1?mode=fast

https://code.claude.com/docs/en/hooks

==

plugins and LSP

https://code.claude.com/docs/en/discover-plugins

https://code.claude.com/docs/en/plugins-reference

https://developers.openai.com/codex/plugins

https://deepwiki.com/search/httpsdevelopersopenaicomcodexp_ee8404d4-ca94-48ac-9fad-60e24e3b4f5a?mode=fast

---

use https://github.com/Uzaaft/libghostty-rs/ replace existing libghostty-vt impl.

---

PLAN mode: handle interactive model picker for /subagent create/edit/udpate interactively instead of requiring manual YAML editing. This is a common source of friction for users creating new subagents, and the current YAML editing approach is error-prone and not user-friendly.

---

checking modal blocking, when an inline modal is showing (eg: the /login modal), currently the whole UI is not selecteable, copyable, or interactable. This is a problem because users may want to copy text from the UI while the modal is open, or interact with other parts of the UI without closing the modal. meaining currently all the modal inline is blocking all interactions with the UI, which is not ideal for user experience. We should make it so that when an inline modal is open, only the modal itself is blocking interactions, while the rest of the UI remains selectable and interactable. This way users can still copy text or interact with other parts of the UI without having to close the modal first.

---

Two of the most powerful features in Claude Code: /loop and /schedule

Use these to schedule Claude to run automatically at a set interval, for up to a week at a time.

I have a bunch of loops running locally:

- /loop 5m /babysit, to auto-address code review, auto-rebase, and shepherd my PRs to production
- /loop 30m /slack-feedback, to automatically put up PRs for Slack feedback every 30 mins
- /loop /post-merge-sweeper to put up PRs to address code review comments I missed
- /loop 1h /pr-pruner to close out stale and no longer necessary PRs
- lots more!..

Experiment with turning workflows into skills + loops. It's powerful.

---

Use hooks to deterministically run logic as part of the agent lifecycle

For example, use hooks to:

- Dynamically load in context each time you start Claude (SessionStart)
- Log every bash command the model runs (PreToolUse)
- Route permission prompts to WhatsApp for you to approve/deny (PermissionRequest)
- Poke Claude to keep going whenever it stops (Stop)

See https://code.claude.com/docs/en/hooks
