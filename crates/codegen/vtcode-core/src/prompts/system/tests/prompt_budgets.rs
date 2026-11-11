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

// Includes direct patch calls and one bounded context-mismatch recovery read.
// Raised from 1210: the delegation rule now names the three keep-conditions
// (parallel, discardable fresh-context, tool-choice conflicts) so subagents
// fold into skills and tool search by default.
// Raised for the partial-commit and conflict-reload runtime guidance (measured 1257).
// Raised for the name-the-missing-fact inspection guidance (measured 1298).
pub(super) const DEFAULT_STATIC_PROMPT_MAX_TOKENS_U64: u64 = 1330;

// Raised from 400: the shared runtime guidance is now full sentences with reasons.
// Includes direct patch calls and one bounded context-mismatch recovery read.
// Raised from 665 for the reuse-reads runtime guidance bullet (measured 688).
// Raised to 725 for the hypothesis-loop runtime bullet (measured 710).
// Raised to 890 for the fold-by-default delegation rule (measured 878).
// Raised for the partial-commit and conflict-reload runtime guidance (measured 920).
// Raised for the name-the-missing-fact inspection guidance (measured 961).
pub(super) const MINIMAL_STATIC_PROMPT_MAX_TOKENS_U64: u64 = 1000;

// Default live prompt plus the instruction appendix (`# INSTRUCTIONS`).
// Raised from 1250 for the reuse-reads runtime guidance bullet (measured 1284).
// Raised to 1320 for the hypothesis-loop runtime bullet (measured 1306).
// Named HOVER guidance measures 1321 tokens.
// Raised for the partial-commit and conflict-reload runtime guidance (measured 1516).
// Raised for the name-the-missing-fact inspection guidance (measured 1557).
// Raised for the login-shell dialect guidance (measured 1606).
pub(super) const DEFAULT_LIVE_PROMPT_WITH_INSTRUCTIONS_MAX_TOKENS_U64: u64 = 1640;

// --- Composed prompts (`estimate_tokens`, usize) ---

// Raised to 1220 for the fold-by-default delegation rule.
// Raised for the partial-commit and conflict-reload runtime guidance
// (measured 1182 minimal).
// Raised for the name-the-missing-fact inspection guidance (measured 1223 minimal).
// Raised for the login-shell dialect guidance (measured 1272 minimal).
pub(super) const COORDINATOR_MINIMAL_MAX_TOKENS_USIZE: usize = 1310;

// Loose cap for non-minimal coordinator prompts; not yet churned by guidance edits.
pub(super) const COORDINATOR_OTHER_MODES_MAX_TOKENS_USIZE: usize = 2000;

// Planning fixture with fixed tool catalog and runtime sections.
// Retain the canonical output contract and HOVER guidance. Before the tracker
// examples, Default/Minimal measured 9847/10180 bytes and 2107/2205 tokens.
// Source inspection adds one shared runtime sentence (about 60 tokens).
// Fresh completion guidance adds 248 bytes and about 45 estimated tokens.
// Zero-on-success planning/runtime guidance adds 253 bytes and about 51 tokens.
// Action-specific tracker examples measure 9997 bytes/2146 tokens for Default
// and 10269 bytes/2216 tokens for Minimal; keep tight caps and presence checks.
// Cancelled planning resumes on fresh input; retain a 50-byte allowance for this policy.
// Terminal planning guidance adds 128 bytes and about 30 estimated tokens.
// Partial-commit and conflict-reload runtime guidance adds 229 bytes and
// about 45 estimated tokens (Default measures 11328 bytes/2398 tokens,
// Minimal measures 11600 bytes/2468 tokens).
// Scoped-verify planning guidance adds 149 bytes and about 35 estimated
// tokens (Default measures 11477 bytes/2433 tokens, Minimal measures
// 11749 bytes/2503 tokens).
// Raised for the name-the-missing-fact inspection guidance (measured 11685
// bytes/2474 tokens Default, 11957 bytes/2544 tokens Minimal).
// Raised for the login-shell dialect guidance (measured 11916 bytes
// Default, 12309 bytes Minimal).
pub(super) const PLANNING_FIXTURE_DEFAULT_MAX_BYTES: usize = 12_100;
pub(super) const PLANNING_FIXTURE_DEFAULT_MAX_TOKENS_USIZE: usize = 2_550;
pub(super) const PLANNING_FIXTURE_MINIMAL_MAX_BYTES: usize = 12_500;
pub(super) const PLANNING_FIXTURE_MINIMAL_MAX_TOKENS_USIZE: usize = 2_620;

// --- Composed prompt byte smoke checks (`len()`, usize; tokens stay authoritative) ---

// Compared with `<` at the call site.
// Includes direct patch calls and bounded context-mismatch recovery.
// Raised from 3700 for the reuse-reads and exact-whitespace patch-copy guidance (measured 3917).
// Raised from 4700 for the fold-by-default delegation rule (measured 4719).
// Raised for the partial-commit and conflict-reload runtime guidance (measured 4948).
// Raised for the name-the-missing-fact inspection guidance (measured 5156).
// Raised for the login-shell dialect guidance (measured 5387).
pub(super) const MINIMAL_COMPOSED_MAX_BYTES: usize = 5600;

// Includes fresh completion evidence guidance (+248 bytes, about 45 tokens).
// Generated verifier success guidance adds 114 bytes (about 25 tokens).
// Includes patch recovery guidance; token tests stay authoritative.
// Raised from 5400 for the reuse-reads runtime guidance bullet (measured 5496).
// Named HOVER guidance measures 5616 bytes; retain a narrow byte allowance.
// Raised from 6350 for the fold-by-default delegation rule (measured 6375).
// Raised for the partial-commit and conflict-reload runtime guidance (measured 6604).
// Raised for the name-the-missing-fact inspection guidance (measured 6812).
// Raised for the login-shell dialect guidance (measured 7043).
pub(super) const DEFAULT_COMPOSED_MAX_BYTES: usize = 7230;

// Compared with `<` at the call site.
// Includes fresh completion evidence guidance (+248 bytes, about 45 tokens).
// Generated verifier success guidance adds 114 bytes (about 25 tokens).
// Includes patch recovery guidance; token tests stay authoritative.
// Raised from 4800 for the reuse-reads runtime guidance bullet (measured 4862).
// Raised for the partial-commit and conflict-reload runtime guidance (measured 5970).
// Raised for the name-the-missing-fact inspection guidance (measured 6178).
// Raised for the login-shell dialect guidance (measured 6409).
pub(super) const LIGHTWEIGHT_COMPOSED_MAX_BYTES: usize = 6600;

// Includes fresh completion evidence guidance (+248 bytes, about 45 tokens).
// Generated verifier success guidance adds 114 bytes (about 25 tokens).
// Includes patch recovery guidance; token tests stay authoritative.
// Raised from 5400 for the reuse-reads runtime guidance bullet (measured 5569).
// Raised for the partial-commit and conflict-reload runtime guidance (measured 6677).
// Raised for the name-the-missing-fact inspection guidance (measured 6885).
// Raised for the login-shell dialect guidance (measured 7116).
pub(super) const SPECIALIZED_COMPOSED_MAX_BYTES: usize = 7300;
