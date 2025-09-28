# Trajectory Logging

VT Code records routing decisions and tool calls as structured JSON lines to support trajectory evaluation and continuous improvement.

-   Output file: `.vtcode/logs/trajectory.jsonl`
-   Writer: `vtcode-core/src/core/trajectory.rs` (`TrajectoryLogger`)

## Records

-   kind: "route" or "tool"
-   route fields: `turn`, `selected_model`, ` class`, `input_preview`, `ts`
-   tool fields: `turn`, `name`, `args` (JSON), `ok`, `ts`

## Usage

-   Enabled by default whenever chat loops run (single-agent and unified tool chat).
-   Safe for long sessions; appends one JSON object per line.

## Analysis Tips

-   Aggregate by `class` to see how often each complexity shows up and which model is selected.
-   Join `tool` events to evaluate tool success rates per class/model.
-   Use the timeline (`ts`) to reconstruct the agent trajectory for debugging.
