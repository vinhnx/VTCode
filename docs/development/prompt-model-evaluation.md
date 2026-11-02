# Prompt and Model Evaluation

The initial dataset is [prompt_model_evaluation.json](../../tests/fixtures/prompt_model_evaluation.json).
It contains twelve supplied-context cases covering rewriting, continuity summaries, diagnosis, injection detection,
routine work, and difficult reasoning. It is an evaluation dataset, not an executable `vtcode eval` suite: helper
cases must be sent to their owning prompt, while agent cases need an isolated workspace and their normal tools.

No paid evaluation has been run. Local regressions verify request construction and runtime contracts; they do not
establish model quality, latency improvements, or dollar savings.

## Comparison procedure

1. Freeze the baseline commit and candidate diff, provider route, model ID, reasoning effort, verbosity, output limit,
   tool catalog, and supplied context. Record the resolved settings rather than configuration labels alone.
2. Compare baseline prompts with candidate prompts using the same model and settings. Use two repetitions per case
   and variant: twelve cases × two variants × two repetitions = 48 task runs, potentially more than 48 API requests.
3. After that comparison, hold the winning prompt fixed and compare lower effort on the current GLM route.
   Preserve supported levels: its repository catalog lists low, high, and max; do not invent medium.
4. Separately compare task-appropriate models. Candidate routes already in the catalog include
   `openai/gpt-6-luna` for bounded helpers, `openai/gpt-6.1-sol` for complex coding, and `openai/gpt-6-astra` for
   particularly difficult cases. These are candidates, not proven improvements over GLM.
5. Run agent tasks in isolated workspaces with identical fixtures and command policy. Reset each workspace between
   variants. Do not grant permissions or change approval behavior for evaluation convenience.

Paid runs require explicit approval of provider routes, a dollar ceiling, maximum requests, and maximum wall time.
Stop at the first exhausted ceiling. Pin exact model versions when available; otherwise record the alias and run date.
No evaluation runner should be enabled automatically by these fixtures.

## Outcomes and measurements

| Measure | Method |
| --- | --- |
| Correctness | Check required and forbidden outcomes; inspect actual patches and checker results for coding tasks |
| Instruction preservation | Reject lost negations, changed paths, broadened authorization, or invented requirements |
| Safety | Any unauthorized allow or accepted truncated safety verdict fails the candidate regardless of average score |
| Summary continuity | Preserve pending sessions, polling instructions, approvals, and verification freshness |
| Structured output | Count parse failures, incomplete outputs, and retry/fallback calls |
| Latency | Record monotonic time to first visible token and task completion; include retries and tool execution |
| Tokens | Record input, cached input, cache writes where reported, output, and reasoning token details |
| Cost | Apply route-specific rates to normalized usage; include all retries and auxiliary calls |

Do not add reasoning tokens twice when they are already included in reported output usage. Missing usage or unknown
gateway rates mean unknown cost, not zero. Record published-price estimates separately from billed charges. Direct
OpenAI rates do not establish Merge Gateway prices.

Interleave baseline and candidate runs to reduce time-of-day bias. Report cold and warm cache results separately;
keep prefixes and tool definitions stable within each variant. Report median latency and individual failures. Two
repetitions are a smoke comparison, not evidence for reliable tail latency or statistical superiority. Use blind
human review for open-ended cases and deterministic checks for verdicts, IDs, and generated code where practical.

## Settings and tradeoffs

Keep the main model and reasoning configuration unchanged until comparisons justify a change. Use low reasoning on
advertised auxiliary routes; short helper calls reserve additional output allowance for reasoning. Larger allowances
are ceilings, not measured consumption, and do not guarantee that a model finishes before the existing timeout.
Permission checks remain conservative and retain their normal fallback behavior.

Interactive requests retain `agent.verbosity`; wire serialization stays provider-specific. Native OpenAI Responses
supports text verbosity on eligible models. The native Merge Gateway adapter currently does not serialize a text
verbosity field, so concise output requirements remain in the compiled prompt. Do not send an unverified gateway
parameter merely because the upstream OpenAI API supports it.

## References

- [OpenAI prompt engineering](https://developers.openai.com/api/docs/guides/prompt-engineering)
- [OpenAI reasoning guidance](https://developers.openai.com/api/docs/guides/reasoning)
- [OpenAI evaluation best practices](https://developers.openai.com/api/docs/guides/evaluation-best-practices)
- [Model profiles](model-profiles.md)
- [Runtime guidance](runtime-guidance.md)
