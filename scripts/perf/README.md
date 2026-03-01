# Local Performance Scripts

These scripts provide a repeatable local performance workflow for VT Code.

## Commands

```bash
# Capture metrics + raw logs
./scripts/perf/baseline.sh baseline
./scripts/perf/baseline.sh latest

# Compare two captured runs
./scripts/perf/compare.sh \
  .vtcode/perf/baseline.json \
  .vtcode/perf/latest.json

# Build release binary for profiling (line tables + frame pointers)
./scripts/perf/profile.sh

# Local-only host-tuned build/run
./scripts/perf/native-build.sh
./scripts/perf/native-run.sh -- --version
```

## Outputs

All artifacts are written to `.vtcode/perf/`:

- `baseline.json` / `latest.json`: captured metrics
- `*-cargo_check.log`: cargo check output
- `*-bench_core.log`: `vtcode-core` bench output
- `*-bench_tools.log`: `vtcode-tools` bench output
- `*-startup.json` (if `hyperfine` installed)
- `diff.md`: markdown comparison report
