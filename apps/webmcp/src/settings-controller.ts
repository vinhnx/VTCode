import { $, type AppElements } from "./app-elements.ts";
import { browserOrigin, webmcpDeploymentForOrigin } from "./deployments.ts";
import { saveBrowserSettings, type StorageLike } from "./persistence.ts";
import type { StatusPayload } from "./types.ts";

// Read only connection facts; pairing, backend replacement, and approvals stay in main.
type SettingsBackend = {
  readonly kind: "fallback";
  readonly statusPayload: StatusPayload | null;
} | {
  readonly kind: "websocket";
  readonly connected: boolean;
  readonly url: string;
  readonly statusPayload: StatusPayload | null;
};

interface SettingsControllerOptions {
  readonly getBackend: () => SettingsBackend;
  readonly getStorage: () => StorageLike | null;
  readonly appInstance: string;
  readonly log: (text: string) => void;
  readonly status: (title: string, detail: string) => void;
  readonly toast: (text: string) => void;
  readonly copyText: (value: string) => Promise<void>;
}

/** Bridge settings mirror the current terminal-owned status and setup inputs. */
export function createSettingsController(options: SettingsControllerOptions) {
  const { getBackend, getStorage: browserStorage, appInstance: APP_INSTANCE, log, status, toast, copyText } = options;
  let settingsPersistenceWarningShown = false;

  function formatBytes(bytes: unknown): string {
    const value = Number(bytes);
    if (!Number.isFinite(value) || value < 0) return "not reported";
    if (value >= 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(value % (1024 * 1024) ? 1 : 0)} MiB`;
    if (value >= 1024) return `${Math.round(value / 1024)} KiB`;
    return `${Math.round(value)} B`;
  }

  function renderSettings(): void {
    const backend = getBackend();
    const runtime = backend.statusPayload?.runtime;
    const settings = backend.statusPayload?.settings;
    const paired = backend.kind === "websocket" && backend.connected;
    const runtimeLabel = backend.kind === "fallback"
      ? "Fallback"
      : runtime?.turns_available === true ? "Active VT Code TUI" : "Headless workspace bridge";
    const workspace = runtime?.workspace_root || (backend.kind === "fallback" ? "Browser memory" : "Not reported");
    const connection = backend.kind === "fallback"
      ? "In-memory fallback"
      : paired ? backend.url : "Bridge disconnected";
    const origin = backend.statusPayload?.authenticated_origin || browserOrigin();
    const ttl = Number(settings?.pairing_ttl_secs);
    const frameBytes = Number(settings?.max_frame_bytes);
    const inFlight = Number(settings?.max_in_flight_requests);
    const listener = settings
      ? `${settings.host}:${settings.port === 0 ? "auto" : settings.port} · ${settings.remote_enabled ? "remote proxy" : "loopback"}`
      : backend.kind === "fallback" ? "Browser only" : "Not reported by bridge";
    const deployment = webmcpDeploymentForOrigin(browserOrigin());
    const pageLabel = deployment?.label ?? "Current browser origin";

    $("settingsConnection").textContent = connection;
    $("settingsWorkspace").textContent = workspace;
    $("settingsOrigin").textContent = origin;
    $("settingsRuntime").textContent = runtimeLabel;
    $("settingsPairingTtl").textContent = Number.isSafeInteger(ttl) && ttl > 0 ? `${ttl} seconds` : "Not reported";
    $("settingsLimits").textContent = Number.isSafeInteger(frameBytes) && Number.isSafeInteger(inFlight)
      ? `${formatBytes(frameBytes)} · ${inFlight} in flight`
      : "Not reported";
    $("settingsListener").textContent = listener;

    const syncState = $("settingsSyncState");
    syncState.textContent = backend.kind === "fallback" ? "Fallback defaults" : paired ? "Synced from VT Code" : "Pairing required";
    syncState.className = `settings-sync-state${paired ? " connected" : backend.kind === "websocket" ? " warning" : ""}`;
    $("settingsSyncNote").textContent = backend.kind === "fallback"
      ? `${pageLabel}. No bridge is paired. Browser edits stay in memory and never touch the filesystem.`
      : paired
        ? `${pageLabel}. These values are read from the paired VT Code bridge. The terminal owns workspace roots, policy, pairing, and writes.`
        : `${pageLabel}. The previous bridge is not connected. Enter a fresh one-time code; bridge settings will appear after pairing.`;
  }

  function shellQuote(value: string): string {
    return `'${value.replaceAll("'", "'\\''")}'`;
  }

  function workspacePathValue(): string {
    return $("workspacePath").value.trim() || "/absolute/path/to/workspace";
  }

  function renderWorkspaceSetup(): void {
    const origin = browserOrigin();
    $("pairingCommand").textContent = `/webmcp pair ${origin}`;
    const path = shellQuote(workspacePathValue());
    $("browserOrigin").textContent = origin;
    $("activeSetupCommand").textContent = `vtcode --workspace ${path} chat\n\nThen in the TUI:\n/webmcp pair ${origin}`;
    $("headlessSetupCommand").textContent = `vtcode webmcp serve \\\n  --origin ${origin} \\\n  --allowed-root ${path}`;
  }

  function openSettings(section: "connection" | "workspace" | null = null): void {
    if ($("confirmDialog").open) return;
    if ($("quickActionDialog").open) $("quickActionDialog").close();
    if ($("helpDialog").open) $("helpDialog").close();
    if ($("evidenceDialog").open) $("evidenceDialog").close();
    const dialog = $("settingsDialog");
    if (!dialog.open) dialog.showModal();
    if (section === "connection") {
      $("connectionPanel").open = true;
      $("workspaceSetupPanel").open = false;
    } else if (section === "workspace") {
      $("connectionPanel").open = false;
      $("workspaceSetupPanel").open = true;
    }
    renderWorkspaceSetup();
    renderSettings();
  }

  function openWorkspaceSetup(): void {
    openSettings("workspace");
    $("workspacePath").focus();
  }

  function openConnectionPanel(): void {
    openSettings("connection");
    const field = $("bridgeUrl").value.trim() ? $("pairingCode") : $("bridgeUrl");
    field.focus();
  }

  function openSettingsDialog(): void {
    if ($("settingsDialog").open) $("settingsDialog").close();
    else openSettings();
  }

  function persistBrowserSettings(): boolean {
    const saved = saveBrowserSettings(browserStorage(), APP_INSTANCE, {
      workspace_path: $("workspacePath").value.trim(),
      bridge_url: $("bridgeUrl").value.trim(),
    });
    if (!saved && !settingsPersistenceWarningShown) {
      settingsPersistenceWarningShown = true;
      status("Settings not saved", "Browser storage is unavailable or full; setup values may be lost on refresh.");
      toast("Settings could not be saved");
    } else if (saved) {
      settingsPersistenceWarningShown = false;
    }
    return saved;
  }

  async function copyPairingCommand(): Promise<void> {
    const command = $("pairingCommand").textContent?.trim() || "";
    try {
      await copyText(command);
      log("Copied the active pairing command");
      toast("Pairing command copied");
    } catch {
      status("Copy unavailable", "Select the command in the pairing panel and copy it manually.");
      toast("Copy unavailable; copy the command manually");
    }
  }

  async function copySetupCommand(id: keyof AppElements, label: string): Promise<void> {
    renderWorkspaceSetup();
    try {
      await copyText($(id).textContent?.trim() || "");
      log(`Copied ${label} setup command`);
      toast(`${label} setup copied`);
    } catch {
      status("Copy unavailable", `Select the ${label.toLowerCase()} setup command and copy it manually.`);
      toast("Copy unavailable; copy the command manually");
    }
  }

  return {
    renderSettings, renderWorkspaceSetup, openWorkspaceSetup, openConnectionPanel,
    openSettingsDialog, persistBrowserSettings, copyPairingCommand, copySetupCommand,
  };
}
