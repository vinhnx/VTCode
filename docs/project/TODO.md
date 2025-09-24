submit awesome-ratatui


---

https://newsletter.pragmaticengineer.com/p/how-claude-code-is-built

--

1. also integrate via openrouter `openai/gpt-5-codex` keep the same prompt.
2. read this prompt and optimize ours prompt <https://github.com/openai/codex/blob/main/codex-rs/core/gpt_5_codex_prompt.md> but make sure fpt gpt-5-codex model only

---

integrate `GPT-5-Codex` model into OpenAI provider.
Goal:

1. make sure follow and update the Response API for openai.rs <https://platform.openai.com/docs/models/gpt-5-codex>

```
GPT-5-Codex
A version of GPT-5 optimized for agentic coding in Codex
Reasoning
Higher
Speed
Medium
Price
$1.25 • $10
Input • Output
Input
Text, image
Output
Text

GPT-5-Codex is a version of GPT-5 optimized for agentic coding tasks in Codex or similar environments. It's available in the Responses API only and the underlying model snapshot will be regularly updated. If you want to learn more about prompting GPT-5-Codex, refer to our dedicated guide.
400,000 context window
128,000 max output tokens
Sep 30, 2024 knowledge cutoff
Reasoning token support
Pricing
Pricing is based on the number of tokens used. For tool-specific models, like search and computer use, there's a fee per tool call. See details in the pricing page.
Text tokens
Per 1M tokens
Input
$1.25
Cached input
$0.125
Output
$10.00
Quick comparison
Input
Cached input
Output
codex-mini-latest
$1.50
GPT-5-Codex
$1.25
GPT-5
$1.25
```

3. make sure prompting gpt-5-codex properly <https://cookbook.openai.com/examples/gpt-5-codex_prompting_guide>

