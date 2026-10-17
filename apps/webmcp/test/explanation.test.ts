import test, { type TestContext } from "node:test";
import assert from "node:assert/strict";
import { isEvidencePage, isExplanationPage, loadEvidence, loadExplanation, mergeExplanationPages, mountExplanation, type EvidenceRef, type ExplanationBackend, type ExplanationEntry, type ExplanationModel, type ExplanationPage, type TokenBreakdownEntry } from "../src/explanation.ts";
import { validateResponsePayload } from "../src/protocol.ts";
import type { StatusPayload } from "../src/types.ts";
const reference: EvidenceRef = { session_id: "session", offset: 0, length: 100, digest: "a".repeat(64), item_id: "item" };
const entry = (index = 0): ExplanationEntry => ({ label: `Action ${index}`, status: "completed", evidence: { ...reference, item_id: `item-${index}` },
    task_id: "task", actor_id: null, parent_actor_id: null, timestamp: null, path: null, line: null });
const model = (): ExplanationModel => ({ session_id: "session", revision: "b".repeat(64), scope: "task", task_id: "task", status: "completed",
    goals: [], actions: [], changes: [], edit_operations: 0, decisions: [], verification: [], failures: [], review_priorities: [],
    plan_evolution: [], graph: [], usage: null, cost_usd: null,
    completeness: { malformed_records: 0, unknown_records: 0, legacy_records: 0, evicted_turns: 0, warnings: [] } });
