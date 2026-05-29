---
# Compaction

Server-side context compaction for managing long conversations that approach context window limits.

---

<Note>
This feature is eligible for [Zero Data Retention (ZDR)](/docs/en/build-with-claude/api-and-data-retention). When your organization has a ZDR arrangement, data sent through this feature is not stored after the API response is returned.
</Note>

<Warning>
**Compaction is in beta.** Include the [beta header](/docs/en/api/beta-headers) `compact-2026-01-12` in all API requests to use this feature. Beta features may have API surface changes in future releases.
</Warning>

<Tip>
Server-side compaction is the recommended strategy for managing context in long-running conversations and agentic workflows. It handles context management automatically with minimal integration work.
</Tip>

Compaction extends the effective context length for long-running conversations and tasks by automatically summarizing older content when approaching the context window limit. This isn't just about staying under a token cap — as conversations grow longer, model focus degrades across the full history. Compaction keeps the active context focused and performant by replacing stale content with concise summaries.

<Tip>
For a deeper look at why long contexts degrade and how compaction helps, see [Effective context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents).
</Tip>

This is ideal for:

- **Long-lived chat sessions** — multi-turn conversations where users stay in a single chat for extended periods
- **Task-oriented agent loops** — prompts requiring extensive follow-up work (often tool use) that may exceed the context window

## Supported models

| Model | ID |
|:------|:----|
| Claude Mythos Preview | `claude-mythos-preview` |
| Claude Opus 4.8 | `claude-opus-4-8` |
| Claude Opus 4.7 | `claude-opus-4-7` |
| Claude Opus 4.6 | `claude-opus-4-6` |
| Claude Sonnet 4.6 | `claude-sonnet-4-6` |

## How compaction works

When compaction is enabled and input tokens exceed the configured trigger threshold, Claude automatically:

1. **Detects** when input tokens (including system prompt, messages, and tool definitions) exceed the configured trigger threshold.
2. **Generates a summary** of the current conversation history using the same model as your request.
3. **Replaces** all message content before the compaction block with the generated summary. Only the compaction block and messages after it are retained.
4. **Continues** the response with the compacted context.

On subsequent requests, append the full response to your messages. The API automatically drops all message blocks prior to the `compaction` block, continuing the conversation from the summary. If multiple compactions occur across a long conversation, only the most recent compaction block is used; all prior content is discarded.

**Key behaviors:**

- The **system prompt is not summarized** — it is re-sent verbatim on every request and preserved separately from conversation history.
- **Tool definitions** are also counted toward the trigger threshold but are preserved verbatim after compaction.
- **Multiple compactions** may occur within a single request if tool use (dynamic tool calls) generates enough output to trigger compaction again.
- The trigger threshold is measured **before** sending the request. If you set `trigger.value` below the actual input tokens, compaction fires immediately.

![Flow diagram showing the compaction process: when input tokens exceed the trigger threshold, Claude generates a summary in a compaction block and continues the response with the compacted context](/docs/images/compaction-flow.svg)

## Basic usage

Enable compaction by adding the `compact_20260112` strategy to `context_management.edits` in your Messages API request.

