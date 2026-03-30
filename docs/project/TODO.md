NOTE: use deepwiki mcp to reference from codex https://deepwiki.com/openai/codex

---

extract and open source more components from vtcode-core

---

CODEX plus

main account
kiweuro
writedownapp
humidapp
vtchat.io

---

https://code.claude.com/docs/en/headless

---

hooks

https://developers.openai.com/codex/hooks

https://deepwiki.com/search/how-does-hooks-works-in-codex_68383f0e-ec03-44eb-be92-69a26aa3d1e1?mode=fast

https://code.claude.com/docs/en/hooks

==

plugins and LSP

https://code.claude.com/docs/en/discover-plugins

https://code.claude.com/docs/en/plugins-reference

https://developers.openai.com/codex/plugins

https://deepwiki.com/search/httpsdevelopersopenaicomcodexp_ee8404d4-ca94-48ac-9fad-60e24e3b4f5a?mode=fast

---

High-value Rust codemods to build for VT Code (and the broader ecosystem):

Codemod
Effort
Credit
tokio 1.x → 2.x migration
M
$200
clap v3 → v4 derive API
S
$100
ratatui breaking changes (VT Code uses this)
S–M
$100–200
serde attribute renames / deprecations
S
$100
anyhow / thiserror v1 → v2
S
$100
reqwest breaking changes
S
$100
hyper 0.14 → 1.0 (massive pain point)
L
$400
actix-web v3 → v4
M
$200
sqlx 0.7 → 0.8
M
$200
tree-sitter API changes (VT Code uses this)
S
$100

Being first to publish any quality Rust codemod also positions you for the $2,000 framework adoption tier — e.g., getting ratatui or tokio maintainers to reference your codemod in their upgrade guides.

---

use https://github.com/Uzaaft/libghostty-rs/ replace existing libghostty-vt impl.

---

PLAN mode: handle interactive model picker for /subagent create/edit/udpate interactively instead of requiring manual YAML editing. This is a common source of friction for users creating new subagents, and the current YAML editing approach is error-prone and not user-friendly.

---

OpenAI has a standalone compaction endpoint that's incredibly customizable and is disconnected from their responses API.
https://developers.openai.com/api/reference/resources/responses/methods/compact

---

checking modal blocking, when an inline modal is showing (eg: the /login modal), currently the whole UI is not selecteable, copyable, or interactable. This is a problem because users may want to copy text from the UI while the modal is open, or interact with other parts of the UI without closing the modal. meaining currently all the modal inline is blocking all interactions with the UI, which is not ideal for user experience. We should make it so that when an inline modal is open, only the modal itself is blocking interactions, while the rest of the UI remains selectable and interactable. This way users can still copy text or interact with other parts of the UI without having to close the modal first.
