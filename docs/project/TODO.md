context engineering: managing the context the model sees at each inference step, so that multi-turn tool use is not drowned by history, noise, tool definitions, and intermediate results.

===

as tasks evolved into long-horizon tasks, agent capability became a system capability composed of the model, harness, context, tools, evals, sandbox, and state management together: within a finite context, external tools, and a changing environment, the agent must keep pushing toward a goal and get closer to completion over time.

===

A concrete context engineering example is: when there are too many MCP tools and too much data, we should not let every tool definition and intermediate result flow through context. Instead, the agent should write code to call tools on demand, filter the data, and bring only high-signal results back to the model. Context engineering also includes compaction, structured note-taking, and even multi-agent patterns, but these mostly serve the question of “how should context be managed?” The long-running agent harness discussed later goes one step further: it does not only manage context, but separates planning, implementation, evaluation, revision, and final acceptance into different processes.

---

Context engineering

People used to focus on prompt engineering, namely how to write the system prompt. But as agents become more multi-turn and longer-running, the focus becomes: at each inference step, what did the model see? For example, an agent’s context may contain the system prompt, developer instructions, tool definitions, MCP server descriptions, user messages, conversation history, file contents, command output, browser observations, screenshots, errors, progress notes, retrieved documents, evaluator feedback. These things cannot all be blindly stuffed in. Even if the context window becomes larger, we still need to choose selectively, because noise in a long context distracts the model and increases cost and latency. So a good context is the minimal high-signal context that lets the model complete the next step at the current step. In general, we use just-in-time context: instead of stuffing all documents, tools, and history into the model, we keep references, such as paths, URLs, commit hashes, log files, database table names, and so on. When the model needs them, it loads them through tools. In this way, the agent’s working memory keeps indexes and currently relevant fragments, rather than all of the context.

Common context techniques in long tasks include:

    Compaction: When the context is almost full, compress the current conversation trace into a summary, then use that summary to start the next context. Usually this is still the continuation of the same task and the same agent loop. Its goal is to preserve conversational continuity as much as possible. In other words, the model should feel that “what I was just doing is still here”; only the history has been compressed. It should use a cheaper model to summarize, and the summary should be written into a persistent artifact, such as progress.md, so that later sessions can read it. Compaction is not necessarily only for the same task; a project-level progress.md can be read by later sessions, by planner/generator/evaluator, or reused in neighboring tasks.

    Structured note-taking: Let the agent actively write task state into persistent external artifacts, such as progress.md, feature_list.json, NOTES.md, or git commits. It is not necessarily only for the same task. A project-level NOTES.md can be read by later sessions, by planner/generator/evaluator, or reused in neighboring tasks.

    Context reset: Use external artifacts, such as files produced by note-taking, git logs, test results, task lists, and so on, as startup material to open a clean new context/session. It does not preserve the full conversation history, and can clear noise and bad assumptions so that a new agent can reorient itself.

    Multi-agent architecture: Do role division and context isolation. Different agents receive different context, do different subtasks, and finally pass only high-density results back to the main flow.

===

Tool use

There are also some things to watch out for in tool use. For example, the number of tools cannot expand without limit, otherwise the model wastes attention on choosing. Tools should not overlap too much in functionality, otherwise the model does not know which one to use. Tool descriptions should clearly explain when to use them and when not to use them, namely they should make the boundaries clear. Return values should be token-efficient and should not pour irrelevant data into context. Returned error messages should let the model know what to repair next. Names should be clear, so the agent can infer boundaries from the names. Also, tools are preferably not exposed directly to the model as direct tool calls, but mapped into code APIs / a file tree, so the agent can call them through code. This way, the agent can read only the tool definitions it currently needs, filter large data inside the code execution environment, and return only summaries or a small number of results to the model. It can use loops, conditionals, and exception handling to complete complex control flow, or write intermediate state into files instead of stuffing it into context. Tool outputs are deterministic; we should leave deterministic computation to code. Judgment and planning, such as how to call tools, which tools to call, in what order, and how many times, should be left to the model.

===

https://jinyansu1.github.io/blog/2026/07/agent-context-engineering-long-running-harness/

===

check current existing .logs/.checkpoint/.sessions infra, and see if they're redudant or can be unified. The goal is to have a single source of truth for the agent's state, context, and history, which can be efficiently queried and updated as the agent progresses through its tasks. This may involve consolidating logs, checkpoints, and session data into a more streamlined format that supports both real-time inference and long-term learning. It's also prevent overhead from accumulating in the context, ensuring that the runtime environment remains responsive and efficient.

===

IMPORTANT for vtcode's compaction engine. Compaction: When the context is almost full, compress the current conversation trace into a summary, then use that summary to start the next context. Usually this is still the continuation of the same task and the same agent loop. Its goal is to preserve conversational continuity as much as possible. In other words, the model should feel that “what I was just doing is still here”; only the history has been compressed.

===

IMPORTANT CRITICAL: review vtcode's tool systems. the number of tools cannot expand without limit, otherwise the model wastes attention on choosing.

Tool use

There are also some things to watch out for in tool use. For example, the number of tools cannot expand without limit, otherwise the model wastes attention on choosing. Tools should not overlap too much in functionality, otherwise the model does not know which one to use. Tool descriptions should clearly explain when to use them and when not to use them, namely they should make the boundaries clear. Return values should be token-efficient and should not pour irrelevant data into context. Returned error messages should let the model know what to repair next. Names should be clear, so the agent can infer boundaries from the names. Also, tools are preferably not exposed directly to the model as direct tool calls, but mapped into code APIs / a file tree, so the agent can call them through code. This way, the agent can read only the tool definitions it currently needs, filter large data inside the code execution environment, and return only summaries or a small number of results to the model. It can use loops, conditionals, and exception handling to complete complex control flow, or write intermediate state into files instead of stuffing it into context. Tool outputs are deterministic; we should leave deterministic computation to code. Judgment and planning, such as how to call tools, which tools to call, in what order, and how many times, should be left to the model.

===

https://github.com/vinhnx/VTCode/issues/698

---

IMPORTANT: when the agent loop processing is running, prevent switch mode (e.g. from tool-free to tool-enabled) from happening mid-turn. The agent loop should complete the current turn before switching modes, to avoid inconsistent state and context corruption. This may involve locking the mode state during turn processing or queuing mode switch requests until the turn is fully processed. also disable keyboard shortcuts that can trigger mode switches during active processing, to ensure that the agent's state remains stable and predictable.

===

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/memory/token-efficiency-analysis.md
