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

https://deepwiki.com/instructkr/claw-code

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

https://www.mintlify.com/VineeTagarwaL-code/claude-code/concepts/how-it-works

---

/Users/vinhnguyenxuan/Developer/learn-by-doing/claude-code-main

---

https://github.com/openai/codex/pull/12334

https://github.com/openai/codex/pull/15860

--

Claude Code's Real Secret Sauce (Probably) Isn't the Model
Turns out Claude Code source code was leaked today. I saw several snapshots of the typescript code base on GitHub. I don't want to link here for legal reasons, but there are some interesting educational tidbits that can be learned here.
Of course, it's probably common knowledge that Claude Code works better for coding than the Claude web chat because it is not just a chat interface with a shell added to it but more of a carefully designed tool with some nice prompt and context optimizations.
I should also say that while a lot of the qualitative coding performance comes from the model itself, I believe the reason why Claude Code is so good is this software harness, meaning that if we were to drop in other models (say DeepSeek, MiniMax, or Kimi) and optimize this a bit for these models, we would also have very strong coding performance.
Anyways, below are some interesting tidbits for educational purposes to better understand how coding agents work.

1.  Claude Code Builds a Live Repo Context
    This is maybe most obvious, but when you start prompting, Claude loads the main git branch, current git branch, recent commits, etc. in addition to CLAUDE.md for context.
2.  Aggressive Prompt Cache Reuse
    There seems to be something like a boundary marker that separates static and dynamic content. Meaning the static sections are globally cached for stability so that the expensive parts do not need to be rebuilt and reprocessed every time.
3.  The Tooling Is Better Than "Chat With Uploaded Files"
    The prompt seems to tell the model to uses a dedicated Grep tool instead of invoking grep or rg through Bash, presumably because the dedicated tool has better permission handling and (perhaps?) better result collection.
    There is also a dedicated Glob tool for file discovery. And finally it also has a LSP (Language Server Protocol) tool for call hierarchy, finding references etc. That should be a big "power up" compared to the Chat UI, which (I think) sees the code more as static text.
4.  Minimizing Context Bloat
    One of the biggest problems is, of course, the limited context size when working with code repos. This is especially true if we have back-and-forths with the agent and repeated file reads, log files, long shell outputs etc.
    There is a lot of plumbing in Claude Code to minimize that. For example, they do have file-read deduplication that checks whether a file is unchanged and then doesn't reprocess these unchanged files.
    Also, if tool results do get too large, they care written they are written to disk, and the context only uses a preview plus a file reference.
    And, of course, similar to any modern LLM UI, it would automatically truncate long contexts and run autocompaction (/summarization) if needed.
5.  Structured Session Memory
    Claude Code keeps a structured markdown file for the current conversation with sections like:

        Session Title
        Current State
        Task specification
        Files and Functions
        Workflow
        Errors & Corrections
        Codebase and System Documentation
        Learnings
        Key results
        Worklog

It's kind of how we humans code, I'd say, where we keep notes and summaries. 6. It Uses Forks and Subagents
This is probably no surprise that Claude Code parallizes work with subagents. That was basically one of the selling points over Codex for a long time (until Codex recently also added subagent support).
Here, forked agents reuse the parent's cache while being aware or mutable states. So, that lets the system do side work such as summarization, memory extraction, or background analysis without contaminating the main agent loop.
Why This Probably Feels And Works Better Than Coding in the Web UI
All in all, the reason why Claude Code works better than the plain web UI is not prompt engineering or a better model. It's all these little performance and context handling improvement listed above. And there is the convenience, of course, too, in having everything nice and organized on your computer versus uploading files to a Chat UI.

===

THIS: https://github.com/openai/codex/pull/15525

==

===

check enter to use default doesn't work on /compact wizard

===

simplify /compact, no need to use wizard step by step, do it auto matically https://developers.openai.com/api/docs/guides/compaction

---