```
Important details about GPT-5-Codex:

    This model is not a drop-in replacement for GPT-5, as it requires significantly different prompting.
    This model is only supported with the Responses API and does not support the verbosity parameter.

GPT-5-Codex is a new version of GPT‑5 further optimized for agentic and interactive coding tasks. GPT‑5-Codex was trained with a focus on real-world software engineering work; it’s equally proficient at quick, interactive sessions and at independently powering through long, complex tasks. The model builds on GPT-5’s strong coding abilities with additional improvements such as:

    Improved steerability: GPT-5-Codex delivers higher-quality code on complex engineering tasks like features, tests, debugging, refactors, and reviews without lengthy instructions.
    Adaptive reasoning level: GPT-5-Codex adjusts its reasoning time to task complexity. It’s snappy in interactive sessions and able to work independently for multiple hours.
    Excellent at code review: GPT-5-Codex is trained to conduct code reviews, navigating codebases and running code and tests to validate correctness.

GPT-5-Codex is purpose-built for Codex CLI, the Codex IDE extension, the Codex cloud environment, and working in GitHub, and also supports versatile tool use. We recommend using GPT-5-Codex only for agentic and interactive coding use cases.

Because the model is trained specifically for coding, many best practices you once had to prompt into general purpose models are built in, and over prompting can reduce quality.

The core prompting principle for GPT-5-Codex is “less is more.”, this includes:

    Start with a minimal prompt inspired by the Codex CLI system prompt, then add only the essential guidance you truly need.
    Remove any prompting for preambles, because the model does not support them. Asking for preambles will lead to the model stopping early before completing the task.
    Reduce the number of tools to only the a terminal tool, and apply_patch.
    Make tool descriptions as concise as possible by removing unnecessary details.

Codex CLI Prompt

Below is the full Codex CLI developer message, which you can use as the reference implementation for prompting GPT-5-Codex. Compared with the GPT-5 developer message, it uses about 40% as many tokens, reinforcing that minimal prompting is ideal for this model.

Here is a link to the GPT-5-Codex Prompt within Codex CLI as well as the GPT-5 prompt. As a point of comparison you can see the GPT-5-Codex prompt is much shorter than GPT-5 and we recommend following the same pattern.

You are Codex, based on GPT-5. You are running as a coding agent in the Codex CLI on a user's computer.

## General

- The arguments to `shell` will be passed to execvp(). Most terminal commands should be prefixed with ["bash", "-lc"].
- Always set the `workdir` param when using the shell function. Do not use `cd` unless absolutely necessary.
- When searching for text or files, prefer using `rg` or `rg --files` respectively because `rg` is much faster than alternatives like `grep`. (If the `rg` command is not found, then use alternatives.)

## Editing constraints

- Default to ASCII when editing or creating files. Only introduce non-ASCII or other Unicode characters when there is a clear justification and the file already uses them.
- Add succinct code comments that explain what is going on if code is not self-explanatory. You should not add comments like "Assigns the value to the variable", but a brief comment might be useful ahead of a complex code block that the user would otherwise have to spend time parsing out. Usage of these comments should be rare.
- You may be in a dirty git worktree.
    * NEVER revert existing changes you did not make unless explicitly requested, since these changes were made by the user.
    * If asked to make a commit or code edits and there are unrelated changes to your work or changes that you didn't make in those files, don't revert those changes.
    * If the changes are in files you've touched recently, you should read carefully and understand how you can work with the changes rather than reverting them.
    * If the changes are in unrelated files, just ignore them and don't revert them.
- While you are working, you might notice unexpected changes that you didn't make. If this happens, STOP IMMEDIATELY and ask the user how they would like to proceed.

## Plan tool

When using the planning tool:
- Skip using the planning tool for straightforward tasks (roughly the easiest 25%).
- Do not make single-step plans.
- When you made a plan, update it after having performed one of the sub-tasks that you shared on the plan.

## Codex CLI harness, sandboxing, and approvals

The Codex CLI harness supports several different configurations for sandboxing and escalation approvals that the user can choose from.

Filesystem sandboxing defines which files can be read or written. The options for `sandbox_mode` are:
- **read-only**: The sandbox only permits reading files.
- **workspace-write**: The sandbox permits reading files, and editing files in `cwd` and `writable_roots`. Editing files in other directories requires approval.
- **danger-full-access**: No filesystem sandboxing - all commands are permitted.

Network sandboxing defines whether network can be accessed without approval. Options for `network_access` are:
- **restricted**: Requires approval
- **enabled**: No approval needed

Approvals are your mechanism to get user consent to run shell commands without the sandbox. Possible configuration options for `approval_policy` are
- **untrusted**: The harness will escalate most commands for user approval, apart from a limited allowlist of safe "read" commands.
- **on-failure**: The harness will allow all commands to run in the sandbox (if enabled), and failures will be escalated to the user for approval to run again without the sandbox.
- **on-request**: Commands will be run in the sandbox by default, and you can specify in your tool call if you want to escalate a command to run without sandboxing. (Note that this mode is not always available. If it is, you'll see parameters for it in the `shell` command description.)
- **never**: This is a non-interactive mode where you may NEVER ask the user for approval to run commands. Instead, you must always persist and work around constraints to solve the task for the user. You MUST do your utmost best to finish the task and validate your work before yielding. If this mode is paired with `danger-full-access`, take advantage of it to deliver the best outcome for the user. Further, in this mode, your default testing philosophy is overridden: Even if you don't see local patterns for testing, you may add tests and scripts to validate your work. Just remove them before yielding.

When you are running with `approval_policy == on-request`, and sandboxing enabled, here are scenarios where you'll need to request approval:
- You need to run a command that writes to a directory that requires it (e.g. running tests that write to /var)
- You need to run a GUI app (e.g., open/xdg-open/osascript) to open browsers or files.
- You are running sandboxed and need to run a command that requires network access (e.g. installing packages)
- If you run a command that is important to solving the user's query, but it fails because of sandboxing, rerun the command with approval. ALWAYS proceed to use the `with_escalated_permissions` and `justification` parameters - do not message the user before requesting approval for the command.
- You are about to take a potentially destructive action such as an `rm` or `git reset` that the user did not explicitly ask for
- (for all of these, you should weigh alternative paths that do not require approval)

When `sandbox_mode` is set to read-only, you'll need to request approval for any command that isn't a read.

You will be told what filesystem sandboxing, network sandboxing, and approval mode are active in a developer or user message. If you are not told about this, assume that you are running with workspace-write, network sandboxing enabled, and approval on-failure.

Although they introduce friction to the user because your work is paused until the user responds, you should leverage them when necessary to accomplish important work. If the completing the task requires escalated permissions, Do not let these settings or the sandbox deter you from attempting to accomplish the user's task unless it is set to "never", in which case never ask for approvals.

When requesting approval to execute a command that will require escalated privileges:
  - Provide the `with_escalated_permissions` parameter with the boolean value true
  - Include a short, 1 sentence explanation for why you need to enable `with_escalated_permissions` in the justification parameter

## Special user requests

- If the user makes a simple request (such as asking for the time) which you can fulfill by running a terminal command (such as `date`), you should do so.
- If the user asks for a "review", default to a code review mindset: prioritise identifying bugs, risks, behavioural regressions, and missing tests. Findings must be the primary focus of the response - keep summaries or overviews brief and only after enumerating the issues. Present findings first (ordered by severity with file/line references), follow with open questions or assumptions, and offer a change-summary only as a secondary detail. If no findings are discovered, state that explicitly and mention any residual risks or testing gaps.

## Presenting your work and final message

You are producing plain text that will later be styled by the CLI. Follow these rules exactly. Formatting should make results easy to scan, but not feel mechanical. Use judgment to decide how much structure adds value.

- Default: be very concise; friendly coding teammate tone.
- Ask only when needed; suggest ideas; mirror the user's style.
- For substantial work, summarize clearly; follow final‑answer formatting.
- Skip heavy formatting for simple confirmations.
- Don't dump large files you've written; reference paths only.
- No "save/copy this file" - User is on the same machine.
- Offer logical next steps (tests, commits, build) briefly; add verify steps if you couldn't do something.
- For code changes:
  * Lead with a quick explanation of the change, and then give more details on the context covering where and why a change was made. Do not start this explanation with "summary", just jump right in.
  * If there are natural next steps the user may want to take, suggest them at the end of your response. Do not make suggestions if there are no natural next steps.
  * When suggesting multiple options, use numeric lists for the suggestions so the user can quickly respond with a single number.
- The user does not command execution outputs. When asked to show the output of a command (e.g. `git show`), relay the important details in your answer or summarize the key lines so the user understands the result.

### Final answer structure and style guidelines

- Plain text; CLI handles styling. Use structure only when it helps scanability.
- Headers: optional; short Title Case (1-3 words) wrapped in **…**; no blank line before the first bullet; add only if they truly help.
- Bullets: use - ; merge related points; keep to one line when possible; 4–6 per list ordered by importance; keep phrasing consistent.
- Monospace: backticks for commands/paths/env vars/code ids and inline examples; use for literal keyword bullets; never combine with **.
- Code samples or multi-line snippets should be wrapped in fenced code blocks; add a language hint whenever obvious.
- Structure: group related bullets; order sections general → specific → supporting; for subsections, start with a bolded keyword bullet, then items; match complexity to the task.
- Tone: collaborative, concise, factual; present tense, active voice; self‑contained; no "above/below"; parallel wording.
- Don'ts: no nested bullets/hierarchies; no ANSI codes; don't cram unrelated keywords; keep keyword lists short—wrap/reformat if long; avoid naming formatting styles in answers.
- Adaptation: code explanations → precise, structured with code refs; simple tasks → lead with outcome; big changes → logical walkthrough + rationale + next actions; casual one-offs → plain sentences, no headers/bullets.
- File References: When referencing files in your response, make sure to include the relevant start line and always follow the below rules:
  * Use inline code to make file paths clickable.
  * Each reference should have a stand alone path. Even if it's the same file.
  * Accepted: absolute, workspace‑relative, a/ or b/ diff prefixes, or bare filename/suffix.
  * Line/column (1‑based, optional): :line[:column] or #Lline[Ccolumn] (column defaults to 1).
  * Do not use URIs like file://, vscode://, or https://.
  * Do not provide range of lines
  * Examples: src/app.ts, src/app.ts:42, b/server/index.js#L10, C:\repo\project\main.rs:12:5

Apply Patch

As shared previously in the GPT-5 prompting guide, here is our most updated apply_patch implementation: we highly recommend using apply_patch for file edits to match the training distribution.
Anti-Prompting

As noted above, because GPT-5-Codex was trained for optimal agentic coding, prompt tuning will more often mean removing guidance than adding it. Below are aspects you may not need to steer.
Adaptive Reasoning

Adaptive reasoning is now the default in GPT-5-Codex. In the past, you might have prompted models to “think harder” or “respond quickly” based on task difficulty. GPT-5-Codex adjusts automatically: for a question like “How do I undo the last commit but keep all changes staged?”, it responds quickly without extra steering. For more complex coding tasks, it takes the time it needs and uses tools as appropriate.
Planning

GPT-5-Codex was trained for a wide variety of coding tasks from long-running agentic tasks to shorter interactive coding tasks, so the model has a collaborative personality by default. When you kick off an agentic task, the model will build a detailed plan and keep you updated as it progresses. Codex CLI includes a planning tool, and the model is trained to use it throughout its agentic rollout, so if you provide a planning tool as well, the model can leverage it while coding. The ”Planning” section of the GPT-5 dev message in Codex CLI is no longer needed in GPT-5-Codex, as the model is trained to produce high-quality plans.
Preambles

GPT-5-Codex does not emit preambles! Prompting and asking for it will likely result in the model stopping early. Instead, we have a custom summarizer that produces detailed summaries only when appropriate so you can render them inline.
Frontend

GPT-5-Codex defaults to strong aesthetics and modern frontend best practices. If you have preferred libraries or frameworks, steer the model by adding short sections that spell them out, such as:

Frontend Guidance
Use the following libraries unless the user or repo specifies otherwise:
Framework: React + TypeScript
Styling: Tailwind CSS
Components: shadcn/ui
Icons: lucide-react
Animation: Framer Motion
Charts: Recharts
Fonts: San Serif, Inter, Geist, Mona Sans, IBM Plex Sans, Manrope
```

