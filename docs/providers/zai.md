# Z.AI Provider Guide

VT Code has a first-class Z.AI provider for GLM models. This guide covers setup, curated models, and the **GLM-5.3
Flash** multimodal model plus its high-speed **GLM-5.3 FlashX** variant.

## Prerequisites

Create a Z.AI API key from the [Z.AI Platform](https://z.ai/docs) and the
[Z.AI API docs](https://docs.z.ai/guides/llm/glm-5).

```bash
export ZAI_API_KEY="your-zai-api-key"
```

The endpoint defaults to `https://api.z.ai/api`. Set `ZAI_BASE_URL` when a compatible gateway or test endpoint is
required.

## Quickstart

Use the Z.AI provider with its default model:

```bash
vtcode --provider zai --model glm-5.3 chat
```

The equivalent persistent configuration is:

```toml
[agent]
provider = "zai"
default_model = "glm-5.3"
```

For the new Flash model:

```bash
vtcode --provider zai --model glm-5.3-flash chat
# explicit Flash with recommended effort
vtcode --provider zai --model glm-5.3-flash ask "Build a Next.js page from this screenshot"
```

`vtcode.toml` for Flash:

```toml
[agent]
provider = "zai"
default_model = "glm-5.3-flash"
```

## Model API — GLM-5.3 Flash

- **Model code:** `glm-5.3-flash` (canonical VT Code id `glm-5.3-flash`, `ModelId::ZaiGlm53Flash`)
- **API docs:** [Chat Completion API](https://docs.z.ai/api-reference/introduction)
- **Context:** 1M tokens (`context: 1_000_000`, `max_output_tokens: 128000`)
- **Architecture:** 320B total / 18B activated hybrid sparse + linear attention (first GLM-5 frontier model with this
  architecture). Reduces attention compute ~3.01× and KV cache ~4.44× vs GLM-5.3.
- **Modalities:** Native multimodal — text + image input, text output. First native multimodal model in the GLM-5
  series.
- **Capabilities:** Thinking (always-on), streaming, function calling, context caching, structured output, tool
  streaming, vision

### Image parameters

Add a content block with `type: image_url` to `messages[].content[]` and pass an image URL (recommended) or a Base64
Data URL through `image_url.url`. Multiple images are supported by including multiple `image_url` blocks.

### Recommended settings

From [Z.AI GLM-5.3 Flash guide](https://docs.z.ai/guides/vlm/glm-5.3-flash):

- `temperature: 1`, `top_p: 0.95`, `reasoning_effort: max` (VT Code sends native `max`; `xhigh` is accepted as an alias
  for `max`)
- `thinking.type` only supports `enabled` (thinking cannot be disabled); we recommend `thinking.clear_thinking: false`
- For streaming, enable both `stream: true` and `tool_stream: true` (VT Code does this by default)

In VT Code, reasoning effort for GLM models is exposed as:

| Effort | Notes                                                            |
| ------ | ---------------------------------------------------------------- |
| `low`  | Fast, light reasoning                                            |
| `high` | Deep thinking                                                    |
| `max`  | Maximum deep reasoning (provider default, recommended for Flash) |

Set via `vtcode.toml` or the `/model` picker:

```toml
[agent]
provider = "zai"
default_model = "glm-5.3-flash"
reasoning_effort = "max"
```

## Curated models

| Model            |   Context | Notes                                                                                                    |
| ---------------- | --------: | -------------------------------------------------------------------------------------------------------- |
| `glm-5.3`        | 1,000,000 | Default flagship coding model, frontier long-horizon agentic performance                                 |
| `glm-5.3-flash`  | 1,000,000 | Efficient multimodal model, hybrid sparse+linear attention, native vision, vision-in-the-loop coding     |
| `glm-5.3-flashx` | 1,000,000 | High-speed Flash variant, faster inference (up to 200 tok/s), same Flash multimodal stack, native vision |
| `glm-5.2`        | 1,000,000 | Flagship for long-horizon tasks with truly usable 1M context                                             |

All three models appear in the Z.AI section of the `/model` picker and support streaming, function tools, structured
output, and reasoning effort controls through Z.AI's OpenAI-compatible Chat Completions API.

## Vision-driven workflows (Flash)

GLM-5.3 Flash natively integrates visual capabilities into the coding loop — it can observe interfaces, rendered
results, and interaction feedback and iteratively improve. Use cases highlighted by Z.AI include:

- **Vision-driven UI coding:** Transform screenshots, multi-page images, URLs, or screen recordings into polished
  Next.js / frontend apps. Analyze design system, page relationships, shared components, navigation, interaction states,
  and animation logic, then build and iteratively refine against rendered screenshots.
- **Office deliverables:** Generate PPTX/PDF/DOCX/XLSX from research with visual validation (overflow, misalignment,
  overlapping, cropping).
- **Financial research:** Multi-source research → valuation model → report with citations and auditable Excel.
- **Video understanding & editing:** Inventory footage, identify speakers/events, structure story, generate subtitles,
  and verify A/V sync.
- **3D / Blender, game dev (Godot), computer use, CAD (build123d):** Build → fixed-camera render → inspect → refine
  loops with visual feedback.

All of these benefit from Flash's 1M context and reduced serving cost.

## GLM Coding Plan

GLM-5.3 Flash is fully available on the [GLM Coding Plan](https://z.ai/subscribe) with 3× the quota of GLM-5.3. Off-peak
(including all day on weekends) consumes 50% of standard points. See
[Personal Plan](https://z.ai/subscribe?plantype=individual) and [Team Plan](https://z.ai/subscribe?plantype=team).

## Key advancements (summary)

- Competitive intelligence at flash cost: pushes the Pareto frontier on Artificial Analysis Intelligence Index v4.1.1
  (score 57 at $0.045/task discounted); on DeepSWE v1.1 63.4 vs 46.2 (GLM-5.2), AutomationBench 48.8 vs 26.2, and near
  Claude Opus 4.8 on Z.ai Code Bench v1.0 (29.0 vs 29.5 at max effort).
- Hybrid architecture with Manifold-Constrained Hyper-Connections (mHC) and 30T-token multimodal pre-training; IndexPool
  compression for the sparse indexer at 1M context.

See the [announcement blog](https://z.ai/blog/glm-5.3-flash) for full evaluation details.

## Troubleshooting

| Symptom                 | Resolution                                                                                                       |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Missing credentials     | Set `ZAI_API_KEY`.                                                                                               |
| Model rejected          | Use one of `glm-5.3`, `glm-5.3-flash`, `glm-5.3-flashx`, `glm-5.2` for the native Z.AI provider.                 |
| Vision not working      | Use `glm-5.3-flash` or `glm-5.3-flashx` and send images as `image_url` blocks; other GLM-5 models are text-only. |
| Custom endpoint failure | Confirm the endpoint implements `https://api.z.ai/api` Chat Completions protocol and set `ZAI_BASE_URL`.         |
| Reasoning not disabling | Flash only supports `thinking.type: enabled`; this is expected.                                                  |

## References

- [GLM-5.3 Flash guide](https://docs.z.ai/guides/vlm/glm-5.3-flash)
- [GLM-5 docs](https://docs.z.ai/guides/llm/glm-5)
- [Z.AI docs index](https://docs.z.ai/llms.txt)
- [Z.AI Platform](https://z.ai/docs)
