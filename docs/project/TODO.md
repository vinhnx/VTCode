use ratatui crate and integrate minimal Terminal User Interface (TUI) for vtagemt. using the Ratatui crate (reference: https://docs.rs/ratatui/latest/ratatui/). The goal is to port the core logic from an existing CLI-based implementation—including the chat runloop, context management, and agent core logic—to a fully functional TUI version. Ensure a 1-to-1 port of functionality, followed by end-to-end testing to verify seamless operation, such as sending user inputs, processing agent responses, maintaining chat history, and handling intermediate states like tool calls and reasoning.

Key requirements:

-   **Simplicity and Minimality**: Focus on essential features: a chat display area for conversation history (including user messages, agent responses, tool calls, reasoning traces, action logs, and loading statuses), a minimal input field for user messages, and basic navigation (e.g., Enter to send, Esc to quit, arrow keys for scrolling history). Avoid unnecessary complexity; prioritize responsive, keyboard-driven interaction in a standard terminal environment.
-   **Enhanced UI Elements for Feedback**:
    -   Implement a loading UI/UX spinner (e.g., using Ratatui's spinning widget or a custom animated indicator) that appears immediately upon receiving a user message and persists while the model is executing or finishing tool calls.
    -   Provide real-time feedback by rendering action logs and traces in the TUI as they occur, ensuring the interface remains responsive during processing.
    -   Arrange message cells clearly: Display tool calls, reasoning steps, action logs, and loading statuses in distinct, compact sections within the chat area (e.g., using bordered blocks or paragraphs with timestamps/icons for visual separation), providing just enough information without overwhelming the layout—e.g., collapse verbose traces into expandable summaries if needed.
-   **Core Porting Steps**:
    1. Extract and adapt the CLI's chat runloop to handle TUI events (e.g., using Ratatui's event loop with Crossterm backend), integrating loading spinners and action log rendering for asynchronous agent processing.
    2. Port context and agent logic to integrate with TUI rendering, ensuring state persistence across renders, including real-time updates for tool calls, reasoning, and logs.
    3. Replace CLI output (e.g., println!) with Ratatui widgets for formatted display, handling ANSI escapes via the ansi-to-tui crate if needed, and extend to render dynamic elements like spinners and log traces.
-   **Testing**: Implement a comprehensive end-to-end test suite that simulates user interactions (e.g., typing messages, triggering agent replies, tool calls, and loading states) and validates output consistency with the original CLI behavior, including visual verification of spinners, logs, and message arrangements.

Incorporate and adapt code from the following resources for implementation:

-   **Ratatui Core**: Use as the foundation for TUI rendering and event handling (reference example implementations like codex-rs if available for chat-like UIs with dynamic updates).
-   **Input Handling Examples**:
    -   Input form: https://github.com/ratatui/ratatui/tree/main/examples/apps/input-form (adapt for a single-line chat input with validation and Enter-to-send).
    -   User input: https://github.com/ratatui/ratatui/tree/main/examples/apps/user-input (use for real-time keyboard input capture and editing, including Esc for quit).
-   **Templates and Best Practices**:
    -   Starter template: https://github.com/ratatui/templates (initialize project structure with a basic app loop supporting scrolling and state management).
    -   Awesome Ratatui: https://github.com/ratatui/awesome-ratatui (draw from community examples for chat layouts, scrolling lists, loading indicators, and log displays).
-   **Supporting Crates and Examples**:
    -   ANSI-to-TUI: https://github.com/ratatui/ansi-to-tui (integrate to render agent responses with colors/styles from existing CLI ANSI output, including traces and statuses).
    -   Widgets: https://github.com/ratatui/ratatui/tree/main/ratatui-widgets/examples (leverage Paragraph for chat messages and logs, List for history scrolling, Block for borders/padding around message cells, and custom spinners for loading feedback).

--

apply the agent to use curl with caution and security in mind, it should always validate URLs and avoid downloading untrusted content. -> add curl to tool policy with safe defaults and restrictions. note that curl can be dangerous if misused, so note about security implications in the usage and let user know when using curl tool.

--

encourage the agent to use /tmp to store temporary files and clean them up after use.

---

Platform Availability & Technical Limitations

-   Available through homebrew, npm, and direct binary downloads from GitHub releases, with plans to improve Windows support but no PyPi package available due to significant work required for every package manager

-   Would love to support more IDEs like JetBrains but huge amount of work remains on core experience

-   The team is shipping UI improvements but acknowledges terminal output readability issues, as different terminals render outputs differently - more improvements are coming

Usage Limits & Pricing Structure

-   Product lacks UI to show approaching limits which team is working to improve

-   Rate limits reset every 5 hours and weekly

-   No free tier available and no current plans for mid-tier between Plus and Pro though many users request one

-   Batch API-style usage for Codex web during unused GPU capacity discussed as great idea but not prioritized

Model Capabilities & Configuration

-   GPT-5-Codex model is specifically optimized for coding tasks with focused training on diverse coding environments, making separate specialized models for frontend/backend potentially unnecessary since coding tasks span multiple domains

-   Works well with large codebases using grep instead of dedicated indexing, can be prompted to work longer/faster and produce multi-page detailed implementation plans with different specification levels

-   Codex web chooses best configuration for tasks without allowing model or reasoning selection

-   GPT-5-high recommended for planning with more general world knowledge, GPT-5-Codex for technical refactors

-   No plans to allow system prompt editing though users can modify AGENTS[.]md for coding-adjacent tasks like data analysis or non-coding tasks

Features & User Experience

-   CLI supports web search with --search flag coming to IDE extension soon with prompt caching issues being resolved, potentially with full browser automation in the future

-   VS Code extension supports drag & drop when holding shift key, has auto context feature and enables mixing local and cloud work

-   File tagging with @ requested for folders not just individual files

-   Voice mode for terminal/IDE interaction - team would find it very cool to provide native support after seeing exciting open source community demos hacking together voice and coding agents

-   Can try local models with ollama using --oss flag though not first-class experience yet, with any future gpt-oss versions expected to work much better than current 20B model

Planning & Agent Development

-   Currently has Chat/Plan mode in IDE extension and read-only mode in CLI, working on dedicated plan mode with team landing on giving users more control over execution rather than having model do its own planning

-   Sub-agents are a fantastic way of preserving context for longer complex tasks but nothing actively being worked on right now

-   Conversation compacting for longer work coming soon and users able to ask Codex to create plans in markdown files for review and editing, with ability to prompt for multi-page documents where model will work for extended periods

---

keyboard navigation to scroll up and down the chat history, and move to cursor in chat message input e.g. using arrow keys or j/k for vim-style navigation. also allow ctrl+arrow to move by word, home/end to move to start/end of line, and page up/down to scroll by page in chat history.

---

add escape key to cancel current run, double cancel to halt chat session.

---

on control-c, briefly token summarize and tools used before exiting.

---

add https://github.com/rust-cli/anstyle/blob/main/crates/anstyle-git to handle ansi git color codes in `run_terminal_cmd` tool call output if git is used as a tool.

---

add https://github.com/rust-cli/anstyle/blob/main/crates/anstyle-ls to handle ansi ls color codes in `list_files` tool call output if ls is used as a tool.

---

explore https://github.com/rust-cli/anstyle/tree/main/crates/anstyle-syntect to enhance or replace current `syntect` package we are using. enhance tools output with syntax highlighting for code snippets in tool call outputs.

---

`colorchoice-clap` check https://github.com/rust-cli/anstyle/tree/main/crates/colorchoice-clap to handle color choice in clap.

---

--

Optimize prompts for GPT-5 models by following these best practices.
https://cookbook.openai.com/examples/gpt-5/gpt-5_prompting_guide

---

check this package and find a way to better use ansi escape codes in the terminal output. https://github.com/rust-cli/anstyle
if not found, search for other similar packages.

---

check git stash@{1}: On main: streaming. apply only streaming implementation

---

implement planning mode and TODO list (research)

---

support more models and providers.

---

Completed 2025-09-20: OpenRouter provider integration landed. CLI `--provider openrouter` and custom model overrides now
supported; see [`docs/providers/openrouter.md`](../providers/openrouter.md) for details.

---

support huggingface models

---

https://agentclientprotocol.com/overview/introduction

---

reference this https://github.com/openai/codex/tree/main/codex-rs/file-search to update file search tool for vtcode.

check

Uses https://crates.io/crates/ignore under the hood (which is what ripgrep uses) to traverse a directory (while honoring .gitignore, etc.) to produce the list of files to search and then uses https://crates.io/crates/nucleo-matcher to fuzzy-match the user supplied PATTERN against the corpus. write tests to verify it works as expected. update docs and readme accordingly. update system prompt for vtcode to reflect the changes.

---

update rustc and make vtcode use latest 1.90 rustc vversion

---

https://crates.io/crates/ignore
ignore

The ignore crate provides a fast recursive directory iterator that respects various filters such as globs, file types and .gitignore files. This crate also provides lower level direct access to gitignore and file type matchers.

---

https://crates.io/crates/nucleo-matcher

nucleo is a highly performant fuzzy matcher written in rust. It aims to fill the same use case as fzf and skim. Compared to fzf nucleo has a significantly faster matching algorithm. This mainly makes a difference when matching patterns with low selectivity on many items. An (unscientific) comparison is shown in the benchmark section below.

    Note: If you are looking for a replacement of the fuzzy-matcher crate and not a fully managed fuzzy picker, you should use the nucleo-matcher crate.

nucleo uses the exact same scoring system as fzf. That means you should get the same ranking quality (or better) as you are used to from fzf. However, nucleo has a more faithful implementation of the Smith-Waterman algorithm which is normally used in DNA sequence alignment (see https://www.cs.cmu.edu/~ckingsf/bioinfo-lectures/gaps.pdf) with two separate matrices (instead of one like fzf). This means that nucleo finds the optimal match more often. For example if you match foo in xf foo nucleo will match x\_\_foo but fzf will match xf_oo (you can increase the word length the result will stay the same). The former is the more intuitive match and has a higher score according to the ranking system that both nucleo and fzf.

Compared to skim (and the fuzzy-matcher crate) nucleo has an even larger performance advantage and is often around six times faster (see benchmarks below). Furthermore, the bonus system used by nucleo and fzf is (in my opinion) more consistent/superior. nucleo also handles non-ascii text much better. (skims bonus system and even case insensitivity only work for ASCII).

Nucleo also handles Unicode graphemes more correctly. Fzf and skim both operate on Unicode code points (chars). That means that multi codepoint graphemes can have weird effects (match multiple times, weirdly change the score, ...). nucleo will always use the first codepoint of the grapheme for matching instead (and reports grapheme indices, so they can be highlighted correctly).

--

https://github.com/Stebalien/term

A Rust library for terminfo parsing and terminal colors.

--

Ensure that the "Thinking" spinner and loading message appear asynchronously and immediately upon the end of the user's turn. Additionally, investigate and resolve any blocking mechanisms that may occur between user turns and agent turns to improve responsiveness. example for openrouter provider integration.

---

check async-stream for streaming https://crates.io/crates/async-stream?

---

https://github.com/openai/codex/blob/main/codex-rs/core/src/plan_tool.rs

---

https://github.com/openai/codex/blob/main/codex-rs/core/src/project_doc.rs

---

https://github.com/openai/codex/blob/main/codex-rs/core/src/terminal.rs

---

https://ai.google.dev/gemini-api/docs/text-generation#streaming-responses

Streaming responses

By default, the model returns a response only after the entire generation process is complete.

For more fluid interactions, use streaming to receive GenerateContentResponse instances incrementally as they're generated.
Python
JavaScript
Go
REST
Apps Script

```
curl "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse" \
  -H "x-goog-api-key: $GEMINI_API_KEY" \
  -H 'Content-Type: application/json' \
  --no-buffer \
  -d '{
    "contents": [
      {
        "parts": [
          {
            "text": "Explain how AI works"
          }
        ]
      }
    ]
  }'

```

---

Examine the file src/agent/runloop/gemini.rs. Investigate why only Gemini is supported in the /runloop directory. Determine if this file is specific to Gemini or if it can be generalized to support all providers. If possible, suggest modifications to apply it to all providers.

---

handle reasoning trace display in chat repl. if a model supports reasoning trace, display it in the chat repl. if not, skip it.

show along side with messages, tool calls, action logs, and loading status. style if differently to distinguish it from other message types.

---