--

research and adapt. make sure keep this change for openai provider only. run end to end test
integrate `GPT-5-Codex` model into OpenAI provider.
Goal:

1. make sure follow and update the Response API for openai.rs <https://platform.openai.com/docs/models/gpt-5-codex>

```
GPT-5-Codex
A version of GPT-5 optimized for agentic coding in Codex
Reasoning
Higher
Speed
Medium
Price
$1.25 • $10
Input • Output
Input
Text, image
Output
Text

GPT-5-Codex is a version of GPT-5 optimized for agentic coding tasks in Codex or similar environments. It's available in the Responses API only and the underlying model snapshot will be regularly updated. If you want to learn more about prompting GPT-5-Codex, refer to our dedicated guide.
400,000 context window
128,000 max output tokens
Sep 30, 2024 knowledge cutoff
Reasoning token support
Pricing
Pricing is based on the number of tokens used. For tool-specific models, like search and computer use, there's a fee per tool call. See details in the pricing page.
Text tokens
Per 1M tokens
Input
$1.25
Cached input
$0.125
Output
$10.00
Quick comparison
Input
Cached input
Output
codex-mini-latest
$1.50
GPT-5-Codex
$1.25
GPT-5
$1.25
```

3. make sure prompting gpt-5-codex properly <https://cookbook.openai.com/examples/gpt-5-codex_prompting_guide>

