// Saved workflow: crate-ops (Muse runner for the crate-ops skill)
//
// Harness-neutral procedure: .agents/skills/crate-ops/SKILL.md (source of
// truth; usable from Claude Code, Codex, VT Code, or by hand). This script is
// the Muse executable version of that skill: it performs a scoped
// module/crate change (manifest edits, feature gating, dependency moves,
// small extractions) and verifies it the way CI and crates.io will:
// isolated per-crate builds (no feature-unification masking), the headless
// ACP contract, and `cargo publish --dry-run` packaging.
//
// Invoke with muse.workflow, name "crate-ops", and args:
//   {
//     "task": "gate vtcode-core TUI stack behind default-on tui feature",
//     "crates": ["vtcode-core", "vtcode"],
//     "scope_paths": ["crates/codegen/vtcode-core/Cargo.toml"],  // optional
//     "publish_dry_run": true,  // default true: cargo publish --dry-run per crate
//     "headless": true,         // default true: vtcode-acp headless contract
//     "desktop_gate": false     // default false: slow GPUI desktop check (CI covers it)
//   }
//
// Returns a parent report: {status, verdict, task, crates, implementation,
// gates[], synthesis, unresolved[], evidence[], notes[]}. verdict is
// "verified" only when the implementation completed AND every gate passed.

