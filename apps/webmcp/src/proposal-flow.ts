import { $ } from "./app-elements.ts";
import { MAX_FILE_BYTES, buildTurnPrompt, createUnifiedDiff } from "./backend.ts";
import { message, openTurnComposer, selectTerminal, snapshotSizeBytes } from "./app-feedback.ts";
import { BackendError, errorCode, type ClientProposal, type PatchProposal, type RuntimeStatus } from "./types.ts";
import type { CodeEditor } from "./editor.ts";
import { current, dirtyPaths, snapshot, type AppBackend, type AppState } from "./workspace-view.ts";

export interface ProposalFlowOptions {
  readonly state: AppState;
  readonly editor: CodeEditor;
  readonly getBackend: () => AppBackend;
  readonly runtimeStatus: () => RuntimeStatus | undefined;
  readonly isHeadlessBridge: () => boolean;
  readonly isActiveRuntime: () => boolean;
  readonly refreshFile: (path: string, force?: boolean, render?: boolean) => Promise<void>;
  readonly renderTree: () => void;
  readonly renderTabs: () => void;
  readonly renderSelectedEditor: () => void;
  readonly updateEditorFooter: () => void;
  readonly openConnectionPanel: () => void;
  readonly persist: () => boolean;
  readonly log: (text: string) => void;
  readonly status: (title: string, detail: string) => void;
  readonly toast: (text: string) => void;
}