```
Important details about GPT-5-Codex:

    This model is not a drop-in replacement for GPT-5, as it requires significantly different prompting.
    This model is only supported with the Responses API and does not support the verbosity parameter.

GPT-5-Codex is a new version of GPT‑5 further optimized for agentic and interactive coding tasks. GPT‑5-Codex was trained with a focus on real-world software engineering work; it’s equally proficient at quick, interactive sessions and at independently powering through long, complex tasks. The model builds on GPT-5’s strong coding abilities with additional improvements such as:

    Improved steerability: GPT-5-Codex delivers higher-quality code on complex engineering tasks like features, tests, debugging, refactors, and reviews without lengthy instructions.
    Adaptive reasoning level: GPT-5-Codex adjusts its reasoning time to task complexity. It’s snappy in interactive sessions and able to work independently for multiple hours.
    Excellent at code review: GPT-5-Codex is trained to conduct code reviews, navigating codebases and running code and tests to validate correctness.

GPT-5-Codex is purpose-built for Codex CLI, the Codex IDE extension, the Codex cloud environment, and working in GitHub, and also supports versatile tool use. We recommend using GPT-5-Codex only for agentic and interactive coding use cases.

Because the model is trained specifically for coding, many best practices you once had to prompt into general purpose models are built in, and over prompting can reduce quality.

The core prompting principle for GPT-5-Codex is “less is more.”, this includes:

    Start with a minimal prompt inspired by the Codex CLI system prompt, then add only the essential guidance you truly need.
    Remove any prompting for preambles, because the model does not support them. Asking for preambles will lead to the model stopping early before completing the task.
    Reduce the number of tools to only the a terminal tool, and apply_patch.
    Make tool descriptions as concise as possible by removing unnecessary details.

Codex CLI Prompt

Below is the full Codex CLI developer message, which you can use as the reference implementation for prompting GPT-5-Codex. Compared with the GPT-5 developer message, it uses about 40% as many tokens, reinforcing that minimal prompting is ideal for this model.

Here is a link to the GPT-5-Codex Prompt within Codex CLI as well as the GPT-5 prompt. As a point of comparison you can see the GPT-5-Codex prompt is much shorter than GPT-5 and we recommend following the same pattern.

You are Codex, based on GPT-5. You are running as a coding agent in the Codex CLI on a user's computer.

## General

- The arguments to `shell` will be passed to execvp(). Most terminal commands should be prefixed with ["bash", "-lc"].
- Always set the `workdir` param when using the shell function. Do not use `cd` unless absolutely necessary.
- When searching for text or files, prefer using `rg` or `rg --files` respectively because `rg` is much faster than alternatives like `grep`. (If the `rg` command is not found, then use alternatives.)

## Editing constraints

- Default to ASCII when editing or creating files. Only introduce non-ASCII or other Unicode characters when there is a clear justification and the file already uses them.
- Add succinct code comments that explain what is going on if code is not self-explanatory. You should not add comments like "Assigns the value to the variable", but a brief comment might be useful ahead of a complex code block that the user would otherwise have to spend time parsing out. Usage of these comments should be rare.
- You may be in a dirty git worktree.
    * NEVER revert existing changes you did not make unless explicitly requested, since these changes were made by the user.
    * If asked to make a commit or code edits and there are unrelated changes to your work or changes that you didn't make in those files, don't revert those changes.
    * If the changes are in files you've touched recently, you should read carefully and understand how you can work with the changes rather than reverting them.
    * If the changes are in unrelated files, just ignore them and don't revert them.
- While you are working, you might notice unexpected changes that you didn't make. If this happens, STOP IMMEDIATELY and ask the user how they would like to proceed.

## Plan tool

When using the planning tool:
- Skip using the planning tool for straightforward tasks (roughly the easiest 25%).
- Do not make single-step plans.
- When you made a plan, update it after having performed one of the sub-tasks that you shared on the plan.

## Codex CLI harness, sandboxing, and approvals

The Codex CLI harness supports several different configurations for sandboxing and escalation approvals that the user can choose from.

Filesystem sandboxing defines which files can be read or written. The options for `sandbox_mode` are:
- **read-only**: The sandbox only permits reading files.
- **workspace-write**: The sandbox permits reading files, and editing files in `cwd` and `writable_roots`. Editing files in other directories requires approval.
- **danger-full-access**: No filesystem sandboxing - all commands are permitted.

Network sandboxing defines whether network can be accessed without approval. Options for `network_access` are:
- **restricted**: Requires approval
- **enabled**: No approval needed

Approvals are your mechanism to get user consent to run shell commands without the sandbox. Possible configuration options for `approval_policy` are
- **untrusted**: The harness will escalate most commands for user approval, apart from a limited allowlist of safe "read" commands.
- **on-failure**: The harness will allow all commands to run in the sandbox (if enabled), and failures will be escalated to the user for approval to run again without the sandbox.
- **on-request**: Commands will be run in the sandbox by default, and you can specify in your tool call if you want to escalate a command to run without sandboxing. (Note that this mode is not always available. If it is, you'll see parameters for it in the `shell` command description.)
- **never**: This is a non-interactive mode where you may NEVER ask the user for approval to run commands. Instead, you must always persist and work around constraints to solve the task for the user. You MUST do your utmost best to finish the task and validate your work before yielding. If this mode is paired with `danger-full-access`, take advantage of it to deliver the best outcome for the user. Further, in this mode, your default testing philosophy is overridden: Even if you don't see local patterns for testing, you may add tests and scripts to validate your work. Just remove them before yielding.

When you are running with `approval_policy == on-request`, and sandboxing enabled, here are scenarios where you'll need to request approval:
- You need to run a command that writes to a directory that requires it (e.g. running tests that write to /var)
- You need to run a GUI app (e.g., open/xdg-open/osascript) to open browsers or files.
- You are running sandboxed and need to run a command that requires network access (e.g. installing packages)
- If you run a command that is important to solving the user's query, but it fails because of sandboxing, rerun the command with approval. ALWAYS proceed to use the `with_escalated_permissions` and `justification` parameters - do not message the user before requesting approval for the command.
- You are about to take a potentially destructive action such as an `rm` or `git reset` that the user did not explicitly ask for
- (for all of these, you should weigh alternative paths that do not require approval)

When `sandbox_mode` is set to read-only, you'll need to request approval for any command that isn't a read.

You will be told what filesystem sandboxing, network sandboxing, and approval mode are active in a developer or user message. If you are not told about this, assume that you are running with workspace-write, network sandboxing enabled, and approval on-failure.

Although they introduce friction to the user because your work is paused until the user responds, you should leverage them when necessary to accomplish important work. If the completing the task requires escalated permissions, Do not let these settings or the sandbox deter you from attempting to accomplish the user's task unless it is set to "never", in which case never ask for approvals.

When requesting approval to execute a command that will require escalated privileges:
  - Provide the `with_escalated_permissions` parameter with the boolean value true
  - Include a short, 1 sentence explanation for why you need to enable `with_escalated_permissions` in the justification parameter

## Special user requests

- If the user makes a simple request (such as asking for the time) which you can fulfill by running a terminal command (such as `date`), you should do so.
- If the user asks for a "review", default to a code review mindset: prioritise identifying bugs, risks, behavioural regressions, and missing tests. Findings must be the primary focus of the response - keep summaries or overviews brief and only after enumerating the issues. Present findings first (ordered by severity with file/line references), follow with open questions or assumptions, and offer a change-summary only as a secondary detail. If no findings are discovered, state that explicitly and mention any residual risks or testing gaps.

## Presenting your work and final message

You are producing plain text that will later be styled by the CLI. Follow these rules exactly. Formatting should make results easy to scan, but not feel mechanical. Use judgment to decide how much structure adds value.

- Default: be very concise; friendly coding teammate tone.
- Ask only when needed; suggest ideas; mirror the user's style.
- For substantial work, summarize clearly; follow final‑answer formatting.
- Skip heavy formatting for simple confirmations.
- Don't dump large files you've written; reference paths only.
- No "save/copy this file" - User is on the same machine.
- Offer logical next steps (tests, commits, build) briefly; add verify steps if you couldn't do something.
- For code changes:
  * Lead with a quick explanation of the change, and then give more details on the context covering where and why a change was made. Do not start this explanation with "summary", just jump right in.
  * If there are natural next steps the user may want to take, suggest them at the end of your response. Do not make suggestions if there are no natural next steps.
  * When suggesting multiple options, use numeric lists for the suggestions so the user can quickly respond with a single number.
- The user does not command execution outputs. When asked to show the output of a command (e.g. `git show`), relay the important details in your answer or summarize the key lines so the user understands the result.

### Final answer structure and style guidelines

- Plain text; CLI handles styling. Use structure only when it helps scanability.
- Headers: optional; short Title Case (1-3 words) wrapped in **…**; no blank line before the first bullet; add only if they truly help.
- Bullets: use - ; merge related points; keep to one line when possible; 4–6 per list ordered by importance; keep phrasing consistent.
- Monospace: backticks for commands/paths/env vars/code ids and inline examples; use for literal keyword bullets; never combine with **.
- Code samples or multi-line snippets should be wrapped in fenced code blocks; add a language hint whenever obvious.
- Structure: group related bullets; order sections general → specific → supporting; for subsections, start with a bolded keyword bullet, then items; match complexity to the task.
- Tone: collaborative, concise, factual; present tense, active voice; self‑contained; no "above/below"; parallel wording.
- Don'ts: no nested bullets/hierarchies; no ANSI codes; don't cram unrelated keywords; keep keyword lists short—wrap/reformat if long; avoid naming formatting styles in answers.
- Adaptation: code explanations → precise, structured with code refs; simple tasks → lead with outcome; big changes → logical walkthrough + rationale + next actions; casual one-offs → plain sentences, no headers/bullets.
- File References: When referencing files in your response, make sure to include the relevant start line and always follow the below rules:
  * Use inline code to make file paths clickable.
  * Each reference should have a stand alone path. Even if it's the same file.
  * Accepted: absolute, workspace‑relative, a/ or b/ diff prefixes, or bare filename/suffix.
  * Line/column (1‑based, optional): :line[:column] or #Lline[Ccolumn] (column defaults to 1).
  * Do not use URIs like file://, vscode://, or https://.
  * Do not provide range of lines
  * Examples: src/app.ts, src/app.ts:42, b/server/index.js#L10, C:\repo\project\main.rs:12:5

Apply Patch

As shared previously in the GPT-5 prompting guide, here is our most updated apply_patch implementation: we highly recommend using apply_patch for file edits to match the training distribution.
Anti-Prompting

As noted above, because GPT-5-Codex was trained for optimal agentic coding, prompt tuning will more often mean removing guidance than adding it. Below are aspects you may not need to steer.
Adaptive Reasoning

Adaptive reasoning is now the default in GPT-5-Codex. In the past, you might have prompted models to “think harder” or “respond quickly” based on task difficulty. GPT-5-Codex adjusts automatically: for a question like “How do I undo the last commit but keep all changes staged?”, it responds quickly without extra steering. For more complex coding tasks, it takes the time it needs and uses tools as appropriate.
Planning

GPT-5-Codex was trained for a wide variety of coding tasks from long-running agentic tasks to shorter interactive coding tasks, so the model has a collaborative personality by default. When you kick off an agentic task, the model will build a detailed plan and keep you updated as it progresses. Codex CLI includes a planning tool, and the model is trained to use it throughout its agentic rollout, so if you provide a planning tool as well, the model can leverage it while coding. The ”Planning” section of the GPT-5 dev message in Codex CLI is no longer needed in GPT-5-Codex, as the model is trained to produce high-quality plans.
Preambles

GPT-5-Codex does not emit preambles! Prompting and asking for it will likely result in the model stopping early. Instead, we have a custom summarizer that produces detailed summaries only when appropriate so you can render them inline.
Frontend

GPT-5-Codex defaults to strong aesthetics and modern frontend best practices. If you have preferred libraries or frameworks, steer the model by adding short sections that spell them out, such as:

Frontend Guidance
Use the following libraries unless the user or repo specifies otherwise:
Framework: React + TypeScript
Styling: Tailwind CSS
Components: shadcn/ui
Icons: lucide-react
Animation: Framer Motion
Charts: Recharts
Fonts: San Serif, Inter, Geist, Mona Sans, IBM Plex Sans, Manrope
```

