NOTE: use private relay signup codex free

---

NOTE: use deepwiki mcp to reference from codex https://deepwiki.com/openai/codex

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, context and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

---

Conduct a thorough, end-to-end performance audit and systematic optimization of the vtcode agent harness framework with explicit focus on maximizing execution velocity, achieving superior computational efficiency, and implementing aggressive token and context conservation strategies throughout all operational layers. Execute comprehensive refactoring of the tool invocation and agent communication architecture to eliminate redundant processing, minimize inter-process communication latency, and optimize resource utilization at every stage. Design and implement multilayered error handling protocols including predictive failure detection, graceful degradation mechanisms, automatic recovery procedures, and comprehensive logging to drive error occurrence to near-zero levels. Deliver measurable improvements in reliability, throughput, and operational stability while preserving all existing functionality and maintaining backward compatibility with current integration points.

---

extract and open source more components from vtcode-core

---

Review the unified_exec implementation and vtcode's tool ecosystem to identify token efficiency gaps. Analyze which components waste tokens through redundancy, verbosity, or inefficient patterns, and which are already optimized. Develop optimizations for inefficient tools and propose new tools that consolidate multiple operations into single calls to reduce token consumption in recurring workflows.

Specifically examine these known issues: command payloads for non-diff unified_exec still contain duplicated text (output and stdout fields), which wastes tokens across all command-like tool calls. Address this by ensuring unified_exec normalizes all tool calls to eliminate redundant information.

Identify and address these additional token waste patterns: remove duplicated spool guidance that reaches the model both through spool_hint fields and separate system prompts; trim repeated or unused metadata from model-facing tool payloads such as redundant spool_hint fields, spooled_bytes data, duplicate id==session_id entries, and null working_directory values; shorten high-frequency follow-up prompts for PTY and spool-chunk read operations, and implement compact structured continuation arguments for chunked spool reads.

Review each tool's prompt and response structure to ensure conciseness while maintaining effectiveness, eliminating unnecessary verbosity that increases token usage without adding functional value.

---

