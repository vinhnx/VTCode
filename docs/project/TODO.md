implement claude advisor tool

--

fix buyme coffee readme

--

context engineering: managing the context the model sees at each inference step, so that multi-turn tool use is not drowned by history, noise, tool definitions, and intermediate results.

===

as tasks evolved into long-horizon tasks, agent capability became a system capability composed of the model, harness, context, tools, evals, sandbox, and state management together: within a finite context, external tools, and a changing environment, the agent must keep pushing toward a goal and get closer to completion over time.

===

A concrete context engineering example is: when there are too many MCP tools and too much data, we should not let every tool definition and intermediate result flow through context. Instead, the agent should write code to call tools on demand, filter the data, and bring only high-signal results back to the model. Context engineering also includes compaction, structured note-taking, and even multi-agent patterns, but these mostly serve the question of “how should context be managed?” The long-running agent harness discussed later goes one step further: it does not only manage context, but separates planning, implementation, evaluation, revision, and final acceptance into different processes.

---

Context engineering

People used to focus on prompt engineering, namely how to write the system prompt. But as agents become more multi-turn and longer-running, the focus becomes: at each inference step, what did the model see? For example, an agent’s context may contain the system prompt, developer instructions, tool definitions, MCP server descriptions, user messages, conversation history, file contents, command output, browser observations, screenshots, errors, progress notes, retrieved documents, evaluator feedback. These things cannot all be blindly stuffed in. Even if the context window becomes larger, we still need to choose selectively, because noise in a long context distracts the model and increases cost and latency. So a good context is the minimal high-signal context that lets the model complete the next step at the current step. In general, we use just-in-time context: instead of stuffing all documents, tools, and history into the model, we keep references, such as paths, URLs, commit hashes, log files, database table names, and so on. When the model needs them, it loads them through tools. In this way, the agent’s working memory keeps indexes and currently relevant fragments, rather than all of the context.

Common context techniques in long tasks include:

    Compaction: When the context is almost full, compress the current conversation trace into a summary, then use that summary to start the next context. Usually this is still the continuation of the same task and the same agent loop. Its goal is to preserve conversational continuity as much as possible. In other words, the model should feel that “what I was just doing is still here”; only the history has been compressed.

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

optimize CI build

INFO: CI status: in_progress (1995s elapsed)
INFO: CI status: completed (2027s elapsed)

it's taking too long to build.

https://github.com/vinhnx/VTCode/actions

The goal: reduce the CI build time. This may involve optimizing the build process, caching dependencies, parallelizing tasks, and removing unnecessary steps. We should also review the current CI configuration to identify any bottlenecks or inefficiencies that can be addressed. Optimize for speed without sacrificing reliability or correctness. Consider using incremental builds, leveraging build artifacts, and ensuring that tests are run in a way that maximizes efficiency. check connect with release.sh script for existing release infra, and see if it can be optimized or streamlined. The goal is to have a CI/CD pipeline that is fast, reliable, and easy to maintain, allowing for rapid iteration and deployment of changes.

===

• Read file /Users/vinhnguyenxuan…/vtcode/src/startup/mod.rs
└ Offset: 70
└ Limit: 400
└ Read 400 lines

IMPORTANT: some times read file tool offset and lines are too large (400 lines?), which can cause the model to be overwhelmed with too much information. We should implement a mechanism to limit the amount of data read from files, ensuring that only relevant sections are loaded into context. This may involve reading files in chunks, using pagination, or applying filters to extract only the necessary information. The goal is to provide the model with a manageable amount of data that is directly relevant to the current task, improving efficiency and reducing cognitive load.

===

check the agent loop again:

" 3. Goals (measurable)
• \*\*P50 launch for vtcode --version / vtcode --help... [plan]
[reasoning] Tool wall-clock budget exhausted. I have enough context to synthesize a solid plan. Let me compile what I know:"

==> here the plan mode summarization is interrupted, and then the agent changes to reasoning thinking, this is not ideal. The agent should be able to complete the plan mode summarization without being interrupted by reasoning thinking. We need to ensure that the agent can maintain focus on the current mode of operation, whether it is planning, reasoning, or executing tasks. This may involve implementing better context management, ensuring that the agent has enough information to complete its current task before switching modes, and providing clear boundaries between different modes of operation.

=> then later on, the agent completely failed and stop

"
------------------------------------------------------ Info -------------------------------------------------------
Tool execution completed, but the model follow-up failed (transient — may resolve on retry). Output above is
valid.
"

---

• I've already gathered enough context. Let me also quickly check the dispatch/mod.rs files and one more critical
area, the validate_startup_configuration and init_global_guardian/dotfolder, to confirm my findings with a structured file listing."

==> check logs: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_621.json

---

total eleminate `unsafe {` calls in the codebase. The goal is to ensure that the code is safe and free from undefined behavior, which can lead to security vulnerabilities and runtime errors. This may involve refactoring code to use safe abstractions, leveraging Rust's ownership and borrowing system, and applying best practices for memory safety. We should also review existing unsafe blocks to determine if they can be replaced with safe alternatives or if they are necessary for performance reasons. The aim is to maintain a high level of safety and reliability in the codebase while still achieving the desired functionality.

---

remove local models via LM Studio / Ollama / llama.ccp support. Please note that local model inference via LM Studio and Ollama/llama.ccp is very experimental and not working as expected. Since using local models is hard and take much effort, I might plan to remove local model support completely in the future and use remote LLM API instead.
