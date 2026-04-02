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

===

/init becomes a guided AGENTS.md setup

it reads the repo, figures out the stuff agents actually need to know, and asks a couple targeted questions when the important bits aren’t obvious from the codebase

===

https://draft.ryhl.io/blog/shared-mutable-state/

---

https://github.com/vinhnx/VTCode/issues/621

---

sticky last user message in header on scroll, so that user can always see the last message they sent to the agent, which is important for context and reference when the conversation gets long.

For example, if the user sends a message with a task specification or important instructions, they can always see that message at the top of the conversation even as they scroll through the agent's responses and tool outputs. This helps maintain context and ensures that the user can easily refer back to their last input without having to scroll all the way back up.

===

fix deploy

https://github.com/vinhnx/VTCode/actions/runs/23882925364/job/69639657112

https://github.com/vinhnx/VTCode/actions/runs/23882925364/job/69639657115#logs

==

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

===

check implement lightweight model selection picker, besides current options. like main model picker and remember settings preference. Currently we don't have the options to pick from different models, but we can implement a lightweight model selector that allows users to choose from available models and remember their preferences for future sessions. This would enhance user experience by providing more flexibility and personalization in their interactions with the agent.
