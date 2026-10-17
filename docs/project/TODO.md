Inspect code changes from the last 24 hours for high-confidence risks involving runtime failures, data loss, authorization bypasses, resource leaks, or cross-platform compatibility, and attach code and commit/diff evidence. Perform read-only analysis using only verifiable repository facts; state when evidence is insufficient, do not speculate, and do not modify code or external state. Also scan for large and monolith files and module and plan deduplication and refactor and extract reusable components.

---

Audit the codebase for oversized files, monolithic modules, duplicated logic, and tightly coupled components. Identify refactoring opportunities, extract reusable components, and improve modularity and separation of concerns. Prioritize by complexity, duplication, maintainability, and impact. Produce a phased refactoring plan with file references, proposed module boundaries, and risks. Follow KISS/DRY, preserve existing behavior and public APIs, and avoid unnecessary abstractions.

---

Inspect code changes from the last 24 hours for high-confidence risks involving runtime failures, data loss, authorization bypasses, resource leaks, or cross-platform compatibility, and attach code and commit/diff evidence. Perform read-only analysis using only verifiable repository facts; state when evidence is insufficient, do not speculate, and do not modify code or external state. Also scan for large and monolith files and module and plan deduplication and refactor and extract reusable components. Also check context management and proper resource cleanup. Check for wasted, redundant, or inefficient tools calls and suggest optimizations where applicable. Check recent sessions for any anomalies or unusual patterns that could indicate potential issues.

--

gui

--

computer use

---

```
Use [@OpenAI Developers](plugin://plugin_connector_1p_32dba5a7095c8191adca04ee30276304) and the official prompt-caching guide at [https://developers.openai.com/api/docs/guides/prompt-caching](https://developers.openai.com/api/docs/guides/prompt-caching).

Analyze the OpenAI API usage in my selected repository and suggest how to optimize prompt caching. If no repository is selected, ask me to choose one.

Inspect request construction, reusable prompt prefixes, conversation history, tool definitions, cache keys, caching modes, retention, and any available usage measurements. Apply only guidance supported by the models in use.

Give prioritized recommendations with file references, tradeoffs, and a plan to measure cache reads, writes, and input cost. Distinguish measured results from estimates. Do not change files or run paid API requests until I approve.
```

---

```
Review the prompts, model choices, and model settings in my selected repository. Suggest how to improve prompt quality and balance cost, intelligence, and latency for each task. If no repository is selected, ask me to choose one.

Apply current prompting best practices for the models in use. Suggest concrete prompt rewrites that clarify instructions, provide relevant context and examples, remove ambiguity, and define the desired output while preserving intended behavior.

Evaluate model choice and supported settings such as reasoning effort, output length, and verbosity. Recommend where a faster or less expensive configuration is sufficient and where stronger reasoning is worth the extra cost and latency.

Give prioritized recommendations with file references, before-and-after prompt examples, and tradeoffs. Propose a small evaluation using representative tasks to compare output quality, token cost, and response latency against the current setup. Distinguish measured results from estimates. Do not change files or run paid API requests until I approve.

Use [@OpenAI Developers](plugin://plugin_connector_1p_32dba5a7095c8191adca04ee30276304) and $openai-docs for current OpenAI API guidance and API key creation only if needed.
```

---

```
$openai-docs make a plan to migrate this project to the latest model family. suggest updates to the prompt to make it stronger for this latest model and suggest which model to migrate to based on my current model.
```

---

````
use Codex `Plugin Creator`, Help me build a plugin. Start by asking what it should do, then help me package it as a ZIP for upload to OpenAI Platform.
```
````

## Benchmarking TODO

- [ ] Run a real VT Code evaluation on SWE-bench Lite. Follow the
      [evaluation plan](../benchmarks/PLAN-VTCODE-SWEBENCH-LITE.md), pilot first, and submit reproducible
      results if eligible.

---

https://developers.openai.com/api/docs/guides/decisions-voice
