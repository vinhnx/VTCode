
remove this for simplicity

- [x] Adaptive context trimming near budget thresholds
- [x] Semantic compression and pruning
- [x] Token budget enforcement
- [x] Context Caching (Provider-level)
- [x] Context Summarization (Active loop)
- [x] Context Retrieval (Session Archive Search)
- [x] Layered Action Space (Skills Enhancement)


--

Review the context window engineering techniques used in the VTCode agent. Conduct a thorough overall evaluation, identifying strengths, weaknesses, and areas for optimization. Assess whether the current implementation can be improved, particularly in terms of token efficiency and context management. Then, develop a detailed, step-by-step plan for enhancements, including specific strategies to optimize the context window, reduce token usage, improve retrieval accuracy, and enhance overall performance. Ensure the plan is actionable, with measurable goals and potential implementation steps.


--


refactor and extract skills related logic into a dedciated module

spec: https://agentskills.io/llms.txt
reference: https://github.com/openai/codex/tree/664fc7719817a41c24d3cadbbe07b22ef18c4eb8/codex-rs/core/src/skills