function page(actions: ExplanationEntry[] = [], offset = 0): ExplanationPage {
    return { model: { ...model(), actions: actions.slice(offset, offset + 32) }, offset,
        next_offset: offset + 32 < actions.length ? offset + 32 : null };
}
function backend(overrides: Partial<ExplanationBackend> = {}): ExplanationBackend {
    return { explanation: async () => page(), explanationEvidence: async () => ({ reference, text: "record", offset: 0, next_offset: null, total_bytes: 6 }),
        explanationNavigate: async () => ({ focused: true }), ...overrides };
}
function tokenEntry(index = 0): TokenBreakdownEntry {
    return { fact: { ...entry(index), label: `Request prefix ${index}` }, breakdown: {
            system_prompt_tokens: 10, tool_schema_tokens: 20, instruction_file_tokens: 30, message_history_tokens: 40,
            cache_read_tokens: 50, cache_write_tokens: 60, cache_miss_tokens: 70,
        } };
}
function tokenPage(entries: TokenBreakdownEntry[], offset = 0): ExplanationPage {
    return { ...page(), model: { ...model(), token_breakdowns: entries.slice(offset, offset + 32) }, offset,
        next_offset: offset + 32 < entries.length ? offset + 32 : null };
}
test("token breakdowns accept legacy absence and reject malformed counters, facts and collections", () => {
    assert.equal(isExplanationPage(page()), true);
    assert.equal(isExplanationPage(tokenPage([tokenEntry()])), true);
    for (const key of [...Object.keys(tokenEntry().breakdown), "subagent_bootstrap_tokens"]) {
        for (const value of [-1, 1.5, Number.MAX_SAFE_INTEGER + 1, "10", null, undefined]) {
            const invalid = tokenPage([tokenEntry()]);
            (invalid.model.token_breakdowns![0]!.breakdown as unknown as Record<string, unknown>)[key] = value;
            assert.equal(isExplanationPage(invalid), false, `${key}: ${String(value)}`);
            assert.throws(() => validateResponsePayload("explanation.get", invalid), /invalid WebMCP frame/);
        }
    }
    const invalidSession = tokenPage([tokenEntry()]);
    invalidSession.model.token_breakdowns![0]!.fact.evidence.session_id = "other-session";
    assert.equal(isExplanationPage(invalidSession), false);
    const tooMany = tokenPage([]);
    tooMany.model.token_breakdowns = Array.from({ length: 33 }, (_, index) => tokenEntry(index));
    assert.equal(isExplanationPage(tooMany), false);
    const bootstrap = tokenPage([tokenEntry()]);
    bootstrap.model.token_breakdowns![0]!.breakdown.subagent_bootstrap_tokens = 0;
    assert.equal(isExplanationPage(bootstrap), true);
});
test("token-only pages merge and reject missing fields or exhausted-collection reappearance", async () => {
    const tokens = Array.from({ length: 65 }, (_, index) => tokenEntry(index));
    const offsets: number[] = [];
    const result = await loadExplanation(backend({ explanation: async (_scope, offset = 0) => { offsets.push(offset); return tokenPage(tokens, offset); } }), "task");
    assert.equal(result.token_breakdowns?.length, 65);
    assert.equal(result.token_breakdowns?.at(-1)?.fact.label, "Request prefix 64");
    assert.deepEqual(offsets, [0, 32, 64]);
    const missing = tokenPage(tokens, 32);
    delete missing.model.token_breakdowns;
    missing.model.actions = Array.from({ length: 32 }, (_, index) => entry(index));
    assert.throws(() => mergeExplanationPages([tokenPage(tokens), missing, tokenPage(tokens, 64)]), /changed/);
    const actions = Array.from({ length: 65 }, (_, index) => entry(index));
    const first = page(actions);
    first.model.token_breakdowns = [tokenEntry()];
    const later = page(actions, 32);
    later.model.token_breakdowns = [tokenEntry(1)];
    const last = page(actions, 64);
    last.model.token_breakdowns = [];
    assert.throws(() => mergeExplanationPages([first, later, last]), /pagination/);
});
test("adaptive page widths preserve exact cursor coverage and exhausted collection boundaries", async () => {
    const actions = Array.from({ length: 35 }, (_, index) => entry(index));
    const offsets: number[] = [];
    const result = await loadExplanation(backend({ explanation: async (_scope, offset = 0) => {
            offsets.push(offset);
            const count = offset === 0 ? 16 : offset === 16 ? 8 : 2;
            return { model: { ...model(), actions: actions.slice(offset, offset + count) }, offset,
                next_offset: offset + count < actions.length ? offset + count : null };
        } }), "task");
    assert.equal(result.actions.length, 35);
    assert.deepEqual(offsets, [0, 16, 24, 26, 28, 30, 32, 34]);
    const first = { ...page(), model: { ...model(), actions: actions.slice(0, 16), token_breakdowns: [tokenEntry()] }, next_offset: 16 };
    const later = { ...page(), model: { ...model(), actions: actions.slice(16, 24), token_breakdowns: [tokenEntry(1)] }, offset: 16, next_offset: 24 };
    const last = { ...page(), model: { ...model(), actions: actions.slice(24), token_breakdowns: [] }, offset: 24 };
    assert.throws(() => mergeExplanationPages([first, later, last]), /pagination/);
});
test("explanation validators reject malformed collection and numeric boundaries", () => {
    assert.equal(isExplanationPage(page([entry()])), true);
    for (const mutate of [
        (p: ExplanationPage) => { p.model.actions[0]!.evidence.digest = "bad"; },
        (p: ExplanationPage) => { p.model.actions[0]!.evidence.offset = Number.MAX_SAFE_INTEGER; },
        (p: ExplanationPage) => { p.model.actions[0]!.evidence.session_id = ""; },
        (p: ExplanationPage) => { p.model.cost_usd = Infinity; },
        (p: ExplanationPage) => { p.model.actions = Array.from({ length: 33 }, () => entry()); },
    ]) {
        const invalid = page([entry()]);
        mutate(invalid);
        assert.equal(isExplanationPage(invalid), false);
        assert.throws(() => validateResponsePayload("explanation.get", invalid), /invalid WebMCP frame/);
    }
});
test("workspace snapshots refresh independently of cached canonical facts and reject hostile shapes", async () => {
    const snapshot = { captured_at: "2026-10-03T00:00:00Z", text: "+human edit", truncated: false, note: "Ownership is not attributed to the agent." };
    const current = { ...page(), workspace_diff: snapshot };
    assert.equal(isExplanationPage(current), true);
    const cached = model();
    let received: unknown;
    const result = await loadExplanation(backend({ explanation: async () => current }), "task", () => true, cached, value => { received = value; });
    assert.equal(result, cached);
    assert.deepEqual(received, snapshot);
    assert.equal(result.changes.length, 0);
    for (const workspace_diff of [null, { ...snapshot, text: "界".repeat(3000) }, { ...snapshot, truncated: "false" }, { ...snapshot, text: { html: "<script>" } }]) {
        assert.equal(isExplanationPage({ ...page(), workspace_diff }), false);
    }
    assert.equal(isExplanationPage({ ...page(), offset: 32, workspace_diff: snapshot }), false);
});
test("explanation merging requires complete contiguous pages from one snapshot", () => {
    const actions = Array.from({ length: 65 }, (_, index) => entry(index));
    const pages = [page(actions), page(actions, 32), page(actions, 64)];
    assert.equal(mergeExplanationPages(pages).actions.length, 65);
    assert.throws(() => mergeExplanationPages(pages.slice(0, 2)), /incomplete/);
    assert.throws(() => mergeExplanationPages([pages[0]!, pages[2]!]), /changed/);
    assert.throws(() => mergeExplanationPages([page(), page()]), /changed/);
    for (const mutate of [
        (p: ExplanationPage) => { p.model.revision = "c".repeat(64); },
        (p: ExplanationPage) => { p.model.task_id = "other-task"; },
        (p: ExplanationPage) => { p.model.status = "failed"; },
        (p: ExplanationPage) => { p.model.actions[0]!.evidence.session_id = "other-session"; },
        (p: ExplanationPage) => { p.next_offset = 63; },
    ]) {
        const invalid = structuredClone(pages);
        mutate(invalid[1]!);
        assert.throws(() => mergeExplanationPages(invalid));
    }
});
test("invalid cursors stop requests immediately and superseded queries stop pagination", async () => {
    let calls = 0;
    const actions = Array.from({ length: 33 }, (_, index) => entry(index));
    const source = backend({ explanation: async () => { calls += 1; return { ...page(actions), next_offset: 1 }; } });
    await assert.rejects(loadExplanation(source, "task"), /pagination/);
    assert.equal(calls, 1);
    calls = 0;
    const stale = backend({ explanation: async () => { calls += 1; return page(actions); } });
    await assert.rejects(loadExplanation(stale, "task", () => calls === 0), /superseded/);
    assert.equal(calls, 1);
    await assert.rejects(loadExplanation(backend(), "session"), /scope/);
});
test("10,000-entry histories merge completely within the documented bound", async () => {
    const actions = Array.from({ length: 10000 }, (_, index) => entry(index));
    const offsets: number[] = [];
    const result = await loadExplanation(backend({ explanation: async (_scope, offset = 0) => { offsets.push(offset); return page(actions, offset); } }), "task");
    assert.equal(result.actions.length, 10000);
    assert.equal(offsets.length, 313);
    assert.equal(result.actions.at(-1)!.label, "Action 9999");
    offsets.length = 0;
    const reused = await loadExplanation(backend({ explanation: async (_scope, offset = 0) => { offsets.push(offset); return page(actions, offset); } }), "task", () => true, result);
    assert.equal(reused, result);
    assert.deepEqual(offsets, [0]);
    const oversized = Array.from({ length: 10001 }, (_, index) => entry(index));
    await assert.rejects(loadExplanation(backend({ explanation: async (_scope, offset = 0) => page(oversized, offset) }), "task"), /pagination/);
});
test("derived graphs can exceed retained event count while requests remain bounded", async () => {
    const graph = Array.from({ length: 19997 }, (_, index) => ({ from: `event-${index}`, to: `file-${index}.rs`, relation: "changed file" }));
    let calls = 0;
    const result = await loadExplanation(backend({ explanation: async (_scope, offset = 0) => {
        calls += 1;
        return { ...page(), model: { ...model(), graph: graph.slice(offset, offset + 32) }, offset, next_offset: offset + 32 < graph.length ? offset + 32 : null };
    } }), "task");
    assert.equal(result.graph.length, 19997);
    assert.equal(result.graph.at(-1)?.to, "file-19996.rs");
    assert.equal(calls, 625);
    assert.equal(isExplanationPage({ ...page(), offset: 30000, next_offset: 30001 }), false);
});
test("evidence pagination binds UTF-8 byte cursors, totals and references", async () => {
    const first = { reference, text: "é", offset: 0, next_offset: 2, total_bytes: 3 };
    assert.equal(isEvidencePage(first), true);
    assert.equal(isEvidencePage({ ...first, next_offset: 1 }), false);
    assert.equal(isEvidencePage({ ...first, next_offset: null }), false);
    assert.equal(isEvidencePage({ ...first, text: "é".repeat(32768) }), false);
    const offsets: number[] = [];
    const result = await loadEvidence(backend({ explanationEvidence: async (_reference, offset = 0) => {
            offsets.push(offset);
            return offset === 0 ? first : { reference, text: "!", offset: 2, next_offset: null, total_bytes: 3 };
        } }), reference);
    assert.deepEqual(result, { text: "é!", truncated: false });
    assert.deepEqual(offsets, [0, 2]);
    for (const invalid of [
        { ...first, reference: { ...reference, digest: "c".repeat(64) } },
        { ...first, offset: 1, next_offset: null },
    ])
        await assert.rejects(loadEvidence(backend({ explanationEvidence: async () => invalid }), reference), /pagination/);
    await assert.rejects(loadEvidence(backend({ explanationEvidence: async (_reference, offset = 0) => offset === 0
            ? first : { reference, text: "!", offset: 2, next_offset: null, total_bytes: 4 } }), reference), /pagination/);
});
test("evidence preview remains at most 1 MiB without cutting a UTF-8 character", async () => {
    let calls = 0;
    const result = await loadEvidence(backend({ explanationEvidence: async (_reference, offset = 0) => {
            calls += 1;
            const text = "€".repeat(10922);
            const end = offset + 32766;
            return { reference, text, offset, next_offset: end, total_bytes: 2 * 1024 * 1024 };
        } }), reference);
    assert.equal(result.truncated, true);
    assert.ok(new TextEncoder().encode(result.text).length <= 1024 * 1024);
    assert.equal(result.text.includes("�"), false);
    assert.equal(calls, 33);
});
// Small DOM test double: exercises real event handlers without adding a DOM dependency.
class Element {
    children: Element[] = [];
    parent: Element | null = null;
    value = "";
    className = "";
    type = "";
    title = "";
    disabled = false;
    open = false;
    scrollTop = 0;
    namespaceURI = "http://www.w3.org/2000/svg";
    onclick: (() => void) | null = null;
    onchange: (() => void) | null = null;
    ownText = "";
    listeners = new Map<string, () => void>();
    constructor(readonly tag: string) { }
    get textContent(): string { return this.ownText + this.children.map(child => child.textContent).join(""); }
    set textContent(value: string) { this.ownText = value; this.children = []; }
    get childElementCount(): number { return this.children.length; }
    append(...children: Element[]): void {
        for (const child of children) {
            child.parent = this;
            this.children.push(child);
        }
        if (this.tag === "select" && !this.value)
            this.value = children[0]?.value ?? "";
    }
    replaceChildren(...children: Element[]): void { this.children.forEach(child => { child.parent = null; }); this.children = []; this.ownText = ""; this.append(...children); }
    before(child: Element): void {
        if (this.parent) {
            child.parent = this.parent;
            this.parent.children.splice(this.parent.children.indexOf(this), 0, child);
        }
    }
    after(child: Element): void {
        if (this.parent) {
            child.parent = this.parent;
            this.parent.children.splice(this.parent.children.indexOf(this) + 1, 0, child);
        }
    }
    remove(): void {
        if (this.parent)
            this.parent.children = this.parent.children.filter(child => child !== this);
        this.parent = null;
    }
    setAttribute(): void { }
    contains(node: Element | null): boolean { return !!node && (node === this || this.children.some(child => child.contains(node))); }
    querySelectorAll(selector: string): Element[] {
        return this.children.flatMap(child => [
            ...(selector.startsWith(".") ? child.className.split(" ").includes(selector.slice(1)) : child.tag === selector) ? [child] : [],
            ...child.querySelectorAll(selector),
        ]);
    }
    querySelector(selector: string): Element | null { return this.querySelectorAll(selector)[0] ?? null; }
    addEventListener(name: string, callback: () => void): void { this.listeners.set(name, callback); }
    showModal(): void { this.open = true; }
    close(): void { this.open = false; this.listeners.get("close")?.(); }
}
function installDom(t: TestContext) {
    const body = new Element("body");
    const anchor = new Element("button");
    body.append(anchor);
    let selection: {
        isCollapsed: boolean;
        anchorNode: Element;
        focusNode: Element;
    } | null = null;
    const listeners = new Map<string, () => void>();
    const document = { body, createElement: (tag: string) => new Element(tag), createElementNS: (_namespace: string, tag: string) => new Element(tag),
        getElementById: () => anchor, getSelection: () => selection, addEventListener: (name: string, callback: () => void) => listeners.set(name, callback) };
    for (const [key, value] of Object.entries({ document, location: { hash: "" } })) {
        const old = Object.getOwnPropertyDescriptor(globalThis, key);
        Object.defineProperty(globalThis, key, { configurable: true, value });
        t.after(() => {
            if (old)
                Object.defineProperty(globalThis, key, old);
            else
                Reflect.deleteProperty(globalThis, key);
        });
    }
    t.mock.timers.enable({ apis: ["setTimeout"] });
    return { body, select: (node: Element | null) => { selection = node ? { isCollapsed: false, anchorNode: node, focusNode: node } : null; listeners.get("selectionchange")?.(); } };
}
async function settle(): Promise<void> {
    for (let index = 0; index < 20; index += 1)
        await Promise.resolve();
}
const status: StatusPayload = { protocol_version: "1", connected: true, authenticated_origin: "http://localhost:5173", latest_sequence: 0,
    runtime: { workspace_root: "/workspace", connected: true, explanations_available: true, turns_available: true, mutations_allowed: true, checks_allowed: true, approval_authority: "terminal" },
    settings: { host: "127.0.0.1", port: 1, pairing_ttl_secs: 300, max_frame_bytes: 1048576, max_in_flight_requests: 8, remote_enabled: false } };
