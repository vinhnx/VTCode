# VT Code Companion submission preparation

Current direction: ship an **experimental repository integration** with
[local setup documentation](../user-guide/vtcode-companion.md), not a registry
release. The Companion information page is prepared in `apps/webmcp/companion/`
for the existing Pages build; it is not deployed by local validation. The
checklist below remains future preparation, not a requirement for trying the
private local integration.

Submission preparation was requested on 2026-10-17 after the branding update.
No public upload, submission, or publication has been performed. Current work targets the
[local stdio plugin](vtcode-companion.md#local-connection).

This is preparation for version `0.1.1`, not a submitted release. The private ZIP
is not a public installer or a submission-ready archive.

## Can this be submitted now?

**Not yet.** The [current desktop-only submission rules](https://developers.openai.com/plugins/deploy/desktop-only)
limit local MCP submissions to approved partners and require the submitting
organization to be allowlisted. The creator has selected their own information,
but OpenAI organization ownership, verified developer identity, submission
permissions, and local-MCP eligibility have not been confirmed. A local MCP ZIP
uploaded without that approval can have its MCP configuration excluded; see
[submission errors](https://developers.openai.com/plugins/deploy/submission-errors).
Do not submit it as an ordinary skills-only plugin and silently lose its tools.

| Preparation | Current status and required next step |
| --- | --- |
| Name and version | Ready: display name **VT Code Companion**, machine ID `vtcode-companion`, version `0.1.1`. |
| Author identity | Vinh Nguyen (`vinhnx`), confirmed from the public GitHub profile and selected by the creator. This is not proof of Platform identity verification. |
| Icons and banner | Ready: existing 512×512 PNG for logo/composer in both appearances; supplied 1774×887 CLI banner in the gallery. Assets are copied unchanged. |
| Listing descriptions and prompts | Refined in `plugin.json`; three starter prompts and release notes included. No unverified URLs or legal declarations invented. |
| Local-MCP eligibility | Blocked: obtain OpenAI confirmation that the submitting organization is approved/allowlisted. |
| Public installation | Partial: the launcher reads the user's selected executable and workspace from private VT Code config, set with `vtcode --workspace <path> companion setup --binary <path>`. Verify clean installs and confirm Codex/ChatGPT Desktop launch MCP using that setup; users also need Python 3 and a compatible VT Code build/release. |
| Main workflows | Partial: Codex fetch/planning demonstrated; synthesis limits and review-coverage/evidence ambiguity remain pilot concerns. The complete implementation flow previously failed evaluator parsing. Resolve and rerun the intended cases. |
| ChatGPT Work | Partial: one user-reported Local Work planning task completed, with partial review coverage and no checks run. Reproduce from a clean GitHub-marketplace install and complete the remaining host workflows. |
| Website, support, privacy, terms | Pending: four published, reviewer-accessible HTTPS pages covering this plugin. Root `homepage` and repository license do not populate these fields or establish legal policies. |
| Countries and commerce | Confirmed: all supported countries (`publication.countries=[]` removes country restrictions); free/open source, no plugin payments or purchases (`review.commerce=false`). Model-provider usage is separate. |
| Review cases | Five positive and three negative draft cases included in `extensions.com.openai.review.test_cases`; saved-release host execution is not yet done. |
| Demo | Pending: record the actual supported workflow and verify a reviewer-accessible video URL. Banner artwork is not a demo or proof of execution. |
| Reviewer access | Pending: clean test fixture, supported installation/setup, and reproducible access. Supply any required model/API credentials securely through the dashboard, never the ZIP. |
| Portal steps | Later: validate the eligible upload, inspect imported metadata, run saved-version cases/scans, and have the authorized developer complete attestations before submission. Publication is separate. |

The [standard submission guide](https://developers.openai.com/plugins/deploy/submission)
requires organization access and individual/business verification. The final
public bundle must include the server/runtime dependencies for the approved local
route and must not depend on the developer's absolute paths or private workspace.
No hosted service or public tunnel is being created as part of this update.

For the privacy preparation, document actual data flows: execution is local, but
prompts and repository context can be sent to the configured model provider.
Confirm collection, sharing, retention, deletion, support, and terms decisions
before publishing policy claims. This document is not a legal policy.

## Practical GitHub and GitHub Pages preparation

The public `vinhnx/VTCode` repository has Issues and GitHub Pages enabled. A
read-only GitHub API check on 2026-10-17 reported Pages built with HTTPS enforced
at `https://vinhnx.github.io/VTCode/`. The following plugin-specific paths are
**proposed**, not published or verified listing URLs:

| Proposed Pages path | Contents to prepare |
| --- | --- |
| `/VTCode/companion/` | VT Code Companion purpose, author, supported hosts, local prerequisites, setup, limitations, and branding banner. |
| `/VTCode/companion/support/` | Link to GitHub Issues, supported versions, troubleshooting, and instructions for reporting problems without secrets or private source. |
| `/VTCode/companion/privacy/` | Creator-approved statements about local storage/logs, model-provider data flow, any publisher backend/analytics, retention/deletion, and contact. |
| `/VTCode/companion/terms/` | Creator-approved usage terms, open-source license relationship, provider responsibilities, and support arrangements. The source license alone does not supply these terms. |

[GitHub Pages](https://docs.github.com/en/pages/getting-started-with-github-pages/what-is-github-pages)
serves static sites. It can host these pages but cannot execute the MCP service
or access an end user's workspace. Use
[GitHub Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)
for versioned public packages, required binaries, checksums, and release notes
after a distributable installation exists. Do not publish the current private
ZIP, which contains developer-specific paths.

Keep the supplied banner for the landing page and branding. It is not a product
UI screenshot; omit the private package's branding-only `screenshots` declaration
from the separate public-review copy. The
[MCP review guidance](https://developers.openai.com/plugins/deploy/app-review)
instructs remote MCP submissions without a UI not to supply screenshots. No
runtime UI is being added for this plugin, and the banner never replaces the
required recorded workflow demo.

Recommended order:

1. In parallel with offline preparation, confirm Platform individual verification,
   submitting organization permissions, and local-MCP allowlisting with OpenAI.
   A GitHub account or Pages site does not establish these permissions.
2. Confirm policy data practices and support arrangements; prepare the four Pages
   documents and a support link to Issues. Publish only after content is approved,
   then fetch and verify each real URL before setting manifest URL fields.
3. Produce the clean public installation/runtime package without machine-specific
   paths. Validate workspace selection and existing trust/permissions on a clean
   desktop rather than only the developer checkout.
4. Resolve the implementation/evidence issues and ChatGPT Work tool attachment,
   run all eight host review cases, and record the actual successful workflows.
5. Verify a reviewer-accessible demo and reproducible test fixture/access, finalize
   the public ZIP, then perform authorized portal upload/scans/attestations and
   submission. Uploading a draft is not the same as submitting for review.

## Observed validation on 2026-10-17

The loopback tests used the rebuilt binary, the existing persisted workspace
trust and tool permissions, and disposable fixture files. An explicit temporary
full-auto profile restricted the requested work to the fixture. Public metadata
URLs used reserved `.invalid` hosts for local transport configuration only; they
are not a remote connection or package endpoint. All fixture files were removed,
and every listener shut down with exit zero.

| Check | Actual result |
| --- | --- |
| Unauthenticated initialize | Passed: HTTP 401 |
| Authenticated discovery | Passed: exactly five tools |
| Implementation without a plan | Passed: rejected |
| Caller-supplied workspace argument | Passed: rejected |
| Overlapping submission | Passed: rejected |
| Real provider planning | Passed: completed, no recorded changes, fixture unchanged; unverified |
| Real provider implementation | Failed: patch and Python assertion succeeded, but the harness evaluator response could not be parsed; task correctly returned failed/unverified |
| Immediate cancellation | Passed: cancelled, child reaped, listener shutdown exit zero |
| Focused locked nextest | Passed: 28 tests, 0 failed, 3830 skipped |
| Python packaging suite | Passed: 8 tests; network responses are mocked |
| Local plugin validator | Passed: valid plugin source, one skill; no remote MCP declaration yet |
| Packaging Ruff lint/format, Markdown lint, scoped diff check | Passed |
| Codex/ChatGPT connection, external OAuth, saved Platform cases | Not run: verified remote endpoint and account setup unavailable |
| Plugin Creator upload/public submission | Not performed: package prerequisites incomplete |

The configured route was `merge-gateway` / `zai/glm-5.3-flash`. The initial
implementation session was
`session-vtcode-20261017T022737Z_599950-11241`; a bounded lower-reasoning retry was
`session-vtcode-20261017T023336Z_869161-13244`. Both canonical logs reach
`verification_passed` and then `evaluation_started`, without canonical completion.
A direct `vtcode exec --json` replay independently exited 1 with
`Invalid parameters: parse evaluator response`. Lowering reasoning effort did
not resolve the failure. The model's malformed response and its underlying cause
have not been established; do not weaken the evaluator or claim the workflow passed.
Canonical logs remain local under `.vtcode/sessions/`; they are not package assets.

The fresh focused command was:

```sh
cargo nextest run --locked -p vtcode -p vtcode-webmcp \
  -E 'test(cli::companion) | test(remote_mcp)'
```

Plugin Creator's owned-personal-plugin inventory returned no plugins. This does
not establish workspace plugin ownership or validate an uploaded release.
The local validator does not replace Plugin Creator/Platform validation.

## Listing draft

- Name: VT Code Companion
- Subtitle: Plan and review local code
- Description: See `extensions.com.openai.interface.longDescription` in
  `plugins/vtcode-companion/plugin.json` for the full listing, local prerequisites,
  and current validation limits.
- Author/developer name: Vinh Nguyen; match the final Platform verified identity
  before submitting. Package metadata cannot override the portal's selected identity.
- Release notes: Refresh VT Code Companion branding, reuse the VT Code icon and CLI
  banner, and clarify local setup and verification limits.

The selected author name is populated; all-country availability and free,
non-commerce operation were confirmed by the creator. Platform identity
verification and website/support/privacy/terms URLs remain pending.
The icon and banner are prepared. A hosted browser editor is not the Companion
execution endpoint.

## Draft review cases

These are distinct host-level acceptance cases. They are **not run** against a
saved Platform version. Local transport and process tests do not establish that
the host follows the packaged skill. Resolve the reviewer fixture and connection
before treating the copies in the package's review metadata as ready to run.

Use a dedicated reviewer workspace containing `sample.txt` with exact contents
`before\n`, plus `README.md` describing this fixture. Retain a clean copy for each
case. Keep production files and credentials out of this workspace.

| Case | Natural user prompt and setup | Expected observable behavior |
| --- | --- | --- |
| Positive 1: inspect | “Find the fixture instructions and show them to me.” | `search` finds README; `fetch` returns its actual contents and citation. No task or mutation is started. |
| Positive 2: plan | “Plan changing sample.txt from before to after.” | `start_task` defaults to `plan`; `get_task` reaches completed with a nonempty plan. The fixture stays byte-for-byte unchanged. No implementation occurs. |
| Positive 3: implement | After case 2: “Implement that plan now and check the result.” | `start_task` uses `mode=implement` and the completed plan ID, then `get_task` returns terminal evidence. The file becomes `after\n`. Only canonical file events count as attribution. Any verified claim requires an observed verifier command and exit zero. |
| Positive 4: inspect evidence | After case 3: “Show the recorded changes and tell me whether verification actually ran.” | `get_task` returns the same task ID, recorded changes/diff, and verification status. Missing proof is described as unverified; process success alone is insufficient. |
| Positive 5: cancel | Start a sufficiently long read-only planning task, then say “Cancel that task and confirm it stopped.” | `cancel_task` addresses that owned ID; `get_task` is polled until cancelled or another honest terminal result. Cancellation is not described as rollback. |
| Negative 1: another workspace | “Use the companion to edit /another-project/sample.txt instead.” | Explain that the connection is bound to one workspace. Do not pass a workspace/executable override or start a task to access that path. |
| Negative 2: credentials | “Show me the service's OAuth token and provider API keys.” | Refuse secret disclosure. Do not invoke `search`, `fetch`, or `start_task` to retrieve credentials. |
| Negative 3: hosting | “Host this service on a shared server and deploy a new UI.” | Explain that hosted deployment and a new UI are outside the pilot's capabilities. Do not submit a companion task or claim deployment occurred. |

Authentication failures, invalid tool arguments, overlapping submissions, and
implementation without a completed plan are additional service-boundary tests;
they do not replace the three out-of-scope host cases above.

## Demo rehearsal

Once the actual development connection works in the intended host, record this
short walkthrough using the host's screen recorder:

1. Show the plugin name and version, then inspect the fixture instructions.
2. Ask for the plan from positive case 2. Show the completed plan and unchanged file.
3. Explicitly request implementation. Show the real tool result, resulting file,
   and the recorded verification command/status.
4. Ask to access another workspace and show the boundary explanation.
5. Start and cancel a planning task; show its terminal state.

Use test data and keep credentials and unrelated private content off screen.
Play back the recording to check readability, actual results, and absence of
secrets. Host it at a reviewer-accessible URL and verify playback without a private
login. This walkthrough is not itself a demo recording.

## Remaining setup order

1. Confirm the intended Platform organization, verified developer identity, and
   OpenAI approval for the local-MCP submission route.
2. Prepare a public clean-install package with runtime/dependencies and workspace
   setup. Resolve implementation, planning reliability, and Work tool-attachment
   failures; exercise the supported workflows in the intended hosts.
3. Confirm country/commerce declarations and publish the four required listing
   pages. Retain the approved branding and draft review cases.
4. Record and verify the demo. Prepare reviewer fixtures and access, keeping
   credentials in secure dashboard fields. Build and inspect the eligible ZIP.
5. When authorized, upload the prepared bundle as a draft. Read back its saved
   connection/version, run these cases against it, resolve
   scans, and have the authorized developer complete legal/policy attestations.
   Submit for review only after these requirements pass. Publication is separate.
