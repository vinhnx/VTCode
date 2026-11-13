//! Single home of prompt size budgets for the system-prompt tests.
//!
//! Every byte/token cap that composed or static prompts must fit lives here as
//! a named constant, so a deliberate prose change touches one number instead of
//! one literal per test file. The call sites keep their own comparison
//! operator (`<` vs `<=`) and failure message; only the number is shared.
//!
//! Convention when raising a cap: run the failing test, read the measured
//! value from its message, round up with headroom matching the neighboring
//! caps, and append a `Raised for ... (measured N)` line to the constant's
//! history below. Prose itself is pinned by the
//! `contains` tests in `runtime_guidance.rs`, and the byte-identical goldens
//! assemble from `RUNTIME_GUIDANCE_SECTION`, so a prose tweak touches the
//! guidance source, its `RUNTIME_GUIDANCE_MAX_ESTIMATED_TOKENS` budget when
//! net-new text exceeds it (enforced by an `assert!` in the composition path,
//! so every prompt test fails until it is raised), and the affected caps here.
//!
//! Suffixes encode the unit and integer type: token caps come in `u64`
//! (`estimate_token_count`) and `usize` (`estimate_tokens`) flavors because
//! the two estimators return different types. Do not unify them with casts.

// --- Static prompts (`estimate_token_count`, u64) ---

// The universal guidance is one rule per bullet, the contract lines no
// longer restate runtime-guidance rules, and the tool schemas own per-tool
// mechanics. Measured after the schema-dedup rewrite: Minimal 457, Default 734.
pub(super) const DEFAULT_STATIC_PROMPT_MAX_TOKENS_U64: u64 = 780;

// Measured after the schema-dedup rewrite: Minimal 457.
pub(super) const MINIMAL_STATIC_PROMPT_MAX_TOKENS_U64: u64 = 500;

// Default live prompt plus the instruction appendix (`# INSTRUCTIONS`).
// Measured after the schema-dedup rewrite: Default 1009.
pub(super) const DEFAULT_LIVE_PROMPT_WITH_INSTRUCTIONS_MAX_TOKENS_U64: u64 = 1_050;

// --- Composed prompts (`estimate_tokens`, usize) ---

// Measured after the schema-dedup rewrite: Minimal 725.
pub(super) const COORDINATOR_MINIMAL_MAX_TOKENS_USIZE: usize = 780;

// Non-minimal coordinator prompt. Measured after the schema-dedup rewrite:
// Default 1002.
pub(super) const COORDINATOR_OTHER_MODES_MAX_TOKENS_USIZE: usize = 1_080;

// Planning fixture with fixed tool catalog and runtime sections.
// Measured after the schema-dedup rewrite: Default 9067 bytes / 1908 tokens.
pub(super) const PLANNING_FIXTURE_DEFAULT_MAX_BYTES: usize = 9_400;
pub(super) const PLANNING_FIXTURE_DEFAULT_MAX_TOKENS_USIZE: usize = 1_980;
// Minimal density adds the planning-mode tool guidance, so it is the larger
// of the two. Measured after the schema-dedup rewrite: 9399 bytes / 1995 tokens.
pub(super) const PLANNING_FIXTURE_MINIMAL_MAX_BYTES: usize = 9_800;
pub(super) const PLANNING_FIXTURE_MINIMAL_MAX_TOKENS_USIZE: usize = 2_080;

// --- Composed prompt byte smoke checks (`len()`, usize; tokens stay authoritative) ---

// Measured after the schema-dedup rewrite: Minimal 4342.
pub(super) const MINIMAL_COMPOSED_MAX_BYTES: usize = 4_600;

// Measured after the schema-dedup rewrite: Default 5700.
pub(super) const DEFAULT_COMPOSED_MAX_BYTES: usize = 5_900;

// Measured after the schema-dedup rewrite: Lightweight 5185.
pub(super) const LIGHTWEIGHT_COMPOSED_MAX_BYTES: usize = 5_400;

// Measured after the schema-dedup rewrite: Specialized 5697.
pub(super) const SPECIALIZED_COMPOSED_MAX_BYTES: usize = 5_900;