export default async function workflow(host) {
  const args = host.args && typeof host.args === "object" ? host.args : {};
  const task = typeof args.task === "string" ? args.task.trim() : "";
  const crates = Array.isArray(args.crates)
    ? args.crates.filter((c) => typeof c === "string" && c.trim())
    : [];
  const scopePaths = Array.isArray(args.scope_paths)
    ? args.scope_paths.filter((p) => typeof p === "string" && p.trim())
    : [];
  const wantPublish = args.publish_dry_run !== false;
  const wantHeadless = args.headless !== false;
  const wantDesktop = args.desktop_gate === true;

  const evidenceSchema = {
    type: "object",
    required: ["complete", "evidence", "unresolved"],
    properties: {
      complete: { type: "boolean" },
      evidence: { type: "array", items: { type: "string" } },
      unresolved: { type: "array", items: { type: "string" } },
    },
  };

  const problems = [];
  if (!task) {
    problems.push("args.task is required: describe the crate/module change to perform.");
  }
  if (crates.length === 0) {
    problems.push('args.crates is required: list affected crates, e.g. ["vtcode-core", "vtcode"].');
  }
  if (problems.length > 0) {
    return { status: "invalid_args", unresolved: problems, evidence: [], notes: [] };
  }

  const notesFor = (result, scope) =>
    typeof result?.notes === "string" && result.notes.trim()
      ? [{ source: scope, ref: result.ref, value: result.notes }]
      : [];

  const compact = (result, scope) => {
    const failed = result === null || result.error_kind;
    const data = !failed && result.data && typeof result.data === "object" ? result.data : null;
    const evidence =
      !failed && Array.isArray(data?.evidence) ? data.evidence.filter(Boolean) : [];
    const declared =
      !failed && Array.isArray(data?.unresolved) ? data.unresolved.filter(Boolean) : [];
    const complete = data?.complete === true && declared.length === 0;
    return {
      scope,
      ref: result?.ref ?? null,
      complete,
      evidence,
      notes: notesFor(result, scope),
      unresolved: complete
        ? []
        : declared.length
          ? declared
          : [`${scope}: missing complete evidence result`],
    };
  };

  host.phase("Implement crate change");
  const implement = await host.agent({
    input: [
      `Perform this vtcode crate/module change: ${task}.`,
      `Affected crates: ${crates.join(", ")}.`,
      scopePaths.length > 0
        ? `Only modify files under: ${scopePaths.join(", ")}.`
        : "Keep changes surgical: only touch files the change owns.",
      "Repo rules: read root AGENTS.md and each touched crate's AGENTS.md first; anyhow::Result with context; no unwrap/expect/panic in production code; workspace lints apply; never `cargo test` (use `cargo nextest run --locked` if tests are needed); do not commit.",
      "Manifest rules: keep path+version workspace deps; when a crate needs an optional feature of a dependency, declare it explicitly (e.g. default-features = false, features = [\"tui\"]), never rely on feature unification from other workspace members.",
      "Return the evidence schema: complete=true only when the change is fully implemented; evidence lists each modified file with one line on what changed; unresolved lists anything you could not do.",
    ].join("\n"),
    schema: evidenceSchema,
    label: "crate-ops-implement",
  });
  const implementReport = compact(implement, "implement");
  if (!implementReport.complete) {
    return {
      status: "partial",
      verdict: "not_verified",
      reason:
        "Implementation did not complete; verification skipped so gates cannot give a false pass on unchanged code.",
      gates: [implementReport],
      unresolved: implementReport.unresolved,
      evidence: [],
      notes: implementReport.notes,
    };
  }

  host.phase("Verify crate change");
  const verifyRequests = [
    {
      label: "isolated-checks",
      input: [
        `Verify these vtcode crates in ISOLATION (one cargo invocation per crate, never a single workspace-wide build): ${crates.join(", ")}.`,
        "For each crate, run standalone (no pipes that hide exit status):",
        "  cargo check --locked -p <crate>",
        "  cargo clippy --locked -p <crate> --all-targets -- -D warnings",
        "Isolated builds defeat feature-unification masking: a crate must compile with exactly the features its own manifest requests.",
        "Evidence schema: complete=true only if every command exits 0; evidence holds one line per command with crate, command, exit code, and error count; unresolved holds failing crates with the first error line.",
      ].join("\n"),
    },
  ];
  if (wantHeadless) {
    verifyRequests.push({
      label: "headless-contract",
      input: [
        "Verify the vtcode-acp headless contract with two standalone commands:",
        "  cargo check --locked -p vtcode-acp",
        "  cargo tree --locked -p vtcode-acp --prefix none | grep -cE '^(vtcode-ui|ratatui) '",
        "The tree count must be 0 (headless builds skip the TUI subtree entirely). Note: grep exits 1 on zero matches, which is the PASS case here; report the count, not the exit code.",
        "Evidence schema: complete=true only if the check passes AND the count is 0; evidence holds both results; unresolved describes any failure.",
      ].join("\n"),
    });
  }
  if (wantPublish) {
    verifyRequests.push({
      label: "publish-dry-run",
      input: [
        `Verify crates.io packaging for: ${crates.join(", ")}.`,
        "For each crate, run standalone: cargo publish --dry-run -p <crate>",
        "This rebuilds the packaged tarball exactly like `cargo publish` verification; it catches missing files and feature mistakes that workspace builds mask.",
        "Evidence schema: complete=true only if every crate verifies; evidence holds one line per crate with the result; unresolved holds failures with the first error line.",
      ].join("\n"),
    });
  }
  if (wantDesktop) {
    verifyRequests.push({
      label: "desktop-gate",
      input: [
        "Run the desktop CI gate (slow GPUI build): cargo check --locked -p vtcode-desktop -p vtcode-desktop-protocol -p vtcode",
        "Evidence schema: complete=true only on exit 0; evidence holds the result line; unresolved describes any failure.",
      ].join("\n"),
    });
  }
  const verifyResults = await host.parallel(
    verifyRequests.map((r) => ({ input: r.input, schema: evidenceSchema, label: r.label })),
  );
  const gateReports = verifyResults.map((result, i) => compact(result, verifyRequests[i].label));

  const synthesis = await host.agent({
    input: `Synthesize this crate-ops verification into a verdict. Implementation: ${JSON.stringify(implementReport)}. Gates: ${JSON.stringify(gateReports)}. Reply with the evidence schema: complete=true only if implementation completed AND every gate passed; evidence holds one line per gate (PASS/FAIL plus key fact); unresolved holds every open item.`,
    schema: evidenceSchema,
    label: "crate-ops-synthesis",
  });
  const synthesisReport = compact(synthesis, "synthesis");

  const allGatesPass = gateReports.every((g) => g.complete);
  return {
    status: allGatesPass && synthesisReport.complete ? "complete" : "partial",
    verdict: allGatesPass ? "verified" : "failed",
    task,
    crates,
    implementation: implementReport,
    gates: gateReports,
    synthesis: {
      ref: synthesisReport.ref,
      complete: synthesisReport.complete,
      evidence: synthesisReport.evidence,
    },
    unresolved: [
      ...implementReport.unresolved,
      ...gateReports.flatMap((g) => g.unresolved),
      ...synthesisReport.unresolved,
    ],
    evidence: gateReports.flatMap((g) =>
      g.evidence.map((v) => ({ source: g.scope, ref: g.ref, value: v })),
    ),
    notes: [
      ...implementReport.notes,
      ...gateReports.flatMap((g) => g.notes),
      ...synthesisReport.notes,
    ],
  };
}
