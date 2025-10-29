# Integrating Agentless Skill Priors into VT Code

## Purpose
This document translates the lessons from the Kimi-Dev research note into concrete actions for VT Code. The goal is to establish a staged training pipeline that builds strong agentless skill priors before layering agentic behaviours.

## Staged Training Paradigm
1. **Stage 0 – Agentless Skill Prior**
   - Build dual-role agents (`BugFixer`, `TestWriter`) using scripted workflows and deterministic tooling.
   - Focus on localization, patch authoring, test authoring, and long-form self-reflection via extended reasoning traces.
   - Align prompts with the SEARCH/REPLACE editing paradigm already supported by `tools::apply_patch`.
2. **Stage 1 – Cold-Start SFT**
   - Fine-tune on curated reasoning trajectories (DeepSeek R1, internal vtcode traces) to enable long CoT, error analysis, and self-critique.
   - Maintain dual-role prompts and require each trajectory to include test failure reproduction, patch proposal, and verification output.
3. **Stage 2 – RL for Code Edit Quality**
   - Run RL using execution-based binary rewards (pass/fail) in the sandbox infrastructure.
   - Keep localization fixed; only adapt prompts and policies governing patch synthesis.
   - Apply adaptive curriculum scheduling: focus on partially solved tasks, then gradually reincorporate harder problems as success increases.

## Data Curation Strategy
- **Mid-Training (≈150B tokens)**
  - Mine GitHub issues and PRs with paired diffs and test cases.
  - Capture both natural diff patches and PR commit packs (message + diff + tests).
  - Normalize metadata into VT Code’s training schema (issue summary, reproduction steps, failing tests, final patch).
- **Synthetic Upsampling (×4)**
  - Generate synthetic interactions that mimic the dual-role workflow, including reasoning steps and failure analysis.
  - Validate synthetic tests by running them in the sandbox to prevent false positives.
- **Trace Logging**
  - Instrument `vtcode-core` to persist agent interaction logs (reasoning, tool calls, execution results) for future SFT datasets.

## Prompt and Tooling Alignment
- Standardize prompts for `BugFixer` and `TestWriter` in `vtcode-core/src/prompts` with explicit role headers, required outputs, and verification checklists.
- Extend the tool registry to expose:
  - High-signal localization tools (e.g., `tools::grep_file`, tree-sitter-based AST queries).
  - Test scaffolding helpers that stub failing tests with reproduction steps.
- Require both roles to emit structured JSON outputs (patch diff, tests, rationales) to simplify downstream evaluation and RL logging.

## Inference and Patch Selection
- **Test-Time Self-Play**
  - Run `N=40` rollouts per role with temperature diversification (first pass greedy, subsequent passes at temperature 1.0).
  - Score patches by executing the generated tests plus regression suites; prefer solutions that pass all coverage and include minimal diff footprint.
- **Aggregated Synthesis**
  - Post-process the top-k patches by diffing them and prompting the model to synthesize a merged candidate that resolves conflicting edits.
- **Coverage Monitoring**
  - Track false positives from the TestWriter role and automatically request additional test variants when coverage gaps are detected.

## Infrastructure Requirements
- Provision isolated sandboxes (Kubernetes + Docker) for concurrent execution-based evaluation and RL loops.
- Guarantee long-context availability (64K during training, 128K at inference) to hold full interaction histories.
- Capture execution telemetry (command, exit status, logs) for reward calculation and debugging.

## Implementation Roadmap for VT Code
1. **Prompt & Role Templates** – Draft dual-role prompt templates and JSON schemas; integrate with prompt configuration in `vtcode.toml`.
2. **Data Pipeline** – Build ingestion scripts for GitHub/PR datasets and synthetic generation; store curated corpora in the research data lake.
3. **Agentless Runner** – Implement deterministic workflows that call VT Code tools to produce patches/tests without autonomous planning.
4. **SFT Preparation** – Aggregate reasoning traces from agentless runs and external datasets; fine-tune to activate long-form CoT.
5. **RL Harness** – Connect sandbox execution outputs to the RL loop with binary rewards and curriculum scheduling.
6. **Self-Play Inference** – Expose CLI/API controls for multi-rollout evaluation, patch synthesis, and regression scoring.
7. **Monitoring & Iteration** – Instrument metrics for localization accuracy, patch acceptance rate, test reliability, and RL convergence.

