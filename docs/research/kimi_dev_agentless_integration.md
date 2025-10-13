# Integrating Kimi-Dev Agentless Skill Priors into VTCode

## Summary
- Adopt a staged training recipe where agentless learning supplies transferable skill priors before agentic fine-tuning.
- Focus on five atomic capabilities (localization, patching, test authoring, duo-role coordination, long-form reflection) during agentless training.
- Execute a data and reinforcement curriculum that mirrors Kimi-Dev's mid-training, cold-start SFT, and outcome-driven RL loop.
- Extend inference infrastructure with parallel bug fixer / test writer rollouts, execution verification, and adaptive sampling.
- Align platform and tooling investments (sandboxing, context limits, configuration) with the needs of long-horizon workflows.

## Stage 1: Agentless Skill Prior Program
1. **Curriculum Objectives**
   - Teach vtcode to localize faults, generate patches, and author regression tests using structured search/replace prompts.
   - Encourage long-form chain-of-thought, self-critique, and duo-role collaboration patterns before introducing autonomy.
2. **Data Sourcing and Curation**
   - Aggregate high-signal GitHub issues, PRs, and commit packs that include reasoning, diffs, and test coverage.
   - Upsample synthetic interaction traces (bug report → reasoning → diff → test) by 4× to reinforce reasoning structure.
   - Track dataset slices in `docs/research` with provenance metadata for reproducibility.
3. **Training Mechanics**
   - Mid-train for ~150B tokens using instruction formatting that mirrors vtcode's search/replace tooling.
   - Apply SFT with reasoning trajectories (e.g., DeepSeek R1 derived) as a cold start for extended chain-of-thought.
   - Log skill evaluations (localization accuracy, diff acceptance, failing-then-passing tests) after each epoch.

## Stage 2: Agentic Adaptation Pipeline
1. **Transition Criteria**
   - Promote to agentic fine-tuning once localization accuracy and patch acceptance exceed predefined vtcode thresholds.
   - Gate promotion on the ability to produce failing-before-fix regression tests for >80% of sampled tasks.
2. **Reinforcement Learning Focus**
   - Use execution outcome (pass/fail) as the sole reward; avoid template or process-based shaping.
   - Freeze localization modules initially and optimize the code editing policy head with PPO or GRPO variants.
   - Replay successful trajectories from the latest RL window to reinforce desirable reasoning loops.
3. **Adaptive Task Curriculum**
   - Maintain a task bank partitioned by difficulty; start RL on medium tasks with >10% baseline success.
   - Reintroduce hard tasks once success stabilizes to prevent catastrophic forgetting.

## Stage 3: Inference and Deployment Enhancements
1. **Self-Play Rollouts**
   - Run 40 BugFixer and 40 TestWriter rollouts per issue with temperature diversity (T=0 first pass, T=1 thereafter).
   - Archive reasoning traces and diffs for later distillation into agentless updates.
2. **Execution-Based Selection**
   - Score candidate patches by test pass rate, regression suite safety, and diff minimality.
   - Promote emergent synthesis: allow vtcode to draft a final patch after comparing the top-n candidates.
3. **Coverage Hardening**
   - Maintain a catalog of historical false positives; auto-inject counterexample tests into future self-play rounds.

## Platform and Tooling Requirements
- **Sandboxing**: Provision containerized runners (e.g., k8s-managed Docker) with resource quotas for concurrent rollouts.
- **Context Budgeting**: Ensure 64K-token contexts during training and support up to 128K at inference for long dialogues.
- **Configuration Hygiene**: Store new levers (rollout counts, reward weights, curriculum gates) in `vtcode.toml`; map constants through `vtcode_core::config::constants`.
- **Telemetry**: Extend existing observability to capture localization recall, patch acceptance, and test flake rate per stage.

## Next Actions
1. Define quantitative success targets and add them to the project roadmap.
2. Draft data ingestion scripts for GitHub issue/PR packs aligned with vtcode's tool schema.
3. Prototype the self-play executor inside the sandbox infrastructure and record performance baselines.
4. Schedule recurring evaluations to compare agentless-only, agentic-only, and staged models on shared benchmarks.