Perform a comprehensive analysis of the codebase to identify and eliminate all instances of duplicated code, following the DRY (Don't Repeat Yourself) and KISS (Keep It Simple, Stupid) principles. Conduct a systematic search across all modules, classes, and files to find similar code patterns, duplicate logic, redundant implementations, and opportunities for abstraction. Specifically examine rendering-related code such as diff previews and command output previews to determine if they can share unified rendering logic, styling, and common components. Audit all utility functions scattered throughout different modules and extract them into a centralized shared utility module with proper organization and documentation. Create a detailed report identifying each duplication found, the proposed refactoring strategy, and the expected benefits in terms of maintainability, reduced code complexity, and improved consistency. Ensure all refactored code maintains existing functionality while simplifying the overall architecture. Prioritize changes that provide the greatest reduction in duplication with minimal risk to existing functionality.

---

review any duplicated code in the codebase and refactor to remove duplication. For example, the logic for rendering the diff preview and the command output preview can be unified to use the same rendering logic and styling. This will make the codebase cleaner and easier to maintain. Additionally, any common utility functions that are duplicated across different modules can be extracted into a shared utility module. search across modules for similar code patterns and identify opportunities for refactoring to reduce duplication and improve code reuse.

DRY and KISS

---

Conduct a comprehensive review and enhancement of error handling and recovery mechanisms within the agent loop, with particular emphasis on tool call operations. Implement a multi-layered error handling strategy that includes retry logic with exponential backoff for transient failures such as network timeouts, rate limiting, and temporary service unavailability while implementing fail-fast behavior for non-recoverable errors including authentication failures, invalid parameters, and permission denied scenarios. Develop and integrate a robust state management system that ensures the agent can maintain consistent internal state during and after error occurrences, including proper rollback mechanisms for partial operations and transaction-like semantics where appropriate. Create a comprehensive error categorization system that distinguishes between retryable and non-retryable errors and implements appropriate handling strategies for each category. Enhance user-facing error messages to be clear, actionable, and informative while avoiding technical jargon that may confuse end users. Implement proper logging at multiple levels including debug, info, warning, and error levels to facilitate troubleshooting and monitoring. Conduct a thorough audit of existing error handling implementations to identify gaps, inconsistencies, and potential failure points. Refactor the error handling code to improve modularity, testability, and maintainability while ensuring comprehensive test coverage for error scenarios including edge cases and unexpected inputs. Add appropriate circuit breaker patterns for external service calls to prevent cascading failures and enable graceful degradation when dependent services are unavailable. Implement proper resource cleanup and resource leak prevention throughout the agent loop.

---

https://claude.ai/chat/bac1e18f-f11a-496d-b260-7de5948faf7a

---

CODEX plus

main account
kiweuro
writedownapp
humidapp
vtchat.io

---

https://defuddle.md/x.com/akshay_pachaar/status/2031021906254766128

---

Agent runs, human steers. I also apply this philosophy in the VT Code harness.

---

title: "Prompt guidance for GPT-5.4 | OpenAI API"
source: "https://developers.openai.com/api/docs/guides/prompt-guidance"
domain: "developers.openai.com"
language: "en"
description: "Learn prompt patterns and migration guidance for GPT-5.4, including completeness checks, verification loops, tool persistence, and structured outputs."
word_count: 4566

---

GPT-5.4, our newest mainline model, is designed to balance long-running task performance, stronger control over style and behavior, and more disciplined execution across complex workflows. Building on advances from GPT-5 through GPT-5.3-Codex, GPT-5.4 improves token efficiency, sustains multi-step workflows more reliably, and performs well on long-horizon tasks.

GPT-5.4 is designed for production-grade assistants and agents that need strong multi-step reasoning, evidence-rich synthesis, and reliable performance over long contexts. It is especially effective when prompts clearly specify the output contract, tool-use expectations, and completion criteria. In practice, the biggest gains come from choosing the right reasoning effort for the task, using explicit grounding and citation rules, and giving the model a precise definition of what “done” looks like. This guide focuses on prompt patterns and migration practices that preserve those efficiency wins. For model capabilities, API parameters, and broader migration guidance, see [our latest model guide](https://developers.openai.com/api/docs/guides/latest-model).

## Understand GPT-5.4 behavior

### Where GPT-5.4 is strongest

GPT-5.4 tends to work especially well in these areas:

- Strong personality and tone adherence, with less drift over long answers
- Agentic workflow robustness, with a stronger tendency to stick with multi-step work, retry, and complete agent loops end to end
- Evidence-rich synthesis, especially in long-context or multi-tool workflows
- Instruction adherence in modular, skill-based, and block-structured prompts when the contract is explicit
- Long-context analysis across large, messy, or multi-document inputs
- Batched or parallel tool calling while maintaining tool-call accuracy
- Spreadsheet, finance, and Excel workflows that need instruction following, formatting fidelity, and stronger self-verification

### Where explicit prompting still helps

Even with those strengths, GPT-5.4 benefits from more explicit guidance in a few recurring patterns:

- Low-context tool routing early in a session, when tool selection can be less reliable
- Dependency-aware workflows that need explicit prerequisite and downstream-step checks
- Reasoning effort selection, where higher effort is not always better and the right choice depends on task shape, not intuition
- Research tasks that require disciplined source collection and consistent citations
- Irreversible or high-impact actions that require verification before execution
- Terminal or coding-agent environments where tool boundaries must stay clear

These patterns are observed defaults, not guarantees. Start with the smallest prompt that passes your evals, and add blocks only when they fix a measured failure mode.

## Use core prompt patterns

### Keep outputs compact and structured

To improve token efficiency with GPT-5.4, constrain verbosity and enforce structured output through clear output contracts. In practice, this acts as an additional control layer alongside the `verbosity` parameter in the Responses API, allowing you to guide both how much the model writes and how it structures the output.

```xml
1

2

3

4

5

6

7

8

9

10

11

12

13

<output_contract>
- Return exactly the sections requested, in the requested order.
- If the prompt defines a preamble, analysis block, or working section, do not treat it as extra output.
- Apply length limits only to the section they are intended for.
- If a format is required (JSON, Markdown, SQL, XML), output only that format.
</output_contract>

<verbosity_controls>
- Prefer concise, information-dense writing.
- Avoid repeating the user's request.
- Keep progress updates brief.
- Do not shorten the answer so aggressively that required evidence, reasoning, or completion checks are omitted.
</verbosity_controls>
```

### Set clear defaults for follow-through

Users often change the task, format, or tone mid-conversation. To keep the assistant aligned, define clear rules for when to proceed, when to ask, and how newer instructions override earlier defaults.

Use a default follow-through policy like this:

```xml
1

2

3

4

5

6

7

8

<default_follow_through_policy>
- If the user’s intent is clear and the next step is reversible and low-risk, proceed without asking.
- Ask permission only if the next step is:
  (a) irreversible,
  (b) has external side effects (for example sending, purchasing, deleting, or writing to production), or
  (c) requires missing sensitive information or a choice that would materially change the outcome.
- If proceeding, briefly state what you did and what remains optional.
</default_follow_through_policy>
```

Make instruction priority explicit:

```xml
1

2

3

4

5

6

<instruction_priority>
- User instructions override default style, tone, formatting, and initiative preferences.
- Safety, honesty, privacy, and permission constraints do not yield.
- If a newer user instruction conflicts with an earlier one, follow the newer instruction.
- Preserve earlier instructions that do not conflict.
</instruction_priority>
```

Higher-priority developer or system instructions remain binding.

**Guidance:** When instructions change mid-conversation, make the update explicit, scoped, and local. State what changed, what still applies, and whether the change affects the next turn or the rest of the conversation.

### Handle mid-conversation instruction updates

For mid-conversation updates, use explicit, scoped steering messages that state:

1. Scope
2. Override
3. Carry forward

```text
1

2

3

4

5

6

7

8

<task_update>
For the next response only:
- Do not complete the task.
- Only produce a plan.
- Keep it to 5 bullets.

All earlier instructions still apply unless they conflict with this update.
</task_update>
```

If the task itself changes, say so directly:

```text
1

2

3

4

5

6

7

8

9

10

11

12

13

<task_update>
The task has changed.
Previous task: complete the workflow.
Current task: review the workflow and identify risks only.

Rules for this turn:
- Do not execute actions.
- Do not call destructive tools.
- Return exactly:
  1. Main risks
  2. Missing information
  3. Recommended next step
</task_update>
```

### Make tool use persistent when correctness depends on it

Use explicit rules to keep tool use thorough, dependency-aware, and appropriately paced, especially in workflows where later actions rely on earlier retrieval or verification. A common failure mode is skipping prerequisites because the right end state seems obvious.

GPT-5.4 can be less reliable at tool routing early in a session, when context is still thin. Prompt for prerequisites, dependency checks, and exact tool intent.

```xml
1

2

3

4

5

6

7

8

<tool_persistence_rules>
- Use tools whenever they materially improve correctness, completeness, or grounding.
- Do not stop early when another tool call is likely to materially improve correctness or completeness.
- Keep calling tools until:
  (1) the task is complete, and
  (2) verification passes (see <verification_loop>).
- If a tool returns empty or partial results, retry with a different strategy.
</tool_persistence_rules>
```

This is especially important for workflows where the final action depends on earlier lookup or retrieval steps. One of the most common failure modes is skipping prerequisites because the intended end state seems obvious.

```xml
1

2

3

4

5

<dependency_checks>
- Before taking an action, check whether prerequisite discovery, lookup, or memory retrieval steps are required.
- Do not skip prerequisite steps just because the intended final action seems obvious.
- If the task depends on the output of a prior step, resolve that dependency first.
</dependency_checks>
```

Prompt for parallelism when the work is independent and wall-clock matters. Prompt for sequencing when dependencies, ambiguity, or irreversible actions matter more than speed.

```xml
1

2

3

4

5

6

<parallel_tool_calling>
- When multiple retrieval or lookup steps are independent, prefer parallel tool calls to reduce wall-clock time.
- Do not parallelize steps that have prerequisite dependencies or where one result determines the next action.
- After parallel retrieval, pause to synthesize the results before making more calls.
- Prefer selective parallelism: parallelize independent evidence gathering, not speculative or redundant tool use.
</parallel_tool_calling>
```

### Force completeness on long-horizon tasks

For multi-step workflows, a common failure mode is incomplete execution: the model finishes after partial coverage, misses items in a batch, or treats empty or narrow retrieval as final. GPT-5.4 becomes more reliable when the prompt defines explicit completion rules and recovery behavior.

Coverage can be achieved through sequential or parallel retrieval, but completion rules should remain explicit either way.

```xml
1

2

3

4

5

6

7

8

9

<completeness_contract>
- Treat the task as incomplete until all requested items are covered or explicitly marked [blocked].
- Keep an internal checklist of required deliverables.
- For lists, batches, or paginated results:
  - determine expected scope when possible,
  - track processed items or pages,
  - confirm coverage before finalizing.
- If any item is blocked by missing data, mark it [blocked] and state exactly what is missing.
</completeness_contract>
```

For workflows where empty, partial, or noisy retrieval is common:

```xml
1

2

3

4

5

6

7

8

9

10

11

<empty_result_recovery>
If a lookup returns empty, partial, or suspiciously narrow results:
- do not immediately conclude that no results exist,
- try at least one or two fallback strategies,
  such as:
  - alternate query wording,
  - broader filters,
  - a prerequisite lookup,
  - or an alternate source or tool,
- Only then report that no results were found, along with what you tried.
</empty_result_recovery>
```

### Add a verification loop before high-impact actions

Once the workflow appears complete, add a lightweight verification step before returning the answer or taking an irreversible action. This helps catch requirement misses, grounding issues, and format drift before commit.

```xml
1

2

3

4

5

6

7

<verification_loop>
Before finalizing:
- Check correctness: does the output satisfy every requirement?
- Check grounding: are factual claims backed by the provided context or tool outputs?
- Check formatting: does the output match the requested schema or style?
- Check safety and irreversibility: if the next step has external side effects, ask permission first.
</verification_loop>
```

```xml
1

2

3

4

5

<missing_context_gating>
- If required context is missing, do NOT guess.
- Prefer the appropriate lookup tool when the missing context is retrievable; ask a minimal clarifying question only when it is not.
- If you must proceed, label assumptions explicitly and choose a reversible action.
</missing_context_gating>
```

For agents that actively take actions, add a short execution frame:

```xml
1

2

3

4

5

<action_safety>
- Pre-flight: summarize the intended action and parameters in 1-2 lines.
- Execute via tool.
- Post-flight: confirm the outcome and any validation that was performed.
</action_safety>
```

## Handle specialized workflows

### Choose image detail explicitly for vision and computer use

If your workflow depends on visual precision, specify the image `detail` level in the prompt or integration instead of relying on `auto`. Use `high` for standard high-fidelity image understanding. Use `original` for large, dense, or spatially sensitive images, especially [computer use, localization, OCR, and click-accuracy tasks](https://developers.openai.com/api/docs/guides/tools-computer-use) on `gpt-5.4` and future models. Use `low` only when speed and cost matter more than fine detail. For more details on image detail levels, see the [Images and Vision guide](https://developers.openai.com/api/docs/guides/images-vision).

### Lock research and citations to retrieved evidence

When citation quality matters, make both the source boundary and the format requirement explicit. This helps reduce fabricated references, unsupported claims, and citation-format drift.

```xml
1

2

3

4

5

6

<citation_rules>
- Only cite sources retrieved in the current workflow.
- Never fabricate citations, URLs, IDs, or quote spans.
- Use exactly the citation format required by the host application.
- Attach citations to the specific claims they support, not only at the end.
</citation_rules>
```

```xml
1

2

3

4

5

6

<grounding_rules>
- Base claims only on provided context or tool outputs.
- If sources conflict, state the conflict explicitly and attribute each side.
- If the context is insufficient or irrelevant, narrow the answer or say you cannot support the claim.
- If a statement is an inference rather than a directly supported fact, label it as an inference.
</grounding_rules>
```

If your application requires inline citations, require inline citations. If it requires footnotes, require footnotes. The key is to lock the format and prevent the model from improvising unsupported references.

### Research mode

Push GPT-5.4 into a disciplined research mode. Use this pattern for research, review, and synthesis tasks. Do not force it onto short execution tasks or simple deterministic transforms.

```xml
1

2

3

4

5

6

7

<research_mode>
- Do research in 3 passes:
  1) Plan: list 3-6 sub-questions to answer.
  2) Retrieve: search each sub-question and follow 1-2 second-order leads.
  3) Synthesize: resolve contradictions and write the final answer with citations.
- Stop only when more searching is unlikely to change the conclusion.
</research_mode>
```

If your host environment uses a specific research tool or requires a submit step, combine this with the host’s finalization contract.

### Clamp strict output formats

For SQL, JSON, or other parse-sensitive outputs, tell GPT-5.4 to emit only the target format and check it before finishing.

```text
1

2

3

4

5

6

7

<structured_output_contract>
- Output only the requested format.
- Do not add prose or markdown fences unless they were requested.
- Validate that parentheses and brackets are balanced.
- Do not invent tables or fields.
- If required schema information is missing, ask for it or return an explicit error object.
</structured_output_contract>
```

If you are extracting document regions or OCR boxes, define the coordinate system and add a drift check:

```text
1

2

3

4

5

6

<bbox_extraction_spec>
- Use the specified coordinate format exactly, such as [x1,y1,x2,y2] normalized to 0..1.
- For each box, include page, label, text snippet, and confidence.
- Add a vertical-drift sanity check so boxes stay aligned with the correct line of text.
- If the layout is dense, process page by page and do a second pass for missed items.
</bbox_extraction_spec>
```

### Keep tool boundaries explicit in coding and terminal agents

In coding agents, GPT-5.4 works better when the rules for shell access and file editing are unambiguous. This is especially important when you expose tools like [Shell](https://developers.openai.com/api/docs/guides/tools-shell) or [Apply patch](https://developers.openai.com/api/docs/guides/tools-apply-patch).

### User updates

GPT-5.4 does well with brief, outcome-based updates. Reuse the user-updates pattern from the 5.2 guide, but pair it with explicit completion and verification requirements.

Recommended update spec:

```xml
1

2

3

4

5

6

<user_updates_spec>
- Only update the user when starting a new major phase or when something changes the plan.
- Each update: 1 sentence on outcome + 1 sentence on next step.
- Do not narrate routine tool calls.
- Keep the user-facing status short; keep the work exhaustive.
</user_updates_spec>
```

For coding agents, see the Prompting patterns for coding tasks section below for more specific guidance.

### Prompting patterns for coding tasks

**Autonomy and persistence**

GPT-5.4 is generally more thorough end to end than earlier mainline models on coding and tool-use tasks, so you often need less explicit “verify everything” prompting. Still, for high-stakes changes such as production, migrations, or security work, keep a lightweight verification clause.

```xml
1

2

3

4

5

<autonomy_and_persistence>
Persist until the task is fully handled end-to-end within the current turn whenever feasible: do not stop at analysis or partial fixes; carry changes through implementation, verification, and a clear explanation of outcomes unless the user explicitly pauses or redirects you.

Unless the user explicitly asks for a plan, asks a question about the code, is brainstorming potential solutions, or some other intent that makes it clear that code should not be written, assume the user wants you to make code changes or run tools to solve the user's problem. In these cases, it's bad to output your proposed solution in a message, you should go ahead and actually implement the change. If you encounter challenges or blockers, you should attempt to resolve them yourself.
</autonomy_and_persistence>
```

**Intermediary updates**

Keep updates sparse and high-signal. In coding tasks, prefer updates at key points.

```xml
1

2

3

4

5

6

7

8

9

10

11

12

13

14

<user_updates_spec>
- Intermediary updates go to the \`commentary\` channel.
- User updates are short updates while you are working. They are not final answers.
- Use 1-2 sentence updates to communicate progress and new information while you work.
- Do not begin responses with conversational interjections or meta commentary. Avoid openers such as acknowledgements ("Done -", "Got it", or "Great question") or similar framing.
- Before exploring or doing substantial work, send a user update explaining your understanding of the request and your first step. Avoid commenting on the request or starting with phrases such as "Got it" or "Understood."
- Provide updates roughly every 30 seconds while working.
- When exploring, explain what context you are gathering and what you learned. Vary sentence structure so the updates do not become repetitive.
- When working for a while, keep updates informative and varied, but stay concise.
- When work is substantial, provide a longer plan after you have enough context. This is the only update that may be longer than 2 sentences and may contain formatting.
- Before file edits, explain what you are about to change.
- While thinking, keep the user informed of progress without narrating every tool call. Even if you are not taking actions, send frequent progress updates rather than going silent, especially if you are thinking for more than a short stretch.
- Keep the tone of progress updates consistent with the assistant's overall personality.
</user_updates_spec>
```

**Formatting**

GPT-5.4 often defaults to more structured formatting and may overuse bullet lists. If you want a clean final response, explicitly clamp list shape.

```xml
Never use nested bullets. Keep lists flat (single level). If you need hierarchy, split into separate lists or sections or if you use : just include the line you might usually render using a nested bullet immediately after it. For numbered lists, only use the \`1. 2. 3.\` style markers (with a period), never \`1)\`.
```

**Frontend tasks**

Use this only when additional frontend guidance is useful.

```xml
1

2

3

4

5

6

7

8

9

10

11

12

13

14

15

16

17

18

<frontend_tasks>
When doing frontend design tasks, avoid generic, overbuilt layouts.

Use these hard rules:
- One composition: The first viewport must read as one composition, not a dashboard, unless it is a dashboard.
- Brand first: On branded pages, the brand or product name must be a hero-level signal, not just nav text or an eyebrow. No headline should overpower the brand.
- Brand test: If the first viewport could belong to another brand after removing the nav, the branding is too weak.
- Full-bleed hero only: On landing pages and promotional surfaces, the hero image should usually be a dominant edge-to-edge visual plane or background. Do not default to inset hero images, side-panel hero images, rounded media cards, tiled collages, or floating image blocks unless the existing design system clearly requires them.
- Hero budget: The first viewport should usually contain only the brand, one headline, one short supporting sentence, one CTA group, and one dominant image. Do not place stats, schedules, event listings, address blocks, promos, "this week" callouts, metadata rows, or secondary marketing content there.
- No hero overlays: Do not place detached labels, floating badges, promo stickers, info chips, or callout boxes on top of hero media.
- Cards: Default to no cards. Never use cards in the hero unless they are the container for a user interaction. If removing a border, shadow, background, or radius does not hurt interaction or understanding, it should not be a card.
- One job per section: Each section should have one purpose, one headline, and usually one short supporting sentence.
- Real visual anchor: Imagery should show the product, place, atmosphere, or context.
- Reduce clutter: Avoid pill clusters, stat strips, icon rows, boxed promos, schedule snippets, and competing text blocks.
- Use motion to create presence and hierarchy, not noise. Ship 2-3 intentional motions for visually led work, and prefer Framer Motion when it is available.

Exception: If working within an existing website or design system, preserve the established patterns, structure, and visual language.
</frontend_tasks>
```

```xml
1

2

3

4

5

6

<terminal_tool_hygiene>
- Only run shell commands via the terminal tool.
- Never "run" tool names as shell commands.
- If a patch or edit tool exists, use it directly; do not attempt it in bash.
- After changes, run a lightweight verification step such as ls, tests, or a build before declaring the task done.
</terminal_tool_hygiene>
```

### Document localization and OCR boxes

For bbox tasks, be explicit about coordinate conventions and add drift tests.

```xml
1

2

3

4

5

6

7

<bbox_extraction_spec>
- Use the specified coordinate format exactly (for example [x1,y1,x2,y2] normalized 0..1).
- For each bbox, include: page, label, text snippet, confidence.
- Add a vertical-drift sanity check:
  - ensure bboxes align with the line of text (not shifted up or down).
- If dense layout, process page by page and do a second pass for missed items.
</bbox_extraction_spec>
```

### Use runtime and API integration notes

For long-running or tool-heavy agents, the runtime contract matters as much as the prompt contract.

**Phase parameter**

To better support preamble messages with GPT-5.4, the Responses API includes a `phase` field designed to prevent early stopping on longer-running tasks and other misbehaviors.

- `phase` is optional at the API level, but it is highly recommended. Best-effort inference may exist server-side, but explicit round-tripping of `phase` is strictly better.
- Use `phase` for long-running or tool-heavy agents that may emit commentary before tool calls or before a final answer.
- Preserve `phase` when replaying prior assistant items so the model can distinguish working commentary from the completed answer. This matters most in multi-step flows with preambles, tool-related updates, or multiple assistant messages in the same turn.
- Do not add `phase` to user messages.
- If you use `previous_response_id`, that is usually the simplest path, since OpenAI can often recover prior state without manually replaying assistant items.
- If you replay assistant history yourself, preserve the original `phase` values.
- Missing or dropped `phase` can cause preambles to be interpreted as final answers and degrade behavior on longer, multi-step tasks.

### Preserve behavior in long sessions

Compaction unlocks significantly longer effective context windows, where user conversations can persist for many turns without hitting context limits or long-context performance degradation, and agents can perform very long trajectories that exceed a typical context window for long-running, complex tasks.

If you are using [Compaction](https://developers.openai.com/api/docs/guides/compaction) in the Responses API, compact after major milestones, treat compacted items as opaque state, and keep prompts functionally identical after compaction. The endpoint is ZDR compatible and returns an `encrypted_content` item that you can pass into future requests. GPT-5.4 tends to remain more coherent and reliable over longer, multi-turn conversations with fewer breakdowns as sessions grow.

For more guidance, see the [`/responses/compact` API reference](https://developers.openai.com/api/docs/api-reference/responses/compact).

### Control personality for customer-facing workflows

GPT-5.4 can be steered more effectively when you separate persistent personality from per-response writing controls. This is especially useful for customer-facing workflows such as emails, support replies, announcements, and blog-style content.

- **Personality (persistent):** sets the default tone, verbosity, and decision style across the session.
- **Writing controls (per response):** define the channel, register, formatting, and length for a specific artifact.
- **Reminder:** personality should not override task-specific output requirements. If the user asks for JSON, return JSON.

For natural, high-quality prose, the highest-leverage controls are:

- Give the model a clear persona.
- Specify the channel and emotional register.
- Explicitly ban formatting when you want prose.
- Use hard length limits.

```xml
1

2

3

4

5

6

7

8

<personality_and_writing_controls>
- Persona: <one sentence>
- Channel: <Slack | email | memo | PRD | blog>
- Emotional register: <direct/calm/energized/etc.> + "not <overdo this>"
- Formatting: <ban bullets/headers/markdown if you want prose>
- Length: <hard limit, e.g. <=150 words or 3-5 sentences>
- Default follow-through: if the request is clear and low-risk, proceed without asking permission.
</personality_and_writing_controls>
```

For more personality patterns you can lift directly, see the [Prompt Personalities cookbook](https://developers.openai.com/cookbook/examples/gpt-5/prompt_personalities).

**Professional memo mode**

For memos, reviews, and other professional writing tasks, general writing instructions are often not enough. These workflows benefit from explicit guidance on specificity, domain conventions, synthesis, and calibrated certainty.

```xml
1

2

3

4

5

6

7

8

<memo_mode>
- Write in a polished, professional memo style.
- Use exact names, dates, entities, and authorities when supported by the record.
- Follow domain-specific structure if one is requested.
- Prefer precise conclusions over generic hedging.
- When uncertainty is real, tie it to the exact missing fact or conflicting source.
- Synthesize across documents rather than summarizing each one independently.
</memo_mode>
```

This mode is especially useful for legal, policy, research, and executive-facing writing, where the goal is not just fluency, but disciplined synthesis and clear conclusions.

## Tune reasoning and migration

### Treat reasoning effort as a last-mile knob

Reasoning effort is not one-size-fits-all. Treat it as a last-mile tuning knob, not the primary way to improve quality. In many cases, stronger prompts, clear output contracts, and lightweight verification loops recover much of the performance teams might otherwise seek through higher reasoning settings.

Recommended defaults:

- `none`: Best for fast, cost-sensitive, latency-sensitive tasks where the model does not need to think.
- `low`: Works well for latency-sensitive tasks where a small amount of thinking can produce a meaningful accuracy gain, especially with complex instructions.
- `medium` or `high`: Reserve for tasks that truly require stronger reasoning and can absorb the latency and cost tradeoff. Choose between them based on how much performance gain your task gets from additional reasoning.
- `xhigh`: Avoid as a default unless your evals show clear benefits. It is best suited for long, agentic, reasoning-heavy tasks where maximum intelligence matters more than speed or cost.

In practice, most teams should default to the `none`, `low`, or `medium` range.

Start with `none` for execution-heavy workloads such as workflow steps, field extraction, support triage, and short structured transforms.

Start with `medium` or higher for research-heavy workloads such as long-context synthesis, multi-document review, conflict resolution, and strategy writing. With `medium` and a well-engineered prompt, you can squeeze out a lot of performance.

For GPT-5.4 workloads, `none` can already perform well on action-selection and tool-discipline tasks. If your workload depends on nuanced interpretation, such as implicit requirements, ambiguity, or cancelled-tool-call recovery, start with `low` or `medium` instead.

Before increasing reasoning effort, first add:

- `<completeness_contract>`
- `<verification_loop>`
- `<tool_persistence_rules>`

If the model still feels too literal or stops at the first plausible answer, add an initiative nudge before raising reasoning effort:

```xml
1

2

3

4

5

<dig_deeper_nudge>
- Don’t stop at the first plausible answer.
- Look for second-order issues, edge cases, and missing constraints.
- If the task is safety or accuracy critical, perform at least one verification step.
</dig_deeper_nudge>
```

### Migrate prompts to GPT-5.4 one change at a time

Use the same one-change-at-a-time discipline as the 5.2 guide: switch model first, pin `reasoning_effort`, run evals, then iterate.

These starting points work well for many migrations:

| Current setup             | Suggested GPT-5.4 start            | Notes                                                               |
| ------------------------- | ---------------------------------- | ------------------------------------------------------------------- |
| `gpt-5.2`                 | Match the current reasoning effort | Preserve the existing latency and quality profile first, then tune. |
| `gpt-5.3-codex`           | Match the current reasoning effort | For coding workflows, keep the reasoning effort the same.           |
| `gpt-4.1` or `gpt-4o`     | `none`                             | Keep snappy behavior, and increase only if evals regress.           |
| Research-heavy assistants | `medium` or `high`                 | Use explicit research multi-pass and citation gating.               |
| Long-horizon agents       | `medium` or `high`                 | Add tool persistence and completeness accounting.                   |

If you are migrating a research agent in particular, make these prompt updates before increasing reasoning effort:

- Add `<research_mode>`
- Add `<citation_rules>`
- Add `<empty_result_recovery>`
- Increase `reasoning_effort` one notch only after prompt fixes.

You can start from the 5.2 research block and then layer in citation gating and finalization contracts as needed.

GPT-5.4 performs especially well when the task requires multi-step evidence gathering, long-context synthesis, and explicit prompt contracts. In practice, the highest-leverage prompt changes are choosing reasoning effort by task shape, defining exact output and citation formats, adding dependency-aware tool rules, and making completion criteria explicit. The model is often strong out of the box, but it is most reliable when prompts clearly specify how to search, how to verify, and what counts as done.

- Read [our latest model guide](https://developers.openai.com/api/docs/guides/latest-model) for model capabilities, parameters, and API compatibility details.
- Read [Prompt engineering](https://developers.openai.com/api/docs/guides/prompt-engineering) for broader prompting strategies that apply across model families.
- Read [Compaction](https://developers.openai.com/api/docs/guides/compaction) if you are building long-running GPT-5.4 sessions in the Responses API.

---

---

title: "The Anatomy of an Agent Harness"
author: "@Vtrivedy10"
source: "https://x.com/Vtrivedy10/status/2031408954517971368"
language: "en"
word_count: 2277

---

![Cover image](https://pbs.twimg.com/media/HDDw-jHW0AAJasD.jpg)

**TLDR:** Agent = Model + Harness. Harness engineering is how we build systems around models to turn them into work engines. The model contains the intelligence and the harness makes that intelligence useful. We define what a harness is and derive the core components today's and tomorrow's agents need.

## Can Someone Please Define a "Harness"?

Agent = Model + Harness

**If you're not the model, you're the harness.**

A harness is every piece of code, configuration, and execution logic that isn't the model itself. A raw model is not an agent. But it becomes one when a harness gives it things like state, tool execution, feedback loops, and enforceable constraints.

Concretely, a harness includes things like:

- System Prompts
- Tools, Skills, MCPs + and their descriptions
- Bundled Infrastructure (filesystem, sandbox, browser)
- Orchestration Logic (subagent spawning, handoffs, model routing)
- Hooks/Middleware for deterministic execution (compaction, continuation, lint checks)

There are many messy ways to split the boundaries of an agent system between the model and the harness. But in my opinion, this is the cleanest definition because it forces us to think about **designing systems around model intelligence.**

The rest of this post walks through core harness components and derives why each piece exists working backwards from the core primitive of a model.

## Why Do We Need Harnesses…From a Model's Perspective

**There are things we want an agent to do that a model cannot do out of the box. This is where a harness comes in.**

Models (mostly) take in data like text, images, audio, video and they output text. That's it. Out of the box they cannot:

- Maintain durable state across interactions
- Execute code
- Access realtime knowledge
- Setup environments and install packages to complete work

These are all **harness level features**. The structure of LLMs requires some sort of machinery that wraps them to do useful work.

For example, to get a product UX like "chatting", we wrap the model in a while loop to track previous messages and append new user messages. Everyone reading this has already used this kind of harness. The main idea is that we want to convert a desired agent behavior into an actual feature in the harness.

## Working Backwards from Desired Agent Behavior to Harness Engineering

Harness Engineering helps humans inject useful priors to guide agent behavior. And as models have gotten more capable, harnesses have been used to surgically extend and correct models to complete previously impossible tasks.

We won’t go over an exhaustive list of every harness feature. The goal is to derive a set of features from the starting point of helping models do useful work. We’ll follow a pattern like this:

**Behavior we want (or want to fix) → Harness Design to help the model achieve this.**

## Filesystems for Durable Storage and Context Management

**We want agents to have durable storage to interface with real data, offload information that doesn't fit in context, and persist work across sessions.**

Models can only directly operate on knowledge within their context window. Before filesystems, users had to copy/paste content directly to the model, that’s clunky UX and doesn't work for autonomous agents. The world was already using filesystems to do work so models were naturally trained on billions of tokens of how to use them. The natural solution became:

**Harnesses ship with filesystem abstractions and tools for fs-ops.**

The filesystem is arguably the most foundational harness primitive because of what it unlocks:

- Agents get a workspace to read data, code, and documentation.
- Work can be incrementally added and offloaded instead of holding everything in context. Agents can store intermediate outputs and maintain state that outlasts a single session.
- **The filesystem is a natural collaboration surface.** Multiple agents and humans can coordinate through shared files. Architectures like Agent Teams rely on this.

Git adds versioning to the filesystem so agents can track work, rollback errors, and branch experiments. We revisit the filesystem more below, because it turns out to be a key harness primitive for other features we need.

## Bash + Code as a General Purpose Tool

**We want agents to autonomously solve problems without humans needing to pre-design every tool.**

The main agent execution pattern today is a [ReAct loop](https://docs.langchain.com/oss/python/langchain/agents), where a model reasons, takes an action via a tool call, observes the result, and repeats in a while loop. But harnesses can only execute the tools they have logic for. Instead of forcing users to build tools for every possible action, a better solution is to give agents a general purpose tool like bash.

**Harnesses ship with a bash tool so models can solve problems autonomously by writing & executing code.**

Bash + code exec is a big step towards **giving models a computer** and letting them figure out the rest autonomously. The model can design its own tools on the fly via code instead of being constrained to a fixed set of pre-configured tools.

Harnesses still ship with other tools, but code execution has become the default general-purpose strategy for autonomous problem solving.

## Sandboxes and Tools to Execute & Verify Work

**Agents need an environment with the right defaults so they can safely act, observe results, and make progress.**

We've given models storage and the ability to execute code, but all of that needs to happen somewhere. Running agent-generated code locally is risky and a single local environment doesn’t scale to large agent workloads.

**Sandboxes give agents safe operating environments.** Instead of executing locally, the harness can connect to a sandbox to run code, inspect files, install dependencies, and complete tasks. This creates secure, isolated execution of code. For more security, harnesses can allow-list commands and enforce network isolation. Sandboxes also unlock scale because environments can be created on demand, fanned out across many tasks, and torn down when the work is done.

**Good environments come with good default tooling.** Harnesses are responsible for configuring tooling so agents can do useful work. This includes pre-installing language runtimes and packages, CLIs for git and testing, [browsers](https://github.com/vercel-labs/agent-browser) for web interaction and verification.

Tools like browsers, logs, screenshots, and test runners give agents a way to observe and analyze their work. This helps them create **self-verification loops where** they can **write application code,** run tests, inspect logs, and fix errors.

The model doesn’t configure its own execution environment out of the box. Deciding where the agent runs, what tools are available, what it can access, and how it verifies its work are all harness-level design decisions.

## Memory & Search for Continual Learning

**Agents should remember what they've seen and access information that didn't exist when they were trained.**

Models have no additional knowledge beyond their weights and what's in their current context. Without access to edit model weights, the only way to "add knowledge" is via **context injection.**

For memory, the filesystem is again a core primitive. Harnesses support memory file standards like [AGENTS.md](http://agents.md/) which get injected into context on agent start. As agents add and edit this file, harnesses load the updated file into context. This is a form of [continual learning](https://www.ibm.com/think/topics/continual-learning) where agents durably store knowledge from one session and inject that knowledge into future sessions.

Knowledge cutoffs mean that models can't directly access new data like updated library versions without the user providing them directly. For up-to-date knowledge, Web Search and MCP tools like [Context7](https://context7.com/) help agents access information beyond the knowledge cutoff like new library versions or current data that didn't exist when training stopped.

Web Search and tools for querying up-to-date context are useful primitives to bake into a harness.

## Battling Context Rot

**Agent performance shouldn’t degrade over the course of work.**

[Context Rot](https://research.trychroma.com/context-rot) \*\*\*\*describes how models become worse at reasoning and completing tasks as their context window fills up. Context is a precious and scarce resource, so harnesses need strategies to manage it.

**Harnesses today are largely delivery mechanisms for good context engineering.**

**Compaction** addresses what to do when the context window is close to filling up. Without compaction, what happens when a conversation exceeds the context window? One option is that the API errors, that’s not good. The harness has to use some strategy for this case. So compaction intelligently offloads and summarizes the existing context window so the agent can continue working.

**Tool call offloading** helps reduce the impact of large tool outputs that can noisily clutter the context window without providing useful information. The harness keeps the head and tail tokens of tool outputs above a threshold number of tokens and offloads the full output to the filesystem so the model can access it if needed.

**Skills** address the issue of too many tools or MCP servers loaded into context on agent start which degrades performance before the agent can start working. Skills are a harness level primitive that solve this via **progressive disclosure.** The model didn't choose to have Skill front-matter loaded into context on start but the harness can support this to protect the model against context rot.

## Long Horizon Autonomous Execution

**We want agents to complete complex work, autonomously, correctly, over long time horizons.**

Autonomous software creation is the holy grail for coding agents. But today's models suffer from early stopping, issues decomposing complex problems, and incoherence as work stretches across multiple context windows. A good harness has to design around all of this.

This is where the earlier harness primitives start to compound. Long-horizon work requires durable state, planning, observation, and verification to keep working across multiple context windows.

**Filesystems and git for tracking work across sessions.** Agents produce millions of tokens over a long task so the filesystem durably captures work to track progress over time. Adding git allows new agents to quickly get up to speed on the latest work and history of the project. For multiple agents working together, the filesystem also acts as a shared ledger of work where agents can collaborate.

**Ralph Loops for continuing work.** [The Ralph Loop](https://ghuntley.com/loop/) is a harness pattern that intercepts the model's exit attempt via a hook and reinjects the original prompt in a clean context window, forcing the agent to continue its work against a completion goal. The filesystem makes this possible because each iteration starts with fresh context but reads state from the previous iteration.

**Planning and self-verification to stay on track.** Planning is when a model decomposes a goal into a series of steps. Harnesses support this via good prompting and injecting reminders how to use a plan file in the filesystem. After completing each step, agents benefit from the checking correctness of their work via **self-verification.** Hooks in harnesses can run a pre-defined test suite and loop back to the model on failure with the error message or models can be prompted to self-evaluate their code independently. Verification grounds solution in tests and creates a feedback signal for self-improvement.

The Future of Harnesses

## The Coupling of Model Training and Harness Design

Today's agent products like Claude Code and Codex are post-trained with models and harnesses in the loop. This helps models improve at actions that the harness designers think they should be natively good at like filesystem operations, bash execution, planning, or parallelizing work with subagents.

This creates a feedback loop. Useful primitives are discovered, added to the harness, and then used when training the next generation of models. As this cycle repeats, models become more capable within the harness they were trained in.

But this co-evolution has interesting side effects for generalization. It shows up in ways like how changing tool logic leads to worse model performance. A good example is described [here in the Codex-5.3 prompting guide](https://developers.openai.com/cookbook/examples/gpt-5/codex_prompting_guide/#apply_patch) with the apply_patch tool logic for editing files. A truly intelligent model should have little trouble switching between patch methods, but training with a harness in the loop creates this overfitting.

**But this doesn't mean that the best harness for your task is the one a model was post-trained with.** [The Terminal Bench 2.0 Leaderboard](https://www.tbench.ai/leaderboard/terminal-bench/2.0) is a good example. Opus 4.6 in Claude Code scores far below Opus 4.6 in other harnesses. [In a previous blog](https://x.com/Vtrivedy10/status/2023805578561060992?s=20), we showed how we improved our coding agent Top 30 to Top 5 on Terminal Bench 2.0 by only changing the harness. There's a lot of juice to be squeezed out of optimizing the harness for your task.

## Where Harness Engineering is Going

As models get more capable, some of what lives in the harness today will get absorbed into the model. Models will get better at planning, self-verification, and long horizon coherence natively, thus requiring less context injection for example.

That suggests harnesses should matter less over time. But just as prompt engineering continues to be valuable today, it’s likely that harness engineering will continue to be useful for building good agents.

It’s true that harnesses today patch over model deficiencies, but they also engineer systems around model intelligence to make them more effective. A well-configured environment, the right tools, durable state, and verification loops make any model more efficient regardless of its base intelligence.

Harness engineering is a very active area of research that we use to improve our harness building library [deepagents](https://docs.langchain.com/oss/python/deepagents/overview) at LangChain. Here are a few open and interesting problems we’re exploring today:

- orchestrating hundreds of agents working in parallel on a shared codebase
- agents that analyze their own traces to identify and fix harness-level failure modes
- harnesses that dynamically assemble the right tools and context just-in-time for a given task instead of being pre-configured

This blog was an exercise in defining what a harness is and how it’s shaped by the work we want models to do.

**The model contains the intelligence and the harness is the system that makes that intelligence useful.**

To more harness building, better systems, and better agents.

---

https://code.claude.com/docs/en/interactive-mode#side-questions-with-btw

---

https://defuddle.md/doc.rust-lang.org/nomicon/print.html

---
