# Z.AI Quick Reference

| Setting           | Value                              |
| ----------------- | ---------------------------------- |
| Provider          | `zai`                              |
| API key           | `ZAI_API_KEY`                      |
| Default endpoint  | `https://api.z.ai/api`             |
| Endpoint override | `ZAI_BASE_URL`                     |
| Default model     | `glm-5.3`                          |
| Context           | 1,000,000 tokens (Flash & 5.3/5.2) |

```bash
export ZAI_API_KEY="..."
vtcode --provider zai --model glm-5.3-flash ask "Recreate this UI from screenshots using Next.js"
```

Official Z.AI models:

- `glm-5.3` — flagship coding model, 1M context
- `glm-5.3-flash` — efficient multimodal (320B/18B, hybrid attention), 1M context, native vision (image_url)
- `glm-5.3-flashx` — high-speed Flash variant (up to 200 tok/s), 1M context, native vision
- `glm-5.2` — flagship long-horizon, 1M context

Recommended Flash settings: `temperature: 1`, `top_p: 0.95`, `reasoning_effort: max` (native; `low`/`high` also
supported, `xhigh` aliases to `max`), `thinking.type: enabled` (cannot be disabled), `tool_stream: true` with streaming.

See the [full Z.AI provider guide](./zai.md) and [GLM-5.3 Flash guide](https://docs.z.ai/guides/vlm/glm-5.3-flash).
