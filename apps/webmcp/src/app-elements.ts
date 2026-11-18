export interface AppElements {
  readonly [id: string]: HTMLElement;
  readonly editor: HTMLDivElement;
  readonly toast: HTMLDivElement;
  readonly activityLog: HTMLOListElement;
  readonly fileTree: HTMLDivElement;
  readonly fileTabs: HTMLDivElement;
  readonly diffView: HTMLDivElement;
  readonly quickActionList: HTMLDivElement;
  readonly workspacePath: HTMLInputElement;
  readonly bridgeUrl: HTMLInputElement;
  readonly pairingCode: HTMLInputElement;
  readonly searchInput: HTMLInputElement;
  readonly quickActionSearch: HTMLInputElement;
  readonly promptInput: HTMLTextAreaElement;
  readonly settingsDialog: HTMLDialogElement;
  readonly confirmDialog: HTMLDialogElement;
  readonly quickActionDialog: HTMLDialogElement;
  readonly helpDialog: HTMLDialogElement;
  readonly connectionPanel: HTMLDetailsElement;
  readonly workspaceSetupPanel: HTMLDetailsElement;
  readonly reviewChanges: HTMLButtonElement;
  readonly approvePatch: HTMLButtonElement;
  readonly applyPatch: HTMLButtonElement;
  readonly revertPatch: HTMLButtonElement;
  readonly reloadFile: HTMLButtonElement;
  readonly discardDraft: HTMLButtonElement;
  readonly runChecks: HTMLButtonElement;
  readonly requestTurn: HTMLButtonElement;
  readonly connectBridge: HTMLButtonElement;
  readonly closeSettings: HTMLButtonElement;
  readonly copyPairingCommand: HTMLButtonElement;
  readonly settingsButton: HTMLButtonElement;
  readonly copyActiveSetup: HTMLButtonElement;
  readonly copyHeadlessSetup: HTMLButtonElement;
  readonly showConnection: HTMLButtonElement;
  readonly selfCheck: HTMLButtonElement;
  readonly quickActions: HTMLButtonElement;
  readonly helpButton: HTMLButtonElement;
  readonly dialogConfirm: HTMLButtonElement;
  readonly evidenceDialog: HTMLDialogElement;
  readonly evidenceClient: HTMLSelectElement;
  readonly evidenceSummary: HTMLElement;
  readonly evidenceOutput: HTMLPreElement;
  readonly beginEvidence: HTMLButtonElement;
  readonly copyEvidence: HTMLButtonElement;
  readonly downloadEvidence: HTMLButtonElement;
  readonly evidenceButton: HTMLButtonElement;
  readonly closeEvidence: HTMLButtonElement;
}

export function $<K extends keyof AppElements>(id: K): AppElements[K];
export function $(id: string): HTMLElement;
export function $(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Required app element is missing: ${id}`);
  return element;
}
