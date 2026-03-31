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

check ghostty sidebar unvaiable

```

Ghostty VT sidecar unavailable. PTY snapshots will use legacy_vt100.
```

use https://github.com/Uzaaft/libghostty-rs/ replace existing libghostty-vt impl.

---

use PLAN mode: handle interactive model picker for /subagent create/edit/udpate interactively instead of requiring manual YAML editing. This is a common source of friction for users creating new subagents, and the current YAML editing approach is error-prone and not user-friendly.

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

---

IMPROVE modal visualize

1. add top & bottom border color from modal header section. check subagent header color for reference. this will create a visual connection between the modal header, list selection and main content, making it clearer that they belong together. it will also add a nice design touch to the modal, making it look more polished and cohesive.

ref current status and improve: `/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/modal-border.png`

2. add ansi color to header heading text. this will make the header more visually appealing and easier to read, especially if we use a color that contrasts well with the background. it will also add a nice design touch to the modal, making it look more polished and cohesive.

--

https://developers.openai.com/codex/app-server

---

check and fix codemod follow Alex advices

```
what do you think about these

1. [if possible] a deterministic step with no FP that transforms simple cases
2. [if possible] A determinism step with no FN that grabs (dont change) all possible remaining cases, even if some should not change but ai will decide that later in the next step.
3. An AI step that has the instructions to handle only the tricky cases and instructions not to change things that it shouldn’t.
   a) Point out any cases that aren’t/can’t be automated
   b) Point out any edge cases so AI can review them
   c) Post-codemod review instructions
```

---

and did you confirm its using our MCP and core codemod skill?
the codemods are using string modification instead of AST! (not good)

---

can we use codex app server on our vtcode system?

https://developers.openai.com/codex/app-server

https://deepwiki.com/search/does-vtcode-use-codexs-app-ser_f456c35a-3ba6-4ddb-bd57-ab1144913eb3?mode=fast

---

when doing oauth, can not control+c to abort the oauth flow. need to ctrl+c to regain control of the terminal, but this also kills the process. need a way to abort the oauth flow without killing the process.

---

https://www.mintlify.com/VineeTagarwaL-code/claude-code/concepts/how-it-works

---

/Users/vinhnguyenxuan/Developer/learn-by-doing/claude-code-main

---

https://github.com/openai/codex/pull/12334

https://github.com/openai/codex/pull/15860
