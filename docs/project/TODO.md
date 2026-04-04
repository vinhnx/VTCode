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

CRITICAL: currently github copilot auth doesn't show device login code -> can't login. check recent regerssions and fix

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

===

https://draft.ryhl.io/blog/shared-mutable-state/

---

https://github.com/vinhnx/VTCode/issues/621

===

https://platform.claude.com/cookbook/tool-use-context-engineering-context-engineering-tools

===

https://www.anthropic.com/engineering/harness-design-long-running-apps

===

Anthropic published two context engineering posts last month. The cookbook covers compaction, clearing, and memory. The harness post covers multi-agent architecture for long-running coding sessions.

The cookbook is a good starting point if you're new to context management. If you've been building agents, most of it is familiar.

But a few patterns were useful regardless of what SDK or framework you're using:

- When you clear old tool results from context, you invalidate your cached prompt prefix. So clearing needs to be worth the cache miss. If you clear too often and free too little each time, you're paying cache-write overhead every turn for minimal gain. Clear in bigger, less frequent chunks.

- When you override the default compaction/summarization prompt, you're replacing it entirely. The default usually includes task-continuation scaffolding (state, next steps, open threads). If you write your own, you need to cover that yourself. Easy to miss.

- If you're combining context clearing with a memory/note-taking tool, exclude the memory tool from clearing. Otherwise the agent's own memory reads get wiped and it loses track of what it just saved. This applies to any agent system where one tool's outputs are inputs to another.

The harness post had more I didn't know.

Context anxiety is different from context rot. Context rot is recall degrading as the window fills. Context anxiety is the model wrapping up prematurely because it thinks it's running out of context. Compaction doesn't fix it because the conversational history is still there, just compressed. Only a full context reset (fresh agent + structured handoff) fixes it.

In self-evaluation, the generator reliably praised its own work, even when the output was broken. Separating the evaluator into its own agent and calibrating it to be skeptical was far more tractable than making a generator self-critical. The calibration took multiple rounds of reading evaluator logs and fixing where its judgment diverged from the developer's. Few-shot examples with score breakdowns.

Every component in an agent harness encodes an assumption about what the model can't do on its own. When they upgraded from Opus 4.5 to 4.6, the sprint decomposition became unnecessary (this is a good example of why you have to rewrite all your code from scratch every 6 months). The model held coherence over 2+ hour builds without it. The harness doesn't shrink as models improve.

One last note that seems like Claude is keeping the planner focused on product context and high-level technical design, not granular implementation details, while Codex folks are posting against a separate planning mode. If the planner gets a technical detail wrong, the error cascades into downstream implementation. Better to constrain on what to build and let the generator figure out how.

---

https://code.claude.com/docs/en/fullscreen

---

https://zread.ai/instructkr/claude-code

---

https://x.com/jpschroeder/status/2038960058499768427

===

https://x.com/cryptodavidw/status/2039198363615457538

---

review system prompt to reduce tokens and optimize for better performance. Consider what information is essential for the model to perform well and remove any redundant or non-critical details. keep in mind that a more concise prompt can often lead to faster response times and improved performance, as the model can focus on the most relevant information without being overwhelmed by unnecessary context. goal: reduce system prompt tokens by at least 50% while maintaining or improving model performance. but first, tell me how many tokens the current system prompt is using and identify any areas where it can be streamlined or simplified.

Build (x86_64-unknown-linux-gnu)
Node.js 20 actions are deprecated. The following actions are running on Node.js 20 and may not work as expected: mlugg/setup-zig@v2. Actions will be forced to run with Node.js 24 by default starting June 2nd, 2026. Node.js 20 will be removed from the runner on September 16th, 2026. Please check if updated versions of these actions are available that support Node.js 24. To opt into Node.js 24 now, set the FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true environment variable on the runner or in your workflow file. Once Node.js 24 becomes the default, you can temporarily opt out by setting ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION=true. For more information see: https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/

Build (x86_64-unknown-linux-gnu)
Node.js 20 actions are deprecated. The following actions are running on Node.js 20 and may not work as expected: mlugg/setup-zig@v2. Actions will be forced to run with Node.js 24 by default starting June 2nd, 2026. Node.js 20 will be removed from the runner on September 16th, 2026. Please check if updated versions of these actions are available that support Node.js 24. To opt into Node.js 24 now, set the FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true environment variable on the runner or in your workflow file. Once Node.js 24 becomes the default, you can temporarily opt out by setting ACTIONS_ALLOW_USE_UNSECURE_NODE_VERSION=true. For more information see: https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/

--

idea

https://x.com/tmeszar/status/2039650776654512623

---

---

Fix duplicated tool call and display in transcript
