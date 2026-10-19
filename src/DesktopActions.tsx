import { useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import catalog from "./action-catalog.json";

type Capability = {
  nativeName: string;
  operation: string;
  kind: string;
  available: boolean;
};
type Target = {
  id: string;
  appId: string;
  role: string;
  label: string | null;
  enabled: boolean | null;
  focused: boolean;
  capabilities: Capability[];
};
type Snapshot = {
  platform: string;
  capturedAt: number;
  accessibilityAvailable: boolean;
  labelsIncluded: boolean;
  apps: { id: string; name: string; pid: number; foreground: boolean }[];
  installedApps: { id: string; name: string; source: string }[];
  targets: Target[];
  nodesVisited: number;
  truncated: boolean;
  inventoryTruncated: boolean;
  controlsTruncated: boolean;
  warnings: string[];
  executionAvailable: boolean;
};
const pageSize = 25;
const errorMessage = (error: unknown) =>
  typeof error === "string" ? error : "Desktop discovery failed. Please retry.";

export default function DesktopActions() {
  const desktop = isTauri();
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [pid, setPid] = useState("");
  const [labels, setLabels] = useState(false);
  const [search, setSearch] = useState("");
  const [availableOnly, setAvailableOnly] = useState(false);
  const [page, setPage] = useState(0);
  const scanning = useRef(false);

  async function scan() {
    if (scanning.current) return;
    scanning.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const result = await invoke<Snapshot>("inspect_desktop", {
        targetPid: pid ? Number(pid) : null,
        includeLabels: labels,
      });
      setSnapshot(result);
      setPage(0);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      scanning.current = false;
      setBusy(false);
    }
  }

  async function requestAccess() {
    setError("");
    try {
      const enabled = await invoke<boolean>("request_desktop_access");
      setNotice(
        enabled
          ? "Access is available. Scan again to refresh."
          : "Enable Decider Desktop in System Settings → Privacy & Security → Accessibility, then scan again.",
      );
    } catch (error) {
      setError(errorMessage(error));
    }
  }

  async function copySnapshot() {
    try {
      await navigator.clipboard.writeText(JSON.stringify(snapshot, null, 2));
      setNotice("Desktop snapshot copied. It has not been sent to JEV.");
    } catch {
      setError("Could not copy the snapshot. Please retry.");
    }
  }

  const appNames = new Map(snapshot?.apps.map((app) => [app.id, app.name]));
  const query = search.toLocaleLowerCase();
  const matches =
    snapshot?.targets.filter(
      (target) =>
        (!availableOnly ||
          target.capabilities.some((capability) => capability.available)) &&
        [
          appNames.get(target.appId),
          target.role,
          target.label,
          ...target.capabilities.flatMap((capability) => [
            capability.nativeName,
            capability.operation,
          ]),
        ]
          .join(" ")
          .toLocaleLowerCase()
          .includes(query),
    ) ?? [];
  const pages = Math.max(1, Math.ceil(matches.length / pageSize));
  const currentPage = Math.min(page, pages - 1);
  const capabilities =
    snapshot?.targets.flatMap((target) => target.capabilities) ?? [];

  return (
    <>
      <div className="eyebrow">DESKTOP DISCOVERY</div>
      <h1>What can your desktop do?</h1>
      <p className="subtitle">
        Inspect what apps expose, then use the catalog to plan what comes next.
      </p>
      <section className="settings-card action-inspector">
        <div className="card-heading">
          <h2>Available on this desktop</h2>
          <span>DISCOVERY ONLY</span>
        </div>
        <p>
          A scan lists running apps, installed app entries, controls, native
          actions, and writable attributes. Nothing is executed or sent to JEV.
          Field contents and password controls are excluded.
        </p>
        <div className="inspector-options">
          <label htmlFor="scan-app">Apps to inspect</label>
          <select
            id="scan-app"
            value={pid}
            disabled={busy || !desktop}
            onChange={(event) => setPid(event.target.value)}
          >
            <option value="">All other running apps</option>
            {snapshot?.apps.map((app) => (
              <option key={app.id} value={app.pid}>
                {app.name}
                {app.foreground ? " · Foreground" : ""}
              </option>
            ))}
          </select>
          <label className="checkbox-label">
            <input
              type="checkbox"
              checked={labels}
              disabled={busy}
              onChange={(event) => setLabels(event.target.checked)}
            />{" "}
            Include control labels and window titles in the next scan
          </label>
          <p className="fine-print">
            Labels can reveal document names or other private context. Scans
            remain in memory for this view and can be copied explicitly.
          </p>
        </div>
        <div className="settings-actions">
          <button
            className="primary"
            disabled={!desktop || busy}
            onClick={() => void scan()}
          >
            {busy ? "Scanning desktop…" : "Scan desktop"}
          </button>
          {snapshot?.platform === "macos" &&
            !snapshot.accessibilityAvailable && (
              <button
                className="text-button"
                disabled={busy}
                onClick={() => void requestAccess()}
              >
                Request Accessibility access
              </button>
            )}
          <button
            className="text-button"
            disabled={!snapshot || busy}
            onClick={() => void copySnapshot()}
          >
            Copy snapshot
          </button>
        </div>
        {!desktop && (
          <p className="feedback">
            Open the desktop app to scan. The capability catalog below is
            available in this preview.
          </p>
        )}
        {error && (
          <p className="feedback error" role="alert">
            {error}
          </p>
        )}
        {notice && (
          <p className="feedback" role="status">
            {notice}
          </p>
        )}
        {busy && (
          <p role="status">
            Reading OS-exposed capabilities. Larger scans may reach the time
            limit; choose one app for more detail.
          </p>
        )}
        {snapshot && (
          <>
            <p className="scan-summary" role="status">
              {snapshot.apps.length} running apps ·{" "}
              {snapshot.installedApps.length} installed entries ·{" "}
              {snapshot.targets.length} controls ·{" "}
              {
                capabilities.filter(
                  (capability) => capability.kind !== "inspection",
                ).length
              }{" "}
              action capabilities · {snapshot.nodesVisited} elements inspected
            </p>
            <p className="fine-print">
              Captured{" "}
              {new Date(snapshot.capturedAt * 1000).toLocaleTimeString()} ·
              Labels {snapshot.labelsIncluded ? "included" : "excluded"} ·{" "}
              {!snapshot.accessibilityAvailable
                ? "Control scan needs Accessibility access. "
                : snapshot.controlsTruncated
                  ? "Control scan is partial. Select one app and scan again for more detail. "
                  : "Control scan finished within limits. App-exposed coverage can still be incomplete. "}
              {snapshot.inventoryTruncated &&
                "Installed-app inventory also reached a folder/entry limit; see coverage details."}
            </p>
            <details>
              <summary>Coverage and permission details</summary>
              <ul>
                {snapshot.warnings.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
            </details>
            <details>
              <summary>
                Installed app entries ({snapshot.installedApps.length})
              </summary>
              <ul className="app-inventory">
                {snapshot.installedApps.map((app) => (
                  <li key={app.id}>
                    {app.name} <span>· {app.source}</span>
                  </li>
                ))}
              </ul>
            </details>
            <label htmlFor="action-search">
              Search discovered apps, controls, and actions
            </label>
            <input
              id="action-search"
              type="search"
              value={search}
              onChange={(event) => {
                setSearch(event.target.value);
                setPage(0);
              }}
              placeholder="For example: menu, Press, Scroll, Value…"
            />
            <label className="checkbox-label">
              <input
                type="checkbox"
                checked={availableOnly}
                onChange={(event) => {
                  setAvailableOnly(event.target.checked);
                  setPage(0);
                }}
              />{" "}
              Show controls with an available capability
            </label>
            <p className="fine-print">
              “Available” reflects reported enabled/visible state, not a tested
              action. No desktop action executors are connected yet. Inspection
              patterns are listed separately from actions.
            </p>
            <div className="discovered-targets">
              {matches
                .slice(currentPage * pageSize, (currentPage + 1) * pageSize)
                .map((target) => (
                  <article className="target-card" key={target.id}>
                    <h3>
                      {target.label || target.role}{" "}
                      {target.focused && <span>· Focused</span>}
                    </h3>
                    <p>
                      {appNames.get(target.appId) ?? "Application"} ·{" "}
                      {target.role} ·{" "}
                      {target.enabled === null
                        ? "Enabled state unknown"
                        : target.enabled
                          ? "Enabled"
                          : "Disabled"}
                    </p>
                    <ul>
                      {target.capabilities
                        .filter(
                          (capability) =>
                            !availableOnly || capability.available,
                        )
                        .map((capability, index) => (
                          <li key={`${capability.nativeName}:${index}`}>
                            <strong>{capability.operation}</strong>{" "}
                            <span>
                              · {capability.nativeName} ·{" "}
                              {capability.kind === "inspection"
                                ? "Inspect"
                                : "Action"}{" "}
                              ·{" "}
                              {capability.available
                                ? "Available"
                                : target.enabled === null
                                  ? "Availability unknown"
                                  : "Unavailable"}
                            </span>
                          </li>
                        ))}
                    </ul>
                  </article>
                ))}
            </div>
            {!matches.length && (
              <p>
                {snapshot.accessibilityAvailable
                  ? "No matching capabilities were exposed. Try another app or refresh the scan."
                  : "Grant Accessibility access to discover controls and their actions."}
              </p>
            )}
            {matches.length > pageSize && (
              <div className="settings-actions">
                <button
                  disabled={currentPage === 0}
                  onClick={() => setPage(currentPage - 1)}
                >
                  Previous
                </button>
                <span>
                  Page {currentPage + 1} of {pages} · {matches.length} controls
                </span>
                <button
                  disabled={currentPage + 1 >= pages}
                  onClick={() => setPage(currentPage + 1)}
                >
                  Next
                </button>
              </div>
            )}
          </>
        )}
      </section>
      <section className="settings-card capability-catalog">
        <div className="card-heading">
          <h2>Capability catalog</h2>
          <span>
            {catalog.reduce((count, group) => count + group.actions.length, 0)}{" "}
            ACTION TYPES
          </span>
        </div>
        <p>
          This is the implementation map. These capabilities need executors and
          permission checks before JEV can use them. A discovered menu command
          may cover an app-specific action that is not named here.
        </p>
        {catalog.map((group) => (
          <details key={group.category}>
            <summary>
              {group.category} <span>· {group.actions.length}</span>
            </summary>
            <p className="fine-print">{group.source}</p>
            <ul>
              {group.actions.map((action) => (
                <li key={action}>{action}</li>
              ))}
            </ul>
          </details>
        ))}
      </section>
    </>
  );
}