<Warning>
**Tool-use warning:** When your request includes `tools`, compaction may fail silently — the model occasionally calls a tool during the internal summarization step instead of writing a summary (see [Custom summarization instructions](#custom-summarization-instructions) for mitigation).
</Warning>

<CodeGroup>
```bash cURL
curl https://api.anthropic.com/v1/messages \
     --header "x-api-key: $ANTHROPIC_API_KEY" \
     --header "anthropic-version: 2023-06-01" \
     --header "anthropic-beta: compact-2026-01-12" \
     --header "content-type: application/json" \
     --data \
'{
    "model": "claude-opus-4-8",
    "max_tokens": 4096,
    "messages": [
        {
            "role": "user",
            "content": "Help me build a website"
        }
    ],
    "context_management": {
        "edits": [
            {
                "type": "compact_20260112"
            }
        ]
    }
}'
```

```python Python
import anthropic

client = anthropic.Anthropic()

messages = [{"role": "user", "content": "Help me build a website"}]

response = client.beta.messages.create(
    betas=["compact-2026-01-12"],
    model="claude-opus-4-8",
    max_tokens=4096,
    messages=messages,
    context_management={"edits": [{"type": "compact_20260112"}]},
)

# Append the response (including any compaction block) to continue the conversation
messages.append({"role": "assistant", "content": response.content})
```

```typescript TypeScript
import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic();

const messages: Anthropic.Beta.Messages.BetaMessageParam[] = [
  { role: "user", content: "Help me build a website" }
];

const response = await client.beta.messages.create({
  betas: ["compact-2026-01-12"],
  model: "claude-opus-4-8",
  max_tokens: 4096,
  messages,
  context_management: {
    edits: [
      {
        type: "compact_20260112"
      }
    ]
  }
});

// Append the response (including any compaction block) to continue the conversation
messages.push({
  role: "assistant",
  content: response.content
});
```

```csharp C#
using System;
using System.Collections.Generic;
using System.Threading.Tasks;
using Anthropic;
using Anthropic.Models.Beta.Messages;

class Program
{
    static async Task Main(string[] args)
    {
        AnthropicClient client = new();

        var messages = new List<BetaMessageParam>
        {
            new() { Role = Role.User, Content = "Help me build a website" }
        };

        var parameters = new MessageCreateParams
        {
            Betas = ["compact-2026-01-12"],
            Model = "claude-opus-4-8",
            MaxTokens = 4096,
            Messages = messages,
            ContextManagement = new BetaContextManagementConfig
            {
                Edits = [new BetaCompact20260112Edit()]
            }
        };

        var response = await client.Beta.Messages.Create(parameters);

        // Append the response (including any compaction block) to continue the conversation
        messages.Add(new BetaMessageParam
        {
            Role = Role.Assistant,
            Content = response.Content
        });
    }
}
```

```go Go
package main

import (
	"context"
	"fmt"
	"log"

	"github.com/anthropics/anthropic-sdk-go"
)

func main() {
	client := anthropic.NewClient()

	messages := []anthropic.BetaMessageParam{
		anthropic.NewBetaUserMessage(anthropic.NewBetaTextBlock("Help me build a website")),
	}

	response, err := client.Beta.Messages.New(context.TODO(), anthropic.BetaMessageNewParams{
		Model:     anthropic.ModelClaudeOpus4_8,
		MaxTokens: 4096,
		Messages:  messages,
		ContextManagement: anthropic.BetaContextManagementConfigParam{
			Edits: []anthropic.BetaContextManagementConfigEditUnionParam{
				{OfCompact20260112: &anthropic.BetaCompact20260112EditParam{}},
			},
		},
		Betas: []anthropic.AnthropicBeta{"compact-2026-01-12"},
	})
	if err != nil {
		log.Fatal(err)
	}

	// Append the response (including any compaction block) to continue the conversation
	messages = append(messages, response.ToParam())

	fmt.Println(response)
}
```

```java Java
import com.anthropic.client.AnthropicClient;
import com.anthropic.client.okhttp.AnthropicOkHttpClient;
import com.anthropic.models.beta.messages.MessageCreateParams;
import com.anthropic.models.beta.messages.BetaMessage;
import com.anthropic.models.beta.messages.BetaContextManagementConfig;
import com.anthropic.models.beta.messages.BetaCompact20260112Edit;

public class CompactionExample {
    public static void main(String[] args) {
        AnthropicClient client = AnthropicOkHttpClient.fromEnv();

        MessageCreateParams params = MessageCreateParams.builder()
            .addBeta("compact-2026-01-12")
            .model("claude-opus-4-8")
            .maxTokens(4096L)
            .addUserMessage("Help me build a website")
            .contextManagement(BetaContextManagementConfig.builder()
                .addEdit(BetaCompact20260112Edit.builder().build())
                .build())
            .build();

        BetaMessage response = client.beta().messages().create(params);

        // Append the response (including any compaction block) to continue the conversation
        // by including it in the next request's messages
        System.out.println(response);
    }
}
```

```php PHP
<?php

use Anthropic\Client;

$client = new Client(apiKey: getenv("ANTHROPIC_API_KEY"));

$messages = [
    ['role' => 'user', 'content' => 'Help me build a website']
];

$response = $client->beta->messages->create(
    maxTokens: 4096,
    messages: $messages,
    model: 'claude-opus-4-8',
    betas: ['compact-2026-01-12'],
    contextManagement: [
        'edits' => [
            ['type' => 'compact_20260112']
        ]
    ]
);

// Append the response (including any compaction block) to continue the conversation
$messages[] = ['role' => 'assistant', 'content' => $response->content];
```

```ruby Ruby
require "anthropic"

client = Anthropic::Client.new

messages = [
  { role: "user", content: "Help me build a website" }
]

response = client.beta.messages.create(
  betas: ["compact-2026-01-12"],
  model: "claude-opus-4-8",
  max_tokens: 4096,
  messages: messages,
  context_management: {
    edits: [{ type: "compact_20260112" }]
  }
)

# Append the response (including any compaction block) to continue the conversation
messages << { role: "assistant", content: response.content }
```
</CodeGroup>

## Parameters

| Parameter | Type | Default | Description |
|:----------|:-----|:--------|:------------|
| `type` | string | Required | Must be `"compact_20260112"` |
| `trigger` | object | `{"type": "input_tokens", "value": 150000}` | When to trigger compaction. The `value` field accepts an integer ≥ 50,000 tokens. No hard upper bound exists, but values should be ≤ the model's context window to leave room for the response. |
| `pause_after_compaction` | boolean | `false` | Whether to pause (return early with `stop_reason: "compaction"`) after generating the compaction summary, before producing the response text |
| `instructions` | string | `null` | Custom summarization prompt. Completely replaces the default prompt when provided. See [Custom summarization instructions](#custom-summarization-instructions). |

### Response

When compaction is enabled, the response may include:

| Field | Value | Description |
|:------|:------|:------------|
| `stop_reason` | `"compaction"` | Returned when `pause_after_compaction: true` and compaction triggers. The response contains only the compaction block (no text blocks). |
| `stop_reason` | `"end_turn"` | Normal response. Compaction may or may not have occurred. Check `content` for a compaction block. |
| `content[0].type` | `"compaction"` | The compaction summary block. Present at the start of the content array when compaction triggered. |
| `usage.iterations` | array | Only populated when compaction triggers. Shows per-iteration token usage. |

### Trigger configuration

Configure when compaction triggers using the `trigger` parameter. The trigger value is compared against the total input token count (system prompt + messages + tool definitions). Set it to 50–75% of your model's context window to leave room for the response and compaction overhead.

<CodeGroup>
```python Python
import anthropic

client = anthropic.Anthropic()
messages = [{"role": "user", "content": "Hello, Claude"}]
response = client.beta.messages.create(
    betas=["compact-2026-01-12"],
    model="claude-opus-4-8",
    max_tokens=4096,
    messages=messages,
    context_management={
        "edits": [
            {
                "type": "compact_20260112",
                "trigger": {"type": "input_tokens", "value": 150000},
            }
        ]
    },
)
```

```typescript TypeScript
import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic();
const messages: Anthropic.Beta.Messages.BetaMessageParam[] = [];

const response = await client.beta.messages.create({
  betas: ["compact-2026-01-12"],
  model: "claude-opus-4-8",
  max_tokens: 4096,
  messages,
  context_management: {
    edits: [
      {
        type: "compact_20260112",
        trigger: {
          type: "input_tokens",
          value: 150000
        }
      }
    ]
  }
});
```

```go Go
package main

import (
	"context"
	"fmt"
	"log"

	"github.com/anthropics/anthropic-sdk-go"
)

func main() {
	client := anthropic.NewClient()
	messages := []anthropic.BetaMessageParam{anthropic.NewBetaUserMessage(anthropic.NewBetaTextBlock("Hello, Claude"))}

	response, err := client.Beta.Messages.New(context.TODO(), anthropic.BetaMessageNewParams{
		Model:     anthropic.ModelClaudeOpus4_8,
		MaxTokens: 4096,
		Messages:  messages,
		ContextManagement: anthropic.BetaContextManagementConfigParam{
			Edits: []anthropic.BetaContextManagementConfigEditUnionParam{
				{OfCompact20260112: &anthropic.BetaCompact20260112EditParam{
					Trigger: anthropic.BetaInputTokensTriggerParam{Value: 150000},
				}},
			},
		},
		Betas: []anthropic.AnthropicBeta{"compact-2026-01-12"},
	})
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(response)
}
```

```java Java
import com.anthropic.client.AnthropicClient;
import com.anthropic.client.okhttp.AnthropicOkHttpClient;
import com.anthropic.models.beta.messages.MessageCreateParams;
import com.anthropic.models.beta.messages.BetaMessage;
import com.anthropic.models.beta.messages.BetaContextManagementConfig;
import com.anthropic.models.beta.messages.BetaCompact20260112Edit;
import com.anthropic.models.beta.messages.BetaInputTokensTrigger;

public class CompactionExample {
    public static void main(String[] args) {
        AnthropicClient client = AnthropicOkHttpClient.fromEnv();

        MessageCreateParams params = MessageCreateParams.builder()
            .model("claude-opus-4-8")
            .maxTokens(4096L)
            .addBeta("compact-2026-01-12")
            .addUserMessage("Hello, Claude")
            .contextManagement(BetaContextManagementConfig.builder()
                .addEdit(BetaCompact20260112Edit.builder()
                    .trigger(BetaInputTokensTrigger.builder()
                        .value(150000L)
                        .build())
                    .build())
                .build())
            .build();

        BetaMessage response = client.beta().messages().create(params);
        System.out.println(response);
    }
}
```
</CodeGroup>

**Choosing a trigger threshold:**

- **Default (150,000):** Works well for most use cases with 200K context window models.
- **Lower values (50,000–100,000):** Useful for early compaction in predictable-length tasks. Set lower when tool outputs are large.
- **Higher values (150,000+):** Maximizes context retention before summarizing. Ensure you leave room for the response (at least `max_tokens` + 10% overhead).
- **General rule:** Set trigger at 50–75% of the model's context window size. For a 200K model, 100K–150K is a reasonable range.

### Custom summarization instructions

By default, compaction uses the following summarization prompt:

```text
You have written a partial transcript for the initial task above. Please write a summary
of the transcript. The purpose of this summary is to provide continuity so you can continue
to make progress towards solving the task in a future context, where the raw history above
may not be accessible and will be replaced with this summary. Write down anything that would
be helpful, including the state, next steps, learnings etc. You must wrap your summary in a
<summary></summary> block.
```

<Note>
The default prompt has access to the full context window (system prompt + message history). "The initial task above" refers to the content visible in that window at compaction time.
</Note>

You can provide custom instructions via the `instructions` parameter. These completely replace the default prompt — they do not supplement it:

<CodeGroup>
```python Python
import anthropic

client = anthropic.Anthropic()
messages = [{"role": "user", "content": "Hello, Claude"}]
response = client.beta.messages.create(
    betas=["compact-2026-01-12"],
    model="claude-opus-4-8",
    max_tokens=4096,
    messages=messages,
    context_management={
        "edits": [
            {
                "type": "compact_20260112",
                "instructions": "Focus on preserving code snippets, variable names, and technical decisions.",
            }
        ]
    },
)
```

```typescript TypeScript
import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic();
const messages: Anthropic.Beta.Messages.BetaMessageParam[] = [];

const response = await client.beta.messages.create({
  betas: ["compact-2026-01-12"],
  model: "claude-opus-4-8",
  max_tokens: 4096,
  messages,
  context_management: {
    edits: [
      {
        type: "compact_20260112",
        instructions:
          "Focus on preserving code snippets, variable names, and technical decisions."
      }
    ]
  }
});
```

```go Go
package main

import (
	"context"
	"fmt"
	"log"

	"github.com/anthropics/anthropic-sdk-go"
)

func main() {
	client := anthropic.NewClient()

	response, err := client.Beta.Messages.New(context.TODO(), anthropic.BetaMessageNewParams{
		Model:     anthropic.ModelClaudeOpus4_8,
		MaxTokens: 4096,
		Messages: []anthropic.BetaMessageParam{
			anthropic.NewBetaUserMessage(anthropic.NewBetaTextBlock("Help me build a Python web scraper")),
			{Role: anthropic.BetaMessageParamRoleAssistant, Content: []anthropic.BetaContentBlockParamUnion{anthropic.NewBetaTextBlock("I'll help you build a web scraper...")}},
			anthropic.NewBetaUserMessage(anthropic.NewBetaTextBlock("Add support for JavaScript-rendered pages")),
		},
		ContextManagement: anthropic.BetaContextManagementConfigParam{
			Edits: []anthropic.BetaContextManagementConfigEditUnionParam{
				{OfCompact20260112: &anthropic.BetaCompact20260112EditParam{
					Instructions: anthropic.String("Focus on preserving code snippets, variable names, and technical decisions."),
				}},
			},
		},
		Betas: []anthropic.AnthropicBeta{"compact-2026-01-12"},
	})
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(response)
}
```

```java Java
import com.anthropic.client.AnthropicClient;
import com.anthropic.client.okhttp.AnthropicOkHttpClient;
import com.anthropic.models.beta.messages.MessageCreateParams;
import com.anthropic.models.beta.messages.BetaMessage;
import com.anthropic.models.beta.messages.BetaContextManagementConfig;
import com.anthropic.models.beta.messages.BetaCompact20260112Edit;

public class CompactionExample {
    public static void main(String[] args) {
        AnthropicClient client = AnthropicOkHttpClient.fromEnv();

        MessageCreateParams params = MessageCreateParams.builder()
            .addBeta("compact-2026-01-12")
            .model("claude-opus-4-8")
            .maxTokens(4096L)
            .addUserMessage("Help me build a Python web scraper")
            .addAssistantMessage("I'll help you build a web scraper...")
            .addUserMessage("Add support for JavaScript-rendered pages")
            .contextManagement(BetaContextManagementConfig.builder()
                .addEdit(BetaCompact20260112Edit.builder()
                    .instructions("Focus on preserving code snippets, variable names, and technical decisions.")
                    .build())
                .build())
            .build();

        BetaMessage response = client.beta().messages().create(params);
        System.out.println(response);
    }
}
```
</CodeGroup>

**Recommended for tool-use scenarios:** When your request includes `tools`, add explicit instructions to prevent the model from calling tools during summarization:

```text
Summarize the transcript inside <summary></summary> tags. Include relevant information
for continuing the task in the next context window. Do not call any tools while writing
this summary; respond with text only.
```

### Pausing after compaction

Use `pause_after_compaction` to return early after generating the compaction summary, before the response text. This gives you the opportunity to insert additional content (such as recent messages or instruction updates) after the compaction block.

**How it works:** When `pause_after_compaction: true` and compaction triggers, the API returns early with `stop_reason: "compaction"`. The response contains only the compaction block. You must then make a second API call with:
1. The compaction block as an assistant message
2. Any additional messages you want to preserve verbatim
3. The model then generates its text response using the compacted context plus your preserved messages

<CodeGroup>
```python Python
import anthropic

client = anthropic.Anthropic()
messages = [{"role": "user", "content": "Hello, Claude"}]
response = client.beta.messages.create(
    betas=["compact-2026-01-12"],
    model="claude-opus-4-8",
    max_tokens=4096,
    messages=messages,
    context_management={
        "edits": [{"type": "compact_20260112", "pause_after_compaction": True}]
    },
)

# Check if compaction triggered a pause
if response.stop_reason == "compaction":
    # Response contains only the compaction block
    messages.append({"role": "assistant", "content": response.content})

    # Continue the request — model will generate text response now
    response = client.beta.messages.create(
        betas=["compact-2026-01-12"],
        model="claude-opus-4-8",
        max_tokens=4096,
        messages=messages,
        context_management={"edits": [{"type": "compact_20260112"}]},
    )
```

```typescript TypeScript
import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic();
const messages: Anthropic.Beta.Messages.BetaMessageParam[] = [
  { role: "user", content: "Hello, Claude" }
];

let response = await client.beta.messages.create({
  betas: ["compact-2026-01-12"],
  model: "claude-opus-4-8",
  max_tokens: 4096,
  messages,
  context_management: {
    edits: [
      {
        type: "compact_20260112",
        pause_after_compaction: true
      }
    ]
  }
});

// Check if compaction triggered a pause
if (response.stop_reason === "compaction") {
  // Response contains only the compaction block
  messages.push({
    role: "assistant",
    content: response.content
  });

  // Continue the request
  response = await client.beta.messages.create({
    betas: ["compact-2026-01-12"],
    model: "claude-opus-4-8",
    max_tokens: 4096,
    messages,
    context_management: {
      edits: [{ type: "compact_20260112" }]
    }
  });
}
```
</CodeGroup>

#### Preserving recent messages after compaction

A common pattern is using `pause_after_compaction` to keep the most recent exchange verbatim while summarizing older history:

```python Python
import anthropic

client = anthropic.Anthropic()
messages = [{"role": "user", "content": "Help me build a web scraper"}]

response = client.beta.messages.create(
    betas=["compact-2026-01-12"],
    model="claude-opus-4-8",
    max_tokens=4096,
    messages=messages,
    context_management={
        "edits": [
            {
                "type": "compact_20260112",
                "trigger": {"type": "input_tokens", "value": 100000},
                "pause_after_compaction": True,
            }
        ]
    },
)

if response.stop_reason == "compaction":
    compaction_block = response.content[0]
    # Preserve the last 3 messages verbatim (latest exchange + current user msg)
    preserved = messages[-3:] if len(messages) >= 3 else messages

    # Build: compaction + preserved messages (older history is summarized)
    messages_after = [{"role": "assistant", "content": [compaction_block]}] + preserved

    response = client.beta.messages.create(
        betas=["compact-2026-01-12"],
        model="claude-opus-4-8",
        max_tokens=4096,
        messages=messages_after,
        context_management={"edits": [{"type": "compact_20260112"}]},
    )
    messages = messages_after

messages.append({"role": "assistant", "content": response.content})
```

#### Enforcing a total token budget

When a model works on long tasks with many tool-use iterations, total token consumption can grow significantly. Combine `pause_after_compaction` with a compaction counter to estimate cumulative usage and wrap up gracefully once a budget is reached:

```python Python
import anthropic

client = anthropic.Anthropic()
messages = [{"role": "user", "content": "Hello, Claude"}]
TRIGGER_THRESHOLD = 100_000
TOTAL_TOKEN_BUDGET = 3_000_000
n_compactions = 0

response = client.beta.messages.create(
    betas=["compact-2026-01-12"],
    model="claude-opus-4-8",
    max_tokens=4096,
    messages=messages,
    context_management={
        "edits": [
            {
                "type": "compact_20260112",
                "trigger": {"type": "input_tokens", "value": TRIGGER_THRESHOLD},
                "pause_after_compaction": True,
            }
        ]
    },
)

if response.stop_reason == "compaction":
    n_compactions += 1
    messages.append({"role": "assistant", "content": response.content})

    # Approximate budget check: each compaction implies ~TRIGGER_THRESHOLD
    # input tokens were consumed. For accurate tracking, sum usage.iterations.
    if n_compactions * TRIGGER_THRESHOLD >= TOTAL_TOKEN_BUDGET:
        messages.append(
            {
                "role": "user",
                "content": "Please wrap up your current work and summarize the final state.",
            }
        )
else:
    # Compaction didn't trigger — append response normally
    messages.append({"role": "assistant", "content": response.content})
```

<Note>
The budget estimate `n_compactions * TRIGGER_THRESHOLD` is approximate — it does not account for output token consumption. For accurate per-request tracking, sum `usage.iterations[*].input_tokens + output_tokens` across all iterations.
</Note>

## Working with compaction blocks

When compaction is triggered, the API returns a `compaction` block at the start of the assistant response:

```json Output
{
  "content": [
    {
      "type": "compaction",
      "content": "Summary of the conversation: The user requested help building a web scraper..."
    },
    {
      "type": "text",
      "text": "Based on our conversation so far..."
    }
  ]
}
```

A long-running conversation may undergo multiple compactions. Each compaction block replaces all message content before it. Only the most recent `compaction` block matters — prior ones are discarded.

### Passing compaction blocks back

You must pass the `compaction` block back to the API on subsequent requests. The simplest approach is to append the entire response content to your messages. When the API receives a `compaction` block, all content blocks before it are ignored.

You can either:

- **Keep the original messages** in your list and let the API handle removing compacted content
- **Manually drop** the compacted messages and include only the compaction block onward

### Streaming

When streaming responses with compaction enabled, the compaction block streams differently from text blocks:

1. `content_block_start` — fires with `content_block.type: "compaction"`
2. `content_block_delta` — a single delta with the complete summary content (no intermediate streaming)
3. `content_block_stop` — marks end of the compaction block

<CodeGroup>
```python Python
import anthropic

client = anthropic.Anthropic()
messages = [{"role": "user", "content": "Hello, Claude"}]

with client.beta.messages.stream(
    betas=["compact-2026-01-12"],
    model="claude-opus-4-8",
    max_tokens=4096,
    messages=messages,
    context_management={"edits": [{"type": "compact_20260112"}]},
) as stream:
    for event in stream:
        if event.type == "content_block_start":
            if event.content_block.type == "compaction":
                print("Compaction started...")
            elif event.content_block.type == "text":
                print("Text response started...")
        elif event.type == "content_block_delta":
            if event.delta.type == "compaction_delta":
                print(f"Compaction complete: {len(event.delta.content or '')} chars")
            elif event.delta.type == "text_delta":
                print(event.delta.text, end="", flush=True)

    # Get the final accumulated message
    message = stream.get_final_message()
    messages.append({"role": "assistant", "content": message.content})
```

```typescript TypeScript
import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic();
const messages: Anthropic.Beta.Messages.BetaMessageParam[] = [];

const stream = await client.beta.messages.stream({
  betas: ["compact-2026-01-12"],
  model: "claude-opus-4-8",
  max_tokens: 4096,
  messages,
  context_management: {
    edits: [{ type: "compact_20260112" }]
  }
});

for await (const event of stream) {
  if (event.type === "content_block_start") {
    if (event.content_block.type === "compaction") {
      console.log("Compaction started...");
    } else if (event.content_block.type === "text") {
      console.log("Text response started...");
    }
  } else if (event.type === "content_block_delta") {
    if (event.delta.type === "compaction_delta") {
      console.log(`Compaction complete: ${event.delta.content?.length ?? 0} chars`);
    } else if (event.delta.type === "text_delta") {
      process.stdout.write(event.delta.text);
    }
  }
}

// Get the final accumulated message
const message = await stream.finalMessage();
messages.push({
  role: "assistant",
  content: message.content
});
```

```go Go
package main

import (
	"context"
	"fmt"
	"log"

	"github.com/anthropics/anthropic-sdk-go"
)

func main() {
	client := anthropic.NewClient()
	messages := []anthropic.BetaMessageParam{anthropic.NewBetaUserMessage(anthropic.NewBetaTextBlock("Hello, Claude"))}

	stream := client.Beta.Messages.NewStreaming(context.TODO(), anthropic.BetaMessageNewParams{
		Model:     anthropic.ModelClaudeOpus4_8,
		MaxTokens: 4096,
		Messages:  messages,
		ContextManagement: anthropic.BetaContextManagementConfigParam{
			Edits: []anthropic.BetaContextManagementConfigEditUnionParam{
				{OfCompact20260112: &anthropic.BetaCompact20260112EditParam{}},
			},
		},
		Betas: []anthropic.AnthropicBeta{"compact-2026-01-12"},
	})

	for stream.Next() {
		event := stream.Current()
		switch e := event.AsAny().(type) {
		case anthropic.BetaRawContentBlockStartEvent:
			switch e.ContentBlock.AsAny().(type) {
			case anthropic.BetaCompactionBlock:
				fmt.Println("Compaction started...")
			case anthropic.BetaTextBlock:
				fmt.Println("Text response started...")
			}
		case anthropic.BetaRawContentBlockDeltaEvent:
			switch d := e.Delta.AsAny().(type) {
			case anthropic.BetaCompactionContentBlockDelta:
				fmt.Printf("Compaction complete: %d chars\n", len(d.Content))
			case anthropic.BetaTextDelta:
				fmt.Print(d.Text)
			}
		}
	}
	if err := stream.Err(); err != nil {
		log.Fatal(err)
	}
}
```
</CodeGroup>

### Prompt caching

Compaction works well with [prompt caching](/docs/en/build-with-claude/prompt-caching). You can add a `cache_control` breakpoint on compaction blocks to cache the summarized content:

```json
{
  "role": "assistant",
  "content": [
    {
      "type": "compaction",
      "content": "[summary text]",
      "cache_control": { "type": "ephemeral" }
    },
    {
      "type": "text",
      "text": "Based on our conversation..."
    }
  ]
}
```

<Note>
**Multi-compaction cache behavior:** When a second compaction occurs, the previous compaction block is no longer in the prompt (it's replaced by the new summary). The old cached entry expires naturally. The new compaction summary must be written to a fresh cache entry.
</Note>

#### Maximizing cache hits

When compaction occurs, the summary becomes new content that needs to be cached. Without additional cache breakpoints, this would also invalidate any cached system prompt. To maximize cache hit rates, add a `cache_control` breakpoint at the end of your system prompt:

<CodeGroup>
```python Python
import anthropic

client = anthropic.Anthropic()
messages = [{"role": "user", "content": "Hello, Claude"}]
response = client.beta.messages.create(
    betas=["compact-2026-01-12"],
    model="claude-opus-4-8",
    max_tokens=4096,
    system=[
        {
            "type": "text",
            "text": "You are a helpful coding assistant...",
            "cache_control": {"type": "ephemeral"},
        }
    ],
    messages=messages,
    context_management={"edits": [{"type": "compact_20260112"}]},
)
```
</CodeGroup>

This keeps the system prompt cached separately from the conversation. When compaction occurs:
- The **system prompt cache remains valid** and is read from cache
- Only the **compaction summary** needs to be written as a new cache entry

This is especially beneficial for long system prompts, as they remain cached across multiple compaction events.

## Understanding usage

Compaction requires an additional sampling step, which contributes to rate limits and billing. The API returns detailed usage information:

```json Output
{
  "usage": {
    "input_tokens": 23000,
    "output_tokens": 1000,
    "iterations": [
      {
        "type": "compaction",
        "input_tokens": 180000,
        "output_tokens": 3500
      },
      {
        "type": "message",
        "input_tokens": 23000,
        "output_tokens": 1000
      }
    ]
  }
}
```

- **`usage.iterations`** — only populated when a new compaction is triggered during the request
- **Top-level `input_tokens` / `output_tokens`** — sum of all **non-compaction** iterations only. These match the `message` iteration when there is only one.
- **Total billable tokens** — sum of all entries in `usage.iterations`
- **Re-applying** a previous `compaction` block incurs no additional compaction cost; the `iterations` array is absent in that case

<Important>
If you previously relied on `usage.input_tokens` and `usage.output_tokens` for cost tracking or auditing, you must update your tracking logic to aggregate across `usage.iterations` when compaction is enabled.
</Important>

## Token counting

The token counting endpoint (`/v1/messages/count_tokens`) applies existing `compaction` blocks in your prompt but does **not** trigger new compactions. Use it to check your effective token count after previous compactions:

<CodeGroup>
```python Python
import anthropic

client = anthropic.Anthropic()
messages = [{"role": "user", "content": "Hello, Claude"}]
count_response = client.beta.messages.count_tokens(
    betas=["compact-2026-01-12"],
    model="claude-opus-4-8",
    messages=messages,
    context_management={"edits": [{"type": "compact_20260112"}]},
)

print(f"Current tokens: {count_response.input_tokens}")
print(f"Original tokens: {count_response.context_management.original_input_tokens}")
```

```typescript TypeScript
import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic();
const messages: Anthropic.Beta.Messages.BetaMessageParam[] = [
  { role: "user", content: "Summarize the key points of our conversation so far." }
];

const countResponse = await client.beta.messages.countTokens({
  betas: ["compact-2026-01-12"],
  model: "claude-opus-4-8",
  messages,
  context_management: {
    edits: [{ type: "compact_20260112" }]
  }
});

console.log(`Current tokens: ${countResponse.input_tokens}`);
console.log(`Original tokens: ${countResponse.context_management!.original_input_tokens}`);
```

```go Go
package main

import (
	"context"
	"fmt"
	"log"

	"github.com/anthropics/anthropic-sdk-go"
)

func main() {
	client := anthropic.NewClient()
	messages := []anthropic.BetaMessageParam{anthropic.NewBetaUserMessage(anthropic.NewBetaTextBlock("Hello, Claude"))}

	countResponse, err := client.Beta.Messages.CountTokens(context.TODO(), anthropic.BetaMessageCountTokensParams{
		Model:    anthropic.ModelClaudeOpus4_8,
		Messages: messages,
		ContextManagement: anthropic.BetaContextManagementConfigParam{
			Edits: []anthropic.BetaContextManagementConfigEditUnionParam{
				{OfCompact20260112: &anthropic.BetaCompact20260112EditParam{}},
			},
		},
		Betas: []anthropic.AnthropicBeta{"compact-2026-01-12"},
	})
	if err != nil {
		log.Fatal(err)
	}

	fmt.Printf("Current tokens: %d\n", countResponse.InputTokens)
	fmt.Printf("Original tokens: %d\n", countResponse.ContextManagement.OriginalInputTokens)
}
```
</CodeGroup>

## Combining with other features

### Server tools

When using server tools (like web search), the compaction trigger is checked at the start of each sampling iteration. Compaction may occur multiple times within a single request depending on your trigger threshold and the amount of output generated through tool calls.

### Tool use in multi-turn scenarios

Tool use with large outputs (file reads, web scraping, search results) can consume significant context quickly. Compaction summarizes tool results along with conversation history. For optimal results in tool-heavy workflows:

- **Combine compaction with context editing** — use `clear_tool_uses` (see [Context editing](/docs/en/build-with-claude/context-editing)) to prune old tool results before they would trigger compaction
- **Set trigger thresholds lower** (e.g., 50–75% of default) when tool outputs are large
- **Use custom instructions** that tell the model to preserve tool results and structured data in the summary

## Complete example: long-running conversation with compaction

```python Python
import anthropic

client = anthropic.Anthropic()

messages = []


def chat(user_message: str) -> str:
    messages.append({"role": "user", "content": user_message})

    response = client.beta.messages.create(
        betas=["compact-2026-01-12"],
        model="claude-opus-4-8",
        max_tokens=4096,
        messages=messages,
        context_management={
            "edits": [
                {
                    "type": "compact_20260112",
                    "trigger": {"type": "input_tokens", "value": 100000},
                }
            ]
        },
    )

    messages.append({"role": "assistant", "content": response.content})
    return next(block.text for block in response.content if block.type == "text")


print(chat("Help me build a Python web scraper"))
print(chat("Add support for JavaScript-rendered pages"))
print(chat("Now add rate limiting and error handling"))
# ... continue as long as needed
```

## Error handling

Compaction-related errors manifest in these ways:

| Scenario | Symptom | Mitigation |
|:---------|:--------|:-----------|
| Tools defined + no custom instructions | `compaction` block with `content: null` | Add `instructions` that tell the model not to call tools during summarization |
| Summary exceeds context window | API returns a 400 error | Lower the trigger threshold or reduce `max_tokens` |
| Rate limit exceeded | Standard 429 error | Compaction iterations count toward the same rate limits as normal requests. Retry with exponential backoff. |
| Invalid beta header | 400: `"anthropic-beta: compact-2026-01-12" required` | Ensure the beta header is set on every request |

For production systems, always:
1. Check `response.stop_reason` to detect compaction pauses
2. Validate that compaction blocks contain non-null `content` before passing them back
3. Implement retry logic for transient failures (rate limits, network errors)

## Current limitations

- **Same model for summarization:** The model specified in your request is used for summarization. There is no option to use a different (e.g., cheaper) model.
- **Tool-use interaction:** When your request includes `tools`, the model occasionally calls a tool during the internal summarization step instead of writing a summary. When this occurs, the response contains a `compaction` block with `content: null`. Mitigate by setting `instructions` that explicitly prohibit tool calls during summarization (see [Custom summarization instructions](#custom-summarization-instructions)).
- **No hard limit on conversation length:** In practice, summary quality may degrade over many successive compactions for very long conversations. Monitor output quality and reset conversations periodically if needed.
- **Unavailable on smaller models:** Compaction is only supported on the models listed in [Supported models](#supported-models).

## Next steps

<CardGroup>
  <Card title="Session memory compaction cookbook" icon="book" href="https://platform.claude.com/cookbook/misc-session-memory-compaction">
    Explore a practical implementation that manages long-running conversations with instant session memory compaction using background threading and prompt caching.
  </Card>
  <Card title="Context windows" icon="arrows-maximize" href="/docs/en/build-with-claude/context-windows">
    Learn about context window sizes and management strategies.
  </Card>
  <Card title="Context editing" icon="pen" href="/docs/en/build-with-claude/context-editing">
    Explore other strategies for managing conversation context like tool result clearing and thinking block clearing.
  </Card>
</CardGroup>
