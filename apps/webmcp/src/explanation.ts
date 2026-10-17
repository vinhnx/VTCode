import { isRecord, type StatusPayload } from "./types.ts";
export type ExplanationScope = "task" | "session";
export interface EvidenceRef {
    session_id: string;
    offset: number;
    length: number;
    digest: string;
    item_id: string | null;
}
export interface ExplanationEntry {
    label: string;
    status: string;
    evidence: EvidenceRef;
    task_id: string | null;
    actor_id: string | null;
    parent_actor_id: string | null;
    timestamp: string | null;
    path: string | null;
    line: number | null;
}
export interface VerificationEntry {
    fact: ExplanationEntry;
    exit_code: number | null;
    fresh: boolean;
}
export interface DecisionEntry {
    fact: ExplanationEntry;
    rationale: string;
    alternatives: string[];
    evidence_ids: string[];
}
export interface ReviewSignal {
    priority: "high" | "medium";
    reason: string;
    fact: ExplanationEntry;
}
export interface GraphEdge {
    from: string;
    to: string;
    relation: string;
}
export interface TokenBreakdown {
    system_prompt_tokens: number;
    tool_schema_tokens: number;
    instruction_file_tokens: number;
    message_history_tokens: number;
    cache_read_tokens: number;
    cache_write_tokens: number;
    cache_miss_tokens: number;
    subagent_bootstrap_tokens?: number;
}
export interface TokenBreakdownEntry {
    fact: ExplanationEntry;
    breakdown: TokenBreakdown;
}
export interface ExplanationModel {
    session_id: string;
    revision: string;
    scope: ExplanationScope;
    task_id: string | null;
    status: string;
    goals: ExplanationEntry[];
    actions: ExplanationEntry[];
    changes: ExplanationEntry[];
    edit_operations: number;
    decisions: DecisionEntry[];
    verification: VerificationEntry[];
    failures: ExplanationEntry[];
    review_priorities: ReviewSignal[];
    plan_evolution: ExplanationEntry[];
    graph: GraphEdge[];
    token_breakdowns?: TokenBreakdownEntry[];
    usage: {
        input_tokens: number;
        cached_input_tokens: number;
        cache_creation_tokens: number;
        output_tokens: number;
    } | null;
    cost_usd: number | null;
    completeness: {
        malformed_records: number;
        unknown_records: number;
        legacy_records: number;
        evicted_turns: number;
        warnings: string[];
    };
}
export interface ExplanationPage {
    workspace_diff?: WorkspaceDiffSnapshot;
    model: ExplanationModel;
    offset: number;
    next_offset: number | null;
}
export interface WorkspaceDiffSnapshot {
    captured_at: string;
    text: string | null;
    truncated: boolean;
    note: string;
}
export interface EvidencePage {
    reference: EvidenceRef;
    text: string;
    offset: number;
    next_offset: number | null;
    total_bytes: number;
}
const bounded = (v: unknown, limit: number): v is string => typeof v === "string" && v.length <= limit;
const natural = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v >= 0;
const nullableText = (v: unknown, limit: number): boolean => v === null || bounded(v, limit);
export function isEvidenceRef(v: unknown): v is EvidenceRef {
    return isRecord(v) && bounded(v.session_id, 256) && v.session_id.length > 0 && natural(v.offset) && natural(v.length) && Number.isSafeInteger(v.offset + v.length) && bounded(v.digest, 64) && /^[a-f0-9]{64}$/.test(v.digest) && nullableText(v.item_id, 256);
}
function isEntry(v: unknown): v is ExplanationEntry {
    return isRecord(v) && bounded(v.label, 2048) && bounded(v.status, 128) && isEvidenceRef(v.evidence) && nullableText(v.task_id, 256) && nullableText(v.actor_id, 256) && nullableText(v.parent_actor_id, 256) && nullableText(v.timestamp, 128) && nullableText(v.path, 2048) && (v.line === null || natural(v.line));
}
function array(v: unknown, predicate: (v: unknown) => boolean, maximum = 32): boolean { return Array.isArray(v) && v.length <= maximum && v.every(predicate); }
const tokenFields = ["system_prompt_tokens", "tool_schema_tokens", "instruction_file_tokens", "message_history_tokens", "cache_read_tokens", "cache_write_tokens", "cache_miss_tokens"] as const;
function isTokenBreakdown(v: unknown): v is TokenBreakdown {
    return isRecord(v) && tokenFields.every(key => natural(v[key]))
        && (!Object.hasOwn(v, "subagent_bootstrap_tokens") || natural(v.subagent_bootstrap_tokens));
}
export function isExplanationPage(v: unknown): v is ExplanationPage {
    if (!isRecord(v) || !natural(v.offset) || v.offset > MAX_GRAPH_ENTRIES || !(v.next_offset === null || natural(v.next_offset) && v.next_offset > v.offset && v.next_offset <= MAX_GRAPH_ENTRIES) || !isRecord(v.model))
        return false;
    const m = v.model;
    if (Object.hasOwn(v, "workspace_diff")) {
        const workspace = v.workspace_diff;
        if (v.offset !== 0 || !isRecord(workspace) || !bounded(workspace.captured_at, 128)
            || !nullableText(workspace.text, 8192) || typeof workspace.truncated !== "boolean" || !bounded(workspace.note, 2048)
            || typeof workspace.text === "string" && new TextEncoder().encode(workspace.text).length > 8192)
            return false;
    }
    const valid = bounded(m.session_id, 256) && m.session_id.length > 0 && bounded(m.revision, 64) && /^[a-f0-9]{64}$/.test(m.revision) && (m.scope === "task" || m.scope === "session") && nullableText(m.task_id, 256) && bounded(m.status, 128) && natural(m.edit_operations)
        && [m.goals, m.actions, m.changes, m.failures, m.plan_evolution].every(a => array(a, isEntry))
        && array(m.decisions, d => isRecord(d) && isEntry(d.fact) && bounded(d.rationale, 2048) && array(d.alternatives, a => bounded(a, 2048), 3) && array(d.evidence_ids, id => bounded(id, 256), 8))
        && array(m.verification, c => isRecord(c) && isEntry(c.fact) && (c.exit_code === null || typeof c.exit_code === "number" && Number.isSafeInteger(c.exit_code)) && typeof c.fresh === "boolean")
        && array(m.review_priorities, r => isRecord(r) && (r.priority === "high" || r.priority === "medium") && bounded(r.reason, 2048) && isEntry(r.fact))
        && array(m.graph, e => isRecord(e) && bounded(e.from, 2048) && bounded(e.to, 2048) && bounded(e.relation, 128))
        && (!Object.hasOwn(m, "token_breakdowns") || array(m.token_breakdowns, value => isRecord(value) && isEntry(value.fact) && isTokenBreakdown(value.breakdown)))
        && (m.usage === null || isRecord(m.usage) && [m.usage.input_tokens, m.usage.cached_input_tokens, m.usage.cache_creation_tokens, m.usage.output_tokens].every(natural))
        && (m.cost_usd === null || typeof m.cost_usd === "number" && Number.isFinite(m.cost_usd) && m.cost_usd >= 0)
        && isRecord(m.completeness) && [m.completeness.malformed_records, m.completeness.unknown_records, m.completeness.legacy_records, m.completeness.evicted_turns].every(natural) && array(m.completeness.warnings, w => bounded(w, 2048));
    if (!valid)
        return false;
    const page = v as unknown as ExplanationPage;
    const lengths = collections.map(key => page.model[key]?.length ?? 0);
    const facts = [...page.model.goals, ...page.model.actions, ...page.model.changes, ...page.model.failures,
        ...page.model.plan_evolution, ...page.model.decisions.map(value => value.fact),
        ...page.model.verification.map(value => value.fact), ...page.model.review_priorities.map(value => value.fact),
        ...(page.model.token_breakdowns ?? []).map(value => value.fact)];
    return lengths.every((length, index) => length === 0 || page.offset + length <= (collections[index] === "graph" ? MAX_GRAPH_ENTRIES : MAX_ENTRIES))
        && (page.next_offset === null || Math.max(...lengths) > 0 && page.next_offset === page.offset + Math.max(...lengths))
        && facts.every(fact => fact.evidence.session_id === page.model.session_id);
}
export function isEvidencePage(v: unknown): v is EvidencePage {
    if (!isRecord(v) || !isEvidenceRef(v.reference) || !bounded(v.text, 32768) || !natural(v.offset) || !natural(v.total_bytes))
        return false;
    const bytes = new TextEncoder().encode(v.text).length;
    const end = v.offset + bytes;
    return bytes <= 32768 && end <= v.total_bytes && (end === v.total_bytes
        ? v.next_offset === null
        : bytes > 0 && v.next_offset === end);
}
const collections = ["goals", "actions", "changes", "decisions", "verification", "failures", "review_priorities", "plan_evolution", "graph", "token_breakdowns"] as const;
const MAX_ENTRIES = 10000;
// A retained event can contribute order, file, and delegation relationships.
const MAX_GRAPH_ENTRIES = 3 * MAX_ENTRIES;
const MAX_EVIDENCE_BYTES = 1024 * 1024;
function metadata(model: ExplanationModel): string {
    return JSON.stringify([model.session_id, model.revision, model.scope, model.task_id, model.status,
        model.edit_operations, model.cost_usd, model.token_breakdowns !== undefined,
        model.usage && [model.usage.input_tokens, model.usage.cached_input_tokens, model.usage.cache_creation_tokens, model.usage.output_tokens],
        model.completeness.malformed_records, model.completeness.unknown_records, model.completeness.legacy_records,
        model.completeness.evicted_turns, model.completeness.warnings]);
}
function validatePage(page: ExplanationPage, previous?: ExplanationPage): void {
    if (!isExplanationPage(page))
        throw new Error("Invalid explanation page or pagination");
    if (previous ? page.offset !== previous.next_offset || metadata(page.model) !== metadata(previous.model) : page.offset !== 0) {
        throw new Error("Execution changed during the query; refresh to inspect a consistent snapshot");
    }
    const lengths = collections.map(key => page.model[key]?.length ?? 0);
    const previousWidth = previous ? Math.max(...collections.map(key => previous.model[key]?.length ?? 0)) : 0;
    if ((page.offset > 0 && lengths.every(length => length === 0))
        || (previous && collections.some(key => (previous.model[key]?.length ?? 0) < previousWidth && (page.model[key]?.length ?? 0) !== 0))) {
        throw new Error("Invalid explanation pagination");
    }
}
/** Pages from different revisions never combine into an invented snapshot. */
export function mergeExplanationPages(pages: ExplanationPage[]): ExplanationModel {
    const first = pages[0];
    if (!first || first.offset !== 0)
        throw new Error("Explanation starts at page zero");
    pages.forEach((page, index) => validatePage(page, pages[index - 1]));
    if (pages.at(-1)?.next_offset !== null)
        throw new Error("Explanation query is incomplete");
    const model = structuredClone(first.model);
    for (let i = 1; i < pages.length; i++) {
        const page = pages[i]!;
        for (const key of collections)
            (model[key] as unknown[] | undefined)?.push(...(page.model[key] ?? []));
    }
    return model;
}
export interface ExplanationBackend {
    explanation(scope: ExplanationScope, offset?: number): Promise<ExplanationPage>;
    explanationEvidence(reference: EvidenceRef, offset?: number): Promise<EvidencePage>;
    explanationNavigate(reference: EvidenceRef): Promise<{
        focused: boolean;
    }>;
}
/** Validate each cursor before sending another request, bounding even hostile adapters. */
export async function loadExplanation(source: ExplanationBackend, scope: ExplanationScope, active: () => boolean = () => true, cached?: ExplanationModel, workspace?: (snapshot: WorkspaceDiffSnapshot | null) => void): Promise<ExplanationModel> {
    const pages: ExplanationPage[] = [];
    let offset = 0;
    while (true) {
        if (!active())
            throw new Error("Explanation query superseded");
        const page = await source.explanation(scope, offset);
        validatePage(page, pages.at(-1));
        if (page.model.scope !== scope)
            throw new Error("Explanation scope differs from request");
        if (offset === 0 && active())
            workspace?.(page.workspace_diff ?? null);
        if (offset === 0 && cached && metadata(page.model) === metadata(cached))
            return cached;
        pages.push(page);
        if (page.next_offset === null)
            return mergeExplanationPages(pages);
        offset = page.next_offset;
    }
}
function sameReference(a: EvidenceRef, b: EvidenceRef): boolean {
    return a.session_id === b.session_id && a.offset === b.offset && a.length === b.length && a.digest === b.digest && a.item_id === b.item_id;
}
export async function loadEvidence(source: ExplanationBackend, reference: EvidenceRef, active: () => boolean = () => true): Promise<{
    text: string;
    truncated: boolean;
}> {
    let offset = 0;
    let total: number | undefined;
    const chunks: string[] = [];
    while (true) {
        if (!active())
            throw new Error("Evidence query superseded");
        const page = await source.explanationEvidence(reference, offset);
        if (!isEvidencePage(page) || !sameReference(page.reference, reference) || page.offset !== offset
            || (total !== undefined && total !== page.total_bytes))
            throw new Error("Invalid evidence pagination");
        total = page.total_bytes;
        const bytes = new TextEncoder().encode(page.text);
        let length = Math.min(bytes.length, MAX_EVIDENCE_BYTES - offset);
        while (length > 0 && length < bytes.length && (bytes[length]! & 0xc0) === 0x80)
            length -= 1;
        chunks.push(new TextDecoder().decode(bytes.subarray(0, length)));
        if (length < bytes.length || page.next_offset === null || page.next_offset >= MAX_EVIDENCE_BYTES) {
            return { text: chunks.join(""), truncated: length < bytes.length || page.next_offset !== null };
        }
        offset = page.next_offset;
    }
}
/** DOM-only text rendering: canonical event text never becomes HTML or script. */
export function mountExplanation(getBackend: () => ExplanationBackend & {
    connected: boolean;
    statusPayload: StatusPayload | null;
}): {
    refresh(): void;
} {
    const dialog = document.createElement("dialog");
    dialog.className = "explanation-view";
    const title = document.createElement("h1");
    title.textContent = "Execution explanation";
    const close = document.createElement("button");
    close.textContent = "Close";
    close.type = "button";
    close.onclick = () => dialog.close();
    const scope = document.createElement("select");
    scope.setAttribute("aria-label", "Explanation scope");
    for (const value of ["task", "session"] as const) {
        const option = document.createElement("option");
        option.value = value;
        option.textContent = value === "task" ? "Latest task" : "Retained session";
        scope.append(option);
    }
    const refresh = document.createElement("button");
    refresh.textContent = "Refresh";
    refresh.type = "button";
    const summary = document.createElement("div");
    summary.className = "explanation-summary";
    const workspace = document.createElement("details");
    workspace.className = "explanation-workspace";
    const evidence = document.createElement("pre");
    evidence.className = "explanation-evidence";
    const queryStatus = document.createElement("p");
    queryStatus.setAttribute("role", "status");
    dialog.append(title, close, scope, refresh, queryStatus, summary, workspace, evidence);
    document.body.append(dialog);
    const button = document.createElement("button");
    button.className = "button subtle";
    button.textContent = "Explain";
    button.type = "button";
    document.getElementById("evidenceButton")?.after(button);
    let running = false;
    let dirty = false;
    let selected: EvidenceRef | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let sourceIdentity = getBackend();
    let generation = 0;
    let evidenceRequest = 0;
    let renderedKey: string | null = null;
    let pendingModel: ExplanationModel | null = null;
    let cachedModel: ExplanationModel | undefined;
    let pendingWorkspace: WorkspaceDiffSnapshot | null | undefined;
    const visibleCounts = new Map<string, number>();
    const available = () => getBackend().connected && getBackend().statusPayload?.runtime.connected === true
        && getBackend().statusPayload?.runtime.explanations_available === true;
    const clearEvidence = () => {
        selected = null;
        evidenceRequest += 1;
        evidence.textContent = "";
        dialog.querySelectorAll(".explanation-navigation").forEach(node => node.remove());
    };
    function updateAvailability(): void {
        const source = getBackend();
        if (source !== sourceIdentity || !available()) {
            sourceIdentity = source;
            generation += 1;
            clearEvidence();
            renderedKey = null;
            pendingModel = null;
            cachedModel = undefined;
            visibleCounts.clear();
            summary.replaceChildren();
            workspace.replaceChildren();
            pendingWorkspace = undefined;
        }
        button.disabled = !available();
        refresh.disabled = button.disabled;
        button.title = button.disabled ? "Pair with an active session that supports execution explanations." : "Explain the recorded execution";
        if (button.disabled)
            queryStatus.textContent = button.title;
    }
    const paragraph = (parent: HTMLElement, text: string) => { const p = document.createElement("p"); p.textContent = text; parent.append(p); };
    const fact = (parent: HTMLElement, f: ExplanationEntry, suffix = "") => {
        const row = document.createElement("div");
        row.className = "explanation-fact";
        const link = document.createElement("button");
        link.type = "button";
        link.textContent = `${f.label} — ${f.status}${suffix}`;
        link.onclick = () => { selected = f.evidence; void showEvidence(); };
        row.append(link);
        parent.append(row);
    };
    async function showEvidence(): Promise<void> {
        if (!selected || !available())
            return;
        const reference = selected;
        const source = getBackend();
        const request = ++evidenceRequest;
        const active = () => request === evidenceRequest && selected === reference && source === getBackend() && available() && dialog.open;
        evidence.textContent = "Loading evidence…";
        dialog.querySelectorAll(".explanation-navigation").forEach(node => node.remove());
        try {
            const result = await loadEvidence(source, reference, active);
            if (!active())
                return;
            evidence.textContent = result.text + (result.truncated ? "\nEvidence exceeds the 1 MiB preview limit." : "");
            const navigate = document.createElement("button");
            navigate.type = "button";
            navigate.textContent = "Focus in terminal review";
            navigate.onclick = () => {
                if (!active())
                    return;
                void source.explanationNavigate(reference).then(result => {
                    if (active() && !result.focused)
                        queryStatus.textContent = "Terminal evidence is unavailable or expired.";
                }).catch(() => {
                    if (active())
                        queryStatus.textContent = "Terminal evidence is unavailable or expired.";
                });
            };
            evidence.before(navigate);
            navigate.className = "explanation-navigation";
            dialog.querySelectorAll(".explanation-navigation").forEach(node => {
                if (node !== navigate)
                    node.remove();
            });
        }
        catch {
            if (active())
                evidence.textContent = "Evidence is unavailable, expired, or not supported by this adapter.";
        }
    }
    function hasSelection(): boolean {
        const selection = document.getSelection();
        return !!selection && !selection.isCollapsed && [summary, workspace].some(element => element.contains(selection.anchorNode) || element.contains(selection.focusNode));
    }
    function renderWorkspace(snapshot: WorkspaceDiffSnapshot | null): void {
        if (hasSelection()) { pendingWorkspace = snapshot; return; }
        pendingWorkspace = undefined;
        const scroll = dialog.scrollTop;
        workspace.replaceChildren();
        const title = document.createElement("summary");
        title.textContent = "Current workspace state";
        workspace.append(title);
        paragraph(workspace, snapshot ? `Captured at ${snapshot.captured_at}. ${snapshot.note}` : "Current workspace state unavailable from this adapter.");
        if (snapshot?.text !== null && snapshot?.text !== undefined) {
            const diff = document.createElement("pre");
            diff.textContent = snapshot.text || "No tracked Git changes against HEAD at capture time.";
            workspace.append(diff);
        }
        if (snapshot?.truncated) paragraph(workspace, "Current diff truncated at the snapshot limit.");
        dialog.scrollTop = scroll;
    }
    function render(model: ExplanationModel): void {
        const key = JSON.stringify([model.session_id, model.scope, model.task_id, model.revision]);
        if (key === renderedKey)
            return;
        if (hasSelection()) {
            pendingModel = model;
            queryStatus.textContent = "Updated explanation will appear when the text selection is cleared.";
            return;
        }
        renderedKey = key;
        pendingModel = null;
        queryStatus.textContent = "";
        const scroll = dialog.scrollTop;
        summary.replaceChildren();
        const section = (name: string) => { const s = document.createElement("section"); const h = document.createElement("h2"); h.textContent = name; s.append(h); summary.append(s); return s; };
        const list = <T,>(parent: HTMLElement, entries: readonly T[], draw: (entry: T, index: number) => void) => {
            const key = parent.querySelector("h2")!.textContent!;
            const count = visibleCounts.get(key) ?? 100;
            entries.slice(0, count).forEach(draw);
            if (entries.length > count) {
                const more = document.createElement("button");
                more.type = "button";
                more.textContent = `Show next ${Math.min(100, entries.length - count)} (${entries.length - count} remaining)`;
                more.onclick = () => { visibleCounts.set(key, count + 100); renderedKey = null; render(model); };
                parent.append(more);
            }
        };
        const goal = section("Goal");
        paragraph(goal, `Outcome: ${model.status}`);
        list(goal, model.goals, g => fact(goal, g));
        if (!model.goals.length)
            paragraph(goal, "Goal unavailable in retained history.");
        const changes = section("Changes");
        paragraph(changes, `${model.changes.length} distinct recorded files; ${model.edit_operations} edit operations. Current workspace edits have separate ownership.`);
        list(changes, model.changes, c => fact(changes, c));
        const decisions = section("Decisions");
        list(decisions, model.decisions, d => { fact(decisions, d.fact); paragraph(decisions, `${d.rationale} (agent-reported)`); d.alternatives.forEach(a => paragraph(decisions, `Alternative: ${a}`)); });
        if (!model.decisions.length)
            paragraph(decisions, "Public rationale unavailable.");
        const checks = section("Verification");
        list(checks, model.verification, v => fact(checks, v.fact, v.fact.status === "passed" && !v.fresh ? " (before a later mutation attempt)" : ""));
        if (!model.verification.length)
            paragraph(checks, "No verification command recorded.");
        const review = section("Review first");
        paragraph(review, "These are review signals, not proven bugs or coverage assessments.");
        list(review, model.review_priorities, r => { fact(review, r.fact, ` (${r.priority})`); paragraph(review, r.reason); });
        const completeness = section("Evidence completeness");
        paragraph(completeness, `${model.completeness.malformed_records} malformed; ${model.completeness.unknown_records} unknown; ${model.completeness.legacy_records} legacy; ${model.completeness.evicted_turns} evicted turns.`);
        model.completeness.warnings.forEach(w => paragraph(completeness, w));
        for (const [name, entries] of [["Timeline", model.actions], ["Failures", model.failures], ["Plan evolution", model.plan_evolution]] as const) {
            const s = section(name);
            list(s, entries, f => fact(s, f));
        }
        const agents = section("Agent tree");
        const seen = new Set<string>();
        const actors = model.actions.filter(a => {
            if (!a.actor_id || seen.has(a.actor_id))
                return false;
            seen.add(a.actor_id);
            return true;
        });
        list(agents, actors, a => paragraph(agents, `${a.parent_actor_id ? `${a.parent_actor_id} → ` : ""}${a.actor_id}`));
        if (!seen.size)
            paragraph(agents, "Ancestry unavailable.");
        const graph = section("Recorded action and file graph");
        const graphLabels = new Map([...model.actions, ...model.plan_evolution, ...model.decisions.map(decision => decision.fact)]
            .map(fact => [`${fact.evidence.offset}:${fact.evidence.digest}`, fact.label]));
        const edits = new Set(model.graph.filter(edge => edge.relation === "changed file").map(edge => edge.from));
        const graphLabel = (target: string): string => {
            const reference = /^event ([0-9]+:[a-f0-9]{64}) \[/.exec(target)?.[1];
            return (reference && graphLabels.get(reference)) || (edits.has(target) ? "Recorded edit" : target);
        };
        const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
        svg.setAttribute("viewBox", `0 0 900 ${Math.max(40, model.graph.length * 30)}`);
        svg.setAttribute("role", "img");
        svg.setAttribute("aria-label", "Recorded action and file relationships");
        graph.append(svg);
        list(graph, model.graph, (edge, i) => { const text = document.createElementNS(svg.namespaceURI, "text"); text.setAttribute("x", "8"); text.setAttribute("y", String(i * 30 + 22)); text.textContent = `${graphLabel(edge.from)} → ${graphLabel(edge.to)} (${edge.relation})`; svg.append(text); });
        svg.setAttribute("viewBox", `0 0 900 ${Math.max(40, svg.childElementCount * 30)}`);
        const usage = section("Usage and timing");
        paragraph(usage, model.usage ? `${model.usage.input_tokens} input; ${model.usage.cached_input_tokens} cached; ${model.usage.cache_creation_tokens} cache creation; ${model.usage.output_tokens} output tokens.` : "Usage unavailable.");
        paragraph(usage, model.cost_usd === null ? "Cost unavailable for this scope." : `Recorded cost: $${model.cost_usd}`);
        const breakdowns = section("Request-prefix token breakdowns");
        list(breakdowns, model.token_breakdowns ?? [], entry => {
            fact(breakdowns, entry.fact);
            const breakdown = entry.breakdown;
            paragraph(breakdowns, `${breakdown.system_prompt_tokens} system prompt; ${breakdown.tool_schema_tokens} tool schema; ${breakdown.instruction_file_tokens} instruction files; ${breakdown.message_history_tokens} message history tokens.`);
            paragraph(breakdowns, `${breakdown.cache_read_tokens} cache read; ${breakdown.cache_write_tokens} cache write; ${breakdown.cache_miss_tokens} cache miss tokens.`);
            if (breakdown.subagent_bootstrap_tokens !== undefined)
                paragraph(breakdowns, `${breakdown.subagent_bootstrap_tokens} subagent bootstrap tokens.`);
        });
        if (!model.token_breakdowns?.length)
            paragraph(breakdowns, "Request-prefix token breakdowns unavailable in retained history.");
        list(usage, model.actions.filter(a => a.timestamp), a => paragraph(usage, `${a.timestamp}: ${a.label}`));
        dialog.scrollTop = scroll;
    }
    async function query(): Promise<void> {
        if (!dialog.open || !available())
            return;
        if (running) {
            dirty = true;
            return;
        }
        running = true;
        dirty = false;
        const source = getBackend();
        const requested = scope.value as ExplanationScope;
        const requestedGeneration = generation;
        const active = () => source === getBackend() && requested === scope.value && requestedGeneration === generation && available() && dialog.open;
        try {
            const model = await loadExplanation(source, requested, active, cachedModel, renderWorkspace);
            if (active()) {
                cachedModel = model;
                queryStatus.textContent = "";
                render(model);
            }
        }
        catch {
            if (active())
                queryStatus.textContent = "Explanation unavailable or execution changed during this query. Pair with an active session and refresh.";
        }
        finally {
            running = false;
            if (dirty)
                schedule();
        }
    }
    function schedule(): void {
        updateAvailability();
        if (!dialog.open || !available())
            return;
        if (running) {
            dirty = true;
            return;
        }
        if (timer !== undefined)
            return;
        timer = setTimeout(() => { timer = undefined; void query(); }, 200);
    }
    button.onclick = () => {
        if (!available())
            return;
        dialog.showModal();
        schedule();
    };
    refresh.onclick = schedule;
    scope.onchange = () => { generation += 1; clearEvidence(); pendingModel = null; cachedModel = undefined; renderedKey = null; visibleCounts.clear(); summary.replaceChildren(); schedule(); };
    dialog.addEventListener("close", () => {
        generation += 1;
        clearEvidence();
        pendingModel = null;
        pendingWorkspace = undefined;
        if (timer !== undefined)
            clearTimeout(timer);
        timer = undefined;
    });
    document.addEventListener("selectionchange", () => {
        if (pendingModel && dialog.open && available() && !hasSelection())
            render(pendingModel);
        if (pendingWorkspace !== undefined && dialog.open && available() && !hasSelection())
            renderWorkspace(pendingWorkspace);
    });
    updateAvailability();
    if (location.hash === "#explanation") {
        dialog.showModal();
        schedule();
    }
    return { refresh: schedule };
}