function mounted(t: TestContext, source = backend()) {
    const dom = installDom(t);
    const connected = { ...source, connected: true, statusPayload: structuredClone(status) };
    const view = mountExplanation(() => connected);
    const dialog = dom.body.querySelector("dialog")!;
    const button = dom.body.querySelectorAll("button").find(button => button.textContent === "Explain")!;
    return { ...dom, connected, view, dialog, button, summary: dialog.querySelector(".explanation-summary")!, workspace: dialog.querySelector(".explanation-workspace")!, evidence: dialog.querySelector(".explanation-evidence")!, queryStatus: dialog.children[4]! };
}
test("unsupported capabilities block requests and reconnect restores the open view", async (t) => {
    let calls = 0;
    const ui = mounted(t, backend({ explanation: async () => { calls += 1; return page([entry()]); } }));
    const invalid = structuredClone(status) as unknown as {
        runtime: {
            explanations_available: unknown;
        };
    };
    invalid.runtime.explanations_available = "true";
    assert.throws(() => validateResponsePayload("status", invalid), /invalid WebMCP frame/);
    ui.connected.statusPayload = { ...ui.connected.statusPayload, runtime: { ...ui.connected.statusPayload.runtime, explanations_available: false } };
    ui.view.refresh();
    ui.button.onclick?.();
    t.mock.timers.tick(200);
    await settle();
    assert.equal(ui.button.disabled, true);
    assert.equal(calls, 0);
    ui.connected.statusPayload = { ...ui.connected.statusPayload, runtime: { ...ui.connected.statusPayload.runtime, explanations_available: true } };
    ui.view.refresh();
    ui.button.onclick?.();
    t.mock.timers.tick(200);
    await settle();
    assert.equal(calls, 1);
    ui.connected.connected = false;
    ui.view.refresh();
    assert.equal(ui.summary.textContent, "");
    ui.connected.connected = true;
    ui.view.refresh();
    t.mock.timers.tick(200);
    await settle();
    assert.equal(calls, 2);
    assert.match(ui.summary.textContent, /Action 0/);
});
test("refreshes coalesce, preserve unchanged DOM and scroll, and defer selected text updates", async (t) => {
    let calls = 0;
    let current = page([entry()]);
    const ui = mounted(t, backend({ explanation: async () => { calls += 1; return current; } }));
    ui.button.onclick?.();
    t.mock.timers.tick(200);
    await settle();
    const original = ui.summary.children[0];
    ui.dialog.scrollTop = 123;
    for (let index = 0; index < 20; index += 1)
        ui.view.refresh();
    t.mock.timers.tick(200);
    await settle();
    assert.equal(calls, 2);
    assert.equal(ui.summary.children[0], original);
    assert.equal(ui.dialog.scrollTop, 123);
    ui.select(ui.summary.querySelector("button"));
    current = page([{ ...entry(), label: "Changed" }]);
    current.model.revision = "c".repeat(64);
    ui.view.refresh();
    t.mock.timers.tick(200);
    await settle();
    assert.equal(ui.summary.children[0], original);
    assert.match(ui.queryStatus.textContent, /selection/);
    ui.select(null);
    assert.match(ui.summary.textContent, /Changed/);
    assert.equal(ui.dialog.scrollTop, 123);
});
test("stale evidence failures cannot overwrite a newer selection and event text remains literal", async (t) => {
    let rejectFirst: (reason: Error) => void = () => { };
    const actions = [{ ...entry(), label: "<img src=x onerror=alert(1)>" }, entry(1)];
    const ui = mounted(t, backend({ explanation: async () => page(actions), explanationEvidence: async (ref) => {
            if (ref.item_id === "item-0")
                return new Promise((_resolve, reject) => { rejectFirst = reject; });
            return { reference: ref, text: "<script>literal evidence</script>", offset: 0, next_offset: null, total_bytes: 33 };
        } }));
    ui.button.onclick?.();
    t.mock.timers.tick(200);
    await settle();
    const facts = ui.summary.querySelectorAll(".explanation-fact");
    facts[0]!.children[0]!.onclick?.();
    await settle();
    facts[1]!.children[0]!.onclick?.();
    await settle();
    rejectFirst(new Error("old failure"));
    await settle();
    assert.equal(ui.evidence.textContent, "<script>literal evidence</script>");
    assert.equal(ui.summary.querySelectorAll("img").length, 0);
    assert.equal(ui.evidence.querySelectorAll("script").length, 0);
});
test("workspace refresh preserves canonical DOM, scroll and selected literal diff text", async (t) => {
    let text = "+<script>human edit</script>";
    const ui = mounted(t, backend({ explanation: async () => ({ ...page([entry()]), workspace_diff: { captured_at: "2026-10-03T00:00:00Z", text, truncated: false, note: "Ownership is not attributed to the agent." } }) }));
    ui.button.onclick?.();
    t.mock.timers.tick(200);
    await settle();
    const goal = ui.summary.children[0];
    assert.match(ui.workspace.textContent, /\+<script>human edit<\/script>/);
    assert.equal(ui.workspace.querySelectorAll("script").length, 0);
    ui.dialog.scrollTop = 200;
    ui.select(ui.workspace.querySelector("pre"));
    text = "+later workspace edit";
    ui.view.refresh();
    t.mock.timers.tick(200);
    await settle();
    assert.match(ui.workspace.textContent, /human edit/);
    ui.select(null);
    assert.match(ui.workspace.textContent, /later workspace edit/);
    assert.equal(ui.summary.children[0], goal);
    assert.equal(ui.dialog.scrollTop, 200);
});
test("native graph labels show recorded commands and edits while preserving opaque identities", async (t) => {
    const current = page([entry()]);
    current.model.graph = [
        { from: `event 0:${reference.digest} [task / root / item-0]`, to: "src/file.rs", relation: "changed file" },
        { from: `event 100:${"c".repeat(64)} [task / root / file-change]`, to: "src/other.rs", relation: "changed file" },
        { from: "root", to: "reviewer", relation: "delegated agent" },
    ];
    const ui = mounted(t, backend({ explanation: async () => current }));
    ui.button.onclick?.();
    t.mock.timers.tick(200);
    await settle();
    const graph = ui.summary.querySelector("svg")!;
    assert.match(graph.textContent, /Action 0 → src\/file.rs/);
    assert.match(graph.textContent, /Recorded edit → src\/other.rs/);
    assert.match(graph.textContent, /root → reviewer/);
    assert.equal(current.model.graph[0]!.from, `event 0:${reference.digest} [task / root / item-0]`);
});
test("scope changes suppress stale query failures and request the latest scope once", async (t) => {
    let rejectFirst: (reason: Error) => void = () => { };
    const scopes: string[] = [];
    const ui = mounted(t, backend({ explanation: async (scope) => {
            scopes.push(scope);
            if (scopes.length === 1)
                return new Promise((_resolve, reject) => { rejectFirst = reject; });
            const result = page([entry()]);
            result.model.scope = scope;
            return result;
        } }));
    ui.button.onclick?.();
    t.mock.timers.tick(200);
    await settle();
    const scope = ui.dialog.querySelector("select")!;
    scope.value = "session";
    scope.onchange?.();
    for (let index = 0; index < 10; index += 1)
        ui.view.refresh();
    rejectFirst(new Error("old failure"));
    await settle();
    assert.equal(ui.queryStatus.textContent.includes("unavailable"), false);
    t.mock.timers.tick(200);
    await settle();
    assert.deepEqual(scopes, ["task", "session"]);
    assert.match(ui.summary.textContent, /Action 0/);
});
test("large histories render only the first 100 rows per section and expose more", async (t) => {
    const actions = Array.from({ length: 10000 }, (_, index) => entry(index));
    const ui = mounted(t, backend({ explanation: async (_scope, offset = 0) => page(actions, offset) }));
    ui.button.onclick?.();
    t.mock.timers.tick(200);
    for (let index = 0; index < 40; index += 1)
        await settle();
    assert.equal(ui.summary.querySelectorAll(".explanation-fact").length, 100);
    const more = ui.summary.querySelectorAll("button").find(button => button.textContent.startsWith("Show next"));
    assert.ok(more);
    more.onclick?.();
    assert.equal(ui.summary.querySelectorAll(".explanation-fact").length, 200);
});
test("recorded token breakdowns render bounded rows with evidence navigation and missing-data state", async (t) => {
    const tokens = Array.from({ length: 101 }, (_, index) => tokenEntry(index));
    tokens[0]!.breakdown.subagent_bootstrap_tokens = 80;
    let navigated: EvidenceRef | undefined;
    const ui = mounted(t, backend({ explanation: async (_scope, offset = 0) => tokenPage(tokens, offset),
        explanationEvidence: async (ref) => ({ reference: ref, text: "prefix", offset: 0, next_offset: null, total_bytes: 6 }),
        explanationNavigate: async (ref) => { navigated = ref; return { focused: true }; } }));
    ui.button.onclick?.();
    t.mock.timers.tick(200);
    await settle();
    const section = ui.summary.children.find(node => node.querySelector("h2")?.textContent === "Request-prefix token breakdowns")!;
    assert.equal(section.querySelectorAll(".explanation-fact").length, 100);
    assert.match(section.textContent, /10 system prompt; 20 tool schema; 30 instruction files; 40 message history/);
    assert.match(section.textContent, /50 cache read; 60 cache write; 70 cache miss/);
    assert.match(section.textContent, /80 subagent bootstrap/);
    section.querySelector("button")!.onclick?.();
    await settle();
    ui.dialog.querySelector(".explanation-navigation")!.onclick?.();
    await settle();
    assert.deepEqual(navigated, tokens[0]!.fact.evidence);
    section.querySelectorAll("button").find(button => button.textContent.startsWith("Show next"))!.onclick?.();
    assert.equal(ui.summary.querySelectorAll(".explanation-fact").length, 101);
    ui.connected.explanation = async () => { const legacy = page(); legacy.model.revision = "c".repeat(64); return legacy; };
    ui.view.refresh();
    t.mock.timers.tick(200);
    await settle();
    assert.match(ui.summary.textContent, /token breakdowns unavailable in retained history/);
});