/** Draft review, terminal approval, apply, revert, checks, and agent turns. Backend pairing stays in main. */
export function createProposalFlow(options: ProposalFlowOptions) {
  const {
    state, editor, getBackend, runtimeStatus, isHeadlessBridge, isActiveRuntime,
    refreshFile, renderTree, renderTabs, renderSelectedEditor, updateEditorFooter, openConnectionPanel,
    persist, log, status, toast,
  } = options;

  function collectChanges(): Array<{ path: string; base_digest: string; content: string }> {
    return dirtyPaths(state).flatMap((path) => {
      const base = snapshot(state, path);
      return base ? [{ path, base_digest: base.digest, content: current(state, path) }] : [];
    });
  }

  function renderProposal(): void {
    const view = $("diffView");
    view.replaceChildren();
    const proposal = state.serverProposal || state.clientProposal;
    if (!proposal) {
      const empty = document.createElement("div");
      empty.className = "empty-state";
      const emptyTitle = document.createElement("strong");
      emptyTitle.textContent = "No changes staged";
      const emptyHint = document.createElement("small");
      emptyHint.textContent = "Edit a buffer, then review the draft.";
      empty.append(emptyTitle, emptyHint);
      view.append(empty);
      $("proposalState").textContent = "No proposal";
      $("proposalState").className = "state-badge";
    } else {
      const diff = proposal.unified_diff || state.clientProposal?.unified_diff || "";
      for (const line of diff.split("\n")) {
        const item = document.createElement("div");
        item.className = `diff-line ${line.startsWith("+") && !line.startsWith("+++") ? "added" : line.startsWith("-") && !line.startsWith("---") ? "removed" : "meta"}`;
        item.textContent = line || " ";
        view.append(item);
      }
      const stateLabel = state.pendingTerminalApproval
        ? "Awaiting terminal"
        : state.approved
          ? "Approved · ready"
          : state.serverProposal
            ? isActiveRuntime() ? "Ready for VT Code turn" : "Proposal sent"
            : "Review ready";
      $("proposalState").textContent = stateLabel;
      $("proposalState").className = `state-badge ${state.approved ? "approved" : state.pendingTerminalApproval ? "pending" : "ready"}`;
    }
    const bridgeUnavailable = getBackend().kind === "websocket" && !getBackend().connected;
    $("reviewChanges").disabled = dirtyPaths(state).length === 0;
    $("approvePatch").disabled = bridgeUnavailable || !state.clientProposal || Boolean(state.serverProposal);
    const requiresAgentTurn = isActiveRuntime() && runtimeStatus()?.mutations_allowed === false;
    $("applyPatch").disabled = bridgeUnavailable || !state.serverProposal || (getBackend().kind === "fallback" && !state.approved) || requiresAgentTurn;
    $("revertPatch").disabled = bridgeUnavailable || !state.lastChange;
    $("approvePatch").textContent = isActiveRuntime()
      ? "Stage for VT Code turn"
      : getBackend().kind === "websocket" ? "Request VT Code approval" : "Approve patch";
    $("applyPatch").textContent = requiresAgentTurn
      ? "Apply via VT Code turn"
      : getBackend().kind === "websocket" ? "Apply after terminal approval" : "Apply approved patch";
    const directChecksUnavailable = isHeadlessBridge() || isActiveRuntime();
    $("runChecks").disabled = bridgeUnavailable || directChecksUnavailable;
    $("runChecks").title = directChecksUnavailable
      ? "Ask VT Code to run checks through the terminal"
      : "Run the selected backend checks";
    if (isHeadlessBridge()) {
      $("approvePatch").textContent = "Stage proposal";
      $("applyPatch").disabled = true;
      $("applyPatch").textContent = "Apply unavailable in headless mode";
    }
  }

  async function reviewChanges(): Promise<void> {
    const changes = collectChanges();
    if (!changes.length) throw new Error("No browser draft is ready to review; edit a file or call stage_text_edit first");
    const beforeByPath = Object.fromEntries(changes.flatMap((change) => {
      const base = snapshot(state, change.path);
      return base ? [[change.path, base.content] as const] : [];
    }));
    state.clientProposal = { changes, unified_diff: createUnifiedDiff(changes, beforeByPath) };
    state.serverProposal = null;
    state.approved = false;
    state.pendingTerminalApproval = false;
    selectTerminal("changes");
    renderProposal();
    log(`Reviewed unified diff for ${changes.length} file${changes.length === 1 ? "" : "s"}`);
    status("Review ready", "The diff is client-generated; send it to the selected backend when it looks right.");
    toast("Unified diff ready for review");
  }

  function proposalPaths(proposal: ClientProposal | PatchProposal | null | undefined): string[] {
    return [...new Set((proposal?.changes || [])
      .map((change) => change.path)
      .filter((path) => path.length > 0))];
  }

  async function recoverStaleProposal(proposal: ClientProposal | PatchProposal | null | undefined): Promise<string> {
    const affectedPaths = proposalPaths(proposal);
    state.clientProposal = null;
    state.serverProposal = null;
    state.approved = false;
    state.pendingTerminalApproval = false;
    for (const path of affectedPaths) state.conflicts.add(path);
    renderProposal();
    renderTree();
    renderTabs();
    updateEditorFooter();

    let refreshed = 0;
    for (const path of affectedPaths) {
      try {
        const fresh = await getBackend().readFile(path);
        if (typeof fresh?.content !== "string") throw new Error(`Backend returned invalid content for ${path}`);
        if (snapshotSizeBytes(fresh) > MAX_FILE_BYTES) throw new Error(`File exceeds the browser size limit: ${path}`);
        state.snapshots.set(path, fresh);
        if (state.drafts.get(path) === fresh.content) state.drafts.delete(path);
        state.conflicts.delete(path);
        refreshed += 1;
      } catch (error: unknown) {
        log(`Could not refresh ${path} after the stale proposal: ${message(error)}`);
      }
    }

    if (refreshed > 0 && state.selected !== null && affectedPaths.includes(state.selected)) renderSelectedEditor();
    renderTree();
    renderTabs();
    updateEditorFooter();
    persist();
    log(`Cleared stale proposal for ${affectedPaths.length} file${affectedPaths.length === 1 ? "" : "s"}`);
    const detail = refreshed === affectedPaths.length
      ? "The latest snapshots are loaded and your draft is preserved. Review changes again before requesting the VT Code turn."
      : "Some files could not be refreshed. Reconnect or reload them, then review changes again before requesting the VT Code turn.";
    status("Proposal became stale", detail);
    return detail;
  }

  async function requestApproval(): Promise<void> {
    if (!state.clientProposal) await reviewChanges();
    if (state.serverProposal) throw new Error("This proposal has already been sent");
    const clientProposal = state.clientProposal;
    if (!clientProposal) throw new Error("No browser draft is ready to request approval");
    selectTerminal("changes");
    try {
      state.serverProposal = await getBackend().proposeChanges(clientProposal.changes);
    } catch (error: unknown) {
      if (error instanceof BackendError && error.code === "conflict") error.proposalRecovery = await recoverStaleProposal(clientProposal);
      throw error;
    }
    if (getBackend().kind === "fallback") {
      state.approved = true;
      log("Fallback proposal approved in the browser");
      status("Patch approved", "Fallback mode changes only the in-memory project; apply is still explicit.");
    } else if (isHeadlessBridge()) {
      log("Proposal staged; headless bridge cannot authorize a filesystem apply");
      status("Proposal staged", "This standalone bridge has no terminal runtime for approval. Use explicit full-auto only for a disposable workspace.");
    } else if (isActiveRuntime()) {
      log("Proposal staged for the active VT Code session");
      status("Proposal staged", "Use Request VT Code turn; any write is performed through the terminal permission flow.");
    } else {
      state.pendingTerminalApproval = true;
      log("Requested terminal approval from VT Code");
      status("Awaiting terminal approval", "The browser cannot authorize a real filesystem write.");
    }
    renderProposal();
    if (isActiveRuntime()) openTurnComposer();
  }

  async function applyProposal(): Promise<void> {
    const serverProposal = state.serverProposal;
    if (!serverProposal) throw new Error("Send a proposal before applying it");
    if (getBackend().kind === "fallback" && !state.approved) throw new Error("Explicit approval is required");
    if (isActiveRuntime() && runtimeStatus()?.mutations_allowed === false) {
      throw new Error("Ask the active VT Code session to apply this proposal through a turn");
    }
    selectTerminal("changes");
    const before = serverProposal.changes.flatMap((change) => {
      const file = snapshot(state, change.path);
      return file ? [{ path: change.path, content: file.content }] : [];
    });
    try {
      const result = await getBackend().applyProposal(serverProposal.proposal_id);
      state.lastChange = { change_id: result.change_id, paths: result.paths, before };
      for (const path of serverProposal.changes.map((change) => change.path)) await refreshFile(path, true);
      state.clientProposal = null;
      state.serverProposal = null;
      state.approved = false;
      state.pendingTerminalApproval = false;
      persist();
      renderProposal();
      log(`Applied approved patch (${getBackend().kind === "fallback" ? "in memory" : "VT Code"})`);
      status("Change applied", getBackend().kind === "fallback" ? "Only the deterministic in-memory project changed." : "VT Code completed the authorized workspace change.");
      toast("Patch applied");
    } catch (error: unknown) {
      if (errorCode(error) === "approval_required") {
        state.pendingTerminalApproval = true;
        renderProposal();
        status("Still awaiting terminal approval", "VT Code rejected browser-only authorization; no file was changed.");
      }
      throw error;
    }
  }

  async function revertLastChange(): Promise<void> {
    if (!state.lastChange) throw new Error("There is no applied change to revert");
    selectTerminal("changes");
    const result = await getBackend().revertLastChange(state.lastChange.change_id);
    for (const path of result.paths) await refreshFile(path, true);
    log(`Reverted ${result.paths.join(", ")}`);
    state.lastChange = null;
    persist();
    renderProposal();
    status("Change reverted", "The backend validated the current file before restoring its prior snapshot.");
    toast("Last change reverted");
  }

  async function runChecks(): Promise<void> {
    if (isHeadlessBridge()) {
      throw new Error("Checks are unavailable in the headless bridge; enable explicit full-auto policy for a disposable workspace");
    }
    const result = await getBackend().runChecks(getBackend().kind === "fallback" ? undefined : "cargo check --locked");
    selectTerminal("activity");
    $("checkOutput").textContent = [result.stdout, result.stderr].filter(Boolean).join("\n") || JSON.stringify(result, null, 2);
    log(result.exit_code === 0 ? "Checks passed" : "Checks found an issue");
    status(result.exit_code === 0 ? "Checks passed" : "Checks need attention", getBackend().kind === "fallback" ? "Deterministic browser checks · no filesystem" : "Command executed by VT Code policy");
    toast(result.exit_code === 0 ? "Checks passed" : "A check failed");
  }

  async function requestTurn(): Promise<void> {
    openTurnComposer();
    if (getBackend().kind === "fallback") openConnectionPanel();
    if (dirtyPaths(state).length && !state.clientProposal) await reviewChanges();
    if (isActiveRuntime() && state.clientProposal && !state.serverProposal) await requestApproval();
    const proposal = state.serverProposal || state.clientProposal;
    const prompt = getBackend().kind === "websocket"
      ? $("promptInput").value
      : buildTurnPrompt($("promptInput").value, proposal?.unified_diff);
    let result;
    try {
      const proposalId = proposal && "proposal_id" in proposal && typeof proposal.proposal_id === "string"
        ? proposal.proposal_id
        : undefined;
      result = await getBackend().requestTurn(prompt, proposalId);
    } catch (error: unknown) {
      if (error instanceof BackendError && error.code === "conflict") error.proposalRecovery = await recoverStaleProposal(proposal);
      throw error;
    }
    if (!result.accepted) {
      const reason = result.reason || "The selected backend did not accept the agent turn.";
      $("turnOutput").textContent = reason;
      log(`VT Code turn unavailable: ${reason}`);
      status("VT Code turn unavailable", reason);
      toast("VT Code turn unavailable");
      return;
    }
    const turnId = result.turn_id || "accepted";
    log(`Requested agent turn (${turnId})`);
    $("turnOutput").textContent = result.output || "VT Code accepted the bounded request; waiting for runtime events.";
    status("Turn requested", "VT Code received the prompt and reviewed draft diff; runtime events will appear in the activity panel.");
    toast("VT Code turn requested");
  }

  async function runSelfCheck(): Promise<void> {
    if (getBackend().kind !== "fallback") {
      throw new Error("The self-check runs only in fallback mode; it must not write to a paired workspace");
    }
    if (dirtyPaths(state).length || state.serverProposal || state.lastChange) {
      throw new Error("Finish the current change before running the self-check");
    }
    if (!state.selected) throw new Error("Select a file before running the self-check");
    const path = state.selected;
    state.drafts.set(path, `${current(state, path)}\n`);
    editor.open(path, current(state, path), true);
    await reviewChanges();
    await requestApproval();
    await applyProposal();
    await runChecks();
    await revertLastChange();
    toast("Editor self-check passed");
  }

  return {
    renderProposal, reviewChanges, requestApproval, applyProposal,
    revertLastChange, runChecks, requestTurn, runSelfCheck,
  };
}
