# Prompt-cache measurement

Use measurements to choose caching policies. Local wire tests establish request construction and accounting;
they do not establish live cache performance. No API call is needed to analyze existing trajectory files.

## Available observations

Interactive `prompt_cache_metrics` trajectory rows retain existing fields and add:

| Field | Meaning |
| --- | --- |
| `provider`, `model`, `requested_model` | Provider route, provider-returned route model, and original request model; separate fallback routes when comparing. |
| `input_tokens` | Normalized total input, including reads and writes. |
| `cache_read_tokens_reported`, `cache_write_tokens_reported` | Provider counters; `null` means unavailable, distinct from a reported zero. |
| `elapsed_ms` | End-to-end request duration, including retries; this is not time to first token. |
| `run_id`, `turn_id`, `turn` | Correlate foreground usage and clearing projections by run, user turn, and request step. |

Interactive `history_projection_metrics` rows describe local tool-result clearing: original and projected estimated
token counts, changed-message count, and the first changed message index (zero-based, `null` for no change).
The `trigger_tokens` field records the resolved clearing threshold for that projection.
They contain no prompt text. These counts use the existing local estimator, not provider tokenization. A changed
message identifies a local prefix change; it does not prove the size or cause of a server cache miss.
Headless clearing logs the first changed index alongside its existing projection estimates at debug level.

Completed native OpenAI auxiliary calls emit separate `prompt_cache_metrics` debug events under
`vtcode::llm::prompt_cache`, with `scope = "auxiliary"`, the requested cache profile, counters, and duration.
These events do not feed foreground cache health or advance comparison baselines. Inspect them with the existing
debug logger; the reporter also accepts structured tracing JSON exports whose event data is in a `fields` object.

The existing native OpenAI comparison diagnostics can identify model, tool, effort, or input differences. They are
separate from usage counters. Stable system/catalog hashes do not establish that the complete wire history matches.

## Read-only report

Run against explicitly selected files; do not combine duplicate exports of the same requests:

```sh
python3 scripts/analyze_prompt_cache.py .vtcode/logs/trajectory.jsonl
```

The JSON report groups by provider, model, and foreground/auxiliary scope and calculates weighted read/write fractions,
missing-counter counts,
median request duration, and local clearing estimates. Duplicate file paths are counted once. It makes no network
requests and prints no prompt or response content. Historical zero-filled counters are treated as unknown; historical
positive counters remain observations. Legacy prompt counts are listed separately and cannot produce normalized
fractions or a cost estimate. Use `--scope auxiliary` to isolate auxiliary tracing exports.

To estimate input cost, filter one exact provider/model route and supply all three applicable rates in USD per million
tokens. Obtain rates from [official pricing](https://developers.openai.com/api/docs/pricing); confirm the endpoint,
service tier, region, and context-length premiums. The report does not fetch prices or infer those settings.

```sh
python3 scripts/analyze_prompt_cache.py .vtcode/logs/trajectory.jsonl \
  --provider openai --model gpt-6-luna \
  --input-rate 10 --read-rate 1 --write-rate 12.5
```

The example rates are illustrative, not a current model price. Input cost is:

```text
ordinary = input_tokens - cache_read_tokens - cache_write_tokens
estimated_input_cost = (ordinary * input_rate + reads * read_rate + writes * write_rate) / 1_000_000
```

Writes replace ordinary-input charges; they are not an additive charge on the same tokens. Missing counters leave
cost unknown. Output tokens, tool fees, unreported failed attempts, and prewarming are outside this input-only report.
Provider billing remains the authority for actual spend.

## Compare policies after paid testing is authorized

1. Keep endpoint, exact model, service tier, reasoning effort, tool definitions/order, and task inputs fixed.
2. Use separate output files for each policy. Report cold starts separately from warm requests and compare the same
   number of turns. Include more than one conversation; one warm run cannot establish a general improvement.
3. Compare clearing thresholds using `history_projection_metrics`, total input, reads, writes, estimated cost, duration,
   and task success. Do not disable clearing simply to raise hit rate: retaining history increases context volume.
4. For native GPT-5.6/GPT-6 auxiliary calls, try `prompt_cache.providers.openai.auxiliary_explicit_cache = true`.
   It keeps only the stable instruction breakpoint and selects explicit-only caching. The default remains `false`.
   Compare disposable-suffix write savings against reduced suffix reuse on retries. Other backends and interactive
   requests retain their current policy. Prefixes still need the model's minimum cacheable length; do not add padding.
5. Use comparison diagnostics to investigate differences, then repeat. Label reported usage as measured and all
   price-derived costs as estimates. Record sample size, missing telemetry, idle gaps, and quality results.

## Retention and deferred optimizations

Native GPT-5.6/GPT-6 requests use `prompt_cache_options.ttl: "30m"`. Model-aware gap advisories use 30 minutes even
if legacy 24h retention is configured; legacy settings do not extend the modern TTL. Older supported routes retain
their configured policy; explicit legacy 24h retention uses a cautious 30-minute advisory because 24 hours is a maximum,
not a guarantee. Explicit warning thresholds remain overrides. A warning expresses possible expiry, not proof.
OpenAI-compatible services may implement different cache semantics; evaluate them with their own documentation.

Session cache keys and deterministic tool ordering remain unchanged. GPT-5.6+ keys separate accounting; older models
also use them for routing. Changing key scope requires a separate accounting/isolation decision, not an assumed speedup.

Do not introduce automatic prewarming without a paid latency/cost experiment: prewarm requests are billed.
Do not automatically inject reasoning `configuration_update` items: GPT-6 supports them only in standard single-agent
mode, and they conflict with automatic compaction/truncation and standalone `/responses/compact`, which VT Code uses.
Effort remains request-level. See the [reasoning guide](https://developers.openai.com/api/docs/guides/reasoning#change-reasoning-mid-conversation).

Source guidance: [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching) and
[cache diagnostics](https://developers.openai.com/api/docs/guides/prompt-caching/diagnostics).