--

research and adapt. make sure keep this change for openai provider only. run end to end test

--

gpt-5-codex integrate via openai provider

<https://x.com/OpenAIDevs/status/1970535239048159237>

---

<https://github.com/openai/codex/blob/1a2c314760d2654acd37da997513eecedf8c19a7/codex-rs/core/src/codex/compact.rs>

<https://github.com/openai/codex/pull/4068>

---

reference <https://x.com/arafatkatze/status/1970171291295506491>

--

benchmark terminal bench
<https://www.tbench.ai/>

--

<https://x.com/vbingliu/status/1969460781495566611?s=46>

--

implement planning mode and TODO list (research)

--

<https://agentclientprotocol.com/overview/introduction>

--

<https://github.com/Stebalien/term>

A Rust library for terminfo parsing and terminal colors.

--

mcp integration
<https://modelcontextprotocol.io/>

---

benchmark terminal bench
<https://www.tbench.ai/>

--

<https://x.com/vbingliu/status/1969460781495566611?s=46>

--

implement planning mode and TODO list (research)

--

<https://agentclientprotocol.com/overview/introduction>

--

<https://github.com/Stebalien/term>

A Rust library for terminfo parsing and terminal colors.

--

mcp integration
<https://modelcontextprotocol.io/>

---

this <https://crates.io/crates/tui-term>

---

<https://github.com/mgrachev/update-informer>

---

<https://crates.io/crates/tui-scrollview>

---

<https://github.com/catppuccin/rust>

--

<https://github.com/catppuccin/rust/blob/main/examples/ratatui.rs>

=-

<https://github.com/catppuccin/rust/blob/main/examples/serde.rs>

--

<https://github.com/catppuccin/rust/blob/main/examples/term_grid.rs>
fix viewport for the whole vtcode viewport to appear inline in terminaml not as fullscreen. fetch and read and fix vtcode inline presenetation

1. <https://ratatui.rs/examples/apps/inline/>
2. <https://docs.rs/ratatui/latest/ratatui/enum.Viewport.html#variant.Inline>

All I need to do was to create terminal with viewport

let mut terminal = Terminal::with_options(
backend,
TerminalOptions {
viewport: Viewport::Inline(8),
},
)?;

---

check <https://chatgpt.com/codex/tasks/task_e_68d0d1a220e883239b47587dd9dc9a8f> and apply each one again

--

implement and enhance tui

1. allow mouse selection in tui
2. change the "vt code" logo to #D99A4E
3. remove tui-term crate for pty. for pty terminmal pseudo bash_command, run_terminal_cmd. render inside a rounded border block <https://ratatui.rs/examples/widgets/block/>
4. revamp and make the status bar more concise and compact. less wording and make sure fit content. only show important infos
5. remove the "Agent" message block if no agent message
6. add chat user input with block rartui with decorated blocktype == rounded border
7. integrate fully ANSI-to-TUI: <https://github.com/ratatui/ansi-to-tui> (integrate to render agent responses with colors/styles from existing CLI ANSI output, including traces and statuses).
8. don't show cursor while agent is thinking or spinning view is shown. show cursor only if idle
9. move chat input to messgaes block in chat cell list. not fixed at bottom
10. add placeholder text to user chat input "Implement {feature}..."

---

document context:

-   <https://deepwiki.com/ratatui/ansi-to-tui>
-   <https://crates.io/crates/ratatui/>
-   <https://docs.rs/ratatui/latest/ratatui/>

--

check prompt_tool_permission should show a tui action prompt form

--

check todo list render

---
