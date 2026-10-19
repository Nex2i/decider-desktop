import { useCallback, useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import "./App.css";
import SpeechPanel from "./SpeechPanel";
import DesktopActions from "./DesktopActions";

type AppStatus = {
  platform: string;
  version: string;
  jevKeyConfigured: boolean;
};
type JevConnection = { model: string; checkedAt: number };

export default function App() {
  const [page, setPage] = useState<"workspace" | "actions" | "settings">(
    "workspace",
  );
  const [mode, setMode] = useState<"dictation" | "command">("dictation");
  const [draft, setDraft] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [connection, setConnection] = useState<JevConnection | null>(null);
  const desktop = isTauri();

  async function refreshStatus() {
    setConnection(null);
    setStatus(await invoke<AppStatus>("app_status"));
  }

  useEffect(() => {
    if (desktop)
      void refreshStatus().catch(() =>
        setError("Could not read desktop setup. Open Settings to retry."),
      );
  }, [desktop]);

  useEffect(() => {
    if (!draft) return;
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault();
    };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [draft]);

  async function updateKey(remove: boolean) {
    setConnection(null);
    setBusy(true);
    setError("");
    setNotice("");
    try {
      if (remove) await invoke("remove_jev_key");
      else setStatus(await invoke<AppStatus>("save_jev_key", { apiKey }));
      setApiKey("");
      if (remove) await refreshStatus();
      setNotice(
        remove
          ? "JEV key removed."
          : "JEV key saved securely. Use Test connection to verify access.",
      );
    } catch (error) {
      setError(
        typeof error === "string"
          ? error
          : "Could not update JEV setup. Check that your OS credential store is available, then retry.",
      );
    } finally {
      setBusy(false);
    }
  }

  async function testConnection() {
    setBusy(true);
    setTesting(true);
    setConnection(null);
    setError("");
    setNotice("");
    try {
      const result = await invoke<JevConnection>("test_jev_connection");
      setConnection(result);
      setNotice(
        `JEV connection verified. Your account can access ${result.model}.`,
      );
    } catch (error) {
      setError(
        typeof error === "string"
          ? error
          : "Could not verify the JEV connection. Please retry.",
      );
    } finally {
      setTesting(false);
      setBusy(false);
    }
  }

  async function copyDraft() {
    try {
      await navigator.clipboard.writeText(draft);
      setNotice("Copied to clipboard.");
      setError("");
    } catch {
      setError(
        "Clipboard access is unavailable. Select the text and copy it manually.",
      );
    }
  }

  const appendTranscript = useCallback((text: string) => {
    setDraft((existing) => (existing.trim() ? `${existing}\n\n${text}` : text));
  }, []);

  const words = draft.trim() ? draft.trim().split(/\s+/).length : 0;

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">d</span>
          <div>
            decider<span className="brand-caption">DESKTOP</span>
          </div>
        </div>
        <div className="nav-label">YOUR SPACE</div>
        <nav aria-label="Main navigation">
          <button
            className={page === "actions" ? "nav-item active" : "nav-item"}
            onClick={() => setPage("actions")}
          >
            <span aria-hidden="true">⌘</span> Desktop actions
          </button>
          <button
            className={page === "workspace" ? "nav-item active" : "nav-item"}
            onClick={() => setPage("workspace")}
          >
            <span aria-hidden="true">◉</span> Workspace
          </button>
          <button
            className={page === "settings" ? "nav-item active" : "nav-item"}
            onClick={() => setPage("settings")}
          >
            <span aria-hidden="true">⚙</span> Settings
          </button>
        </nav>
        <div className="sidebar-bottom">
          <span className="small-dot" /> Initial build{" "}
          <span className="version">v{status?.version ?? "0.1.0"}</span>
          <p>Your voice. A little less friction.</p>
        </div>
      </aside>
      <main>
        <header>
          <span>PERSONAL WORKSPACE</span>
          <span className="environment">
            {desktop
              ? status?.platform === "windows"
                ? "Windows"
                : status?.platform === "macos"
                  ? "macOS"
                  : "Desktop"
              : "Browser preview"}
          </span>
        </header>
        <div className="page-content">
          {page === "actions" ? (
            <DesktopActions />
          ) : page === "workspace" ? (
            <>
              <div className="eyebrow">A LITTLE SPACE TO THINK</div>
              <h1>Say what’s on your mind.</h1>
              <p className="subtitle">
                Capture a thought. Open an app. Keep moving.
              </p>
              <div
                className="mode-switch"
                role="group"
                aria-label="Interaction mode"
              >
                <button
                  aria-pressed={mode === "dictation"}
                  className={mode === "dictation" ? "selected" : ""}
                  onClick={() => setMode("dictation")}
                >
                  Dictation
                </button>
                <button
                  aria-pressed={mode === "command"}
                  className={mode === "command" ? "selected" : ""}
                  onClick={() => setMode("command")}
                >
                  Command
                </button>
              </div>
              {mode === "dictation" ? (
                <SpeechPanel onTranscript={appendTranscript} />
              ) : (
                <section className="capture-card">
                  <h2>Desktop commands are coming next</h2>
                  <p>
                    Switch to Dictation to download a local model and test your
                    speech. JEV action routing is not connected yet.
                  </p>
                </section>
              )}
              <section className="draft-card">
                <div className="card-heading">
                  <label htmlFor="draft">Your text</label>
                  <span>SESSION ONLY</span>
                </div>
                <textarea
                  id="draft"
                  value={draft}
                  onChange={(event) => setDraft(event.target.value)}
                  placeholder="Your finalized speech will appear here. You can also type, edit, and copy your text."
                />
                <div className="draft-footer">
                  <span>
                    {words} {words === 1 ? "word" : "words"}
                  </span>
                  <div>
                    <button
                      className="text-button"
                      disabled={!draft}
                      onClick={() => {
                        if (window.confirm("Clear all text in this draft?")) {
                          setDraft("");
                          setNotice("Draft cleared.");
                        }
                      }}
                    >
                      Clear
                    </button>
                    <button
                      className="secondary"
                      disabled={!draft}
                      onClick={() => void copyDraft()}
                    >
                      Copy text
                    </button>
                  </div>
                </div>
              </section>
              <p className="privacy-note">
                Drafts stay in memory for this session. Copy anything you want
                to keep before closing.
              </p>
            </>
          ) : (
            <>
              <div className="eyebrow">MAKE IT YOURS</div>
              <h1>A few things to set up.</h1>
              <p className="subtitle">
                Bring your own key. Choose how your voice is transcribed.
              </p>
              <section className="settings-card">
                <div className="card-heading">
                  <h2>JEV connection</h2>
                  <span>
                    {connection
                      ? "CONNECTION VERIFIED"
                      : status?.jevKeyConfigured
                        ? "KEY SAVED · NOT VALIDATED"
                        : "NOT CONFIGURED"}
                  </span>
                </div>
                <p>
                  Use your own TypeSafe JEV API key. Usage is billed to your
                  provider account. Test your saved key to check JEV access.
                  Desktop command routing is coming next.
                </p>
                <label htmlFor="api-key">
                  {status?.jevKeyConfigured ? "Replace API key" : "API key"}
                </label>
                <input
                  id="api-key"
                  type="password"
                  autoComplete="off"
                  spellCheck={false}
                  value={apiKey}
                  disabled={!desktop || busy}
                  onChange={(event) => setApiKey(event.target.value)}
                  placeholder={
                    desktop
                      ? "Enter your JEV API key"
                      : "Open the desktop app to configure your key"
                  }
                />
                <div className="settings-actions">
                  <button
                    className="primary"
                    disabled={
                      !desktop ||
                      busy ||
                      !apiKey.trim() ||
                      apiKey.trim().length > 4096
                    }
                    onClick={() => void updateKey(false)}
                  >
                    {busy && !testing ? "Updating…" : "Save key securely"}
                  </button>
                  <button
                    className="text-button"
                    disabled={
                      !desktop || busy || !status?.jevKeyConfigured || !!apiKey
                    }
                    onClick={() => void testConnection()}
                  >
                    {testing ? "Testing connection…" : "Test connection"}
                  </button>
                  <button
                    className="text-button"
                    disabled={!desktop || busy || !status?.jevKeyConfigured}
                    onClick={() => {
                      if (window.confirm("Remove your saved JEV API key?"))
                        void updateKey(true);
                    }}
                  >
                    Remove key
                  </button>
                  <button
                    className="text-button"
                    disabled={!desktop || busy}
                    onClick={() =>
                      void refreshStatus()
                        .then(() => setError(""))
                        .catch(() =>
                          setError(
                            "Could not read desktop setup. Please retry.",
                          ),
                        )
                    }
                  >
                    Refresh status
                  </button>
                </div>
                <p className="fine-print">
                  {connection
                    ? `Verified this session at ${new Date(connection.checkedAt * 1000).toLocaleTimeString()}. Model access: ${connection.model}. Re-test after reopening the app.`
                    : "Test connection checks your saved key with TypeSafe’s model list. It sends no speech or draft text and makes no inference request. Save any replacement key before testing."}
                </p>
                <p className="fine-print">
                  Saved in macOS Keychain or Windows Credential Manager. The
                  saved key is never sent back to the interface or stored in
                  browser storage.
                </p>
              </section>
              <section className="settings-card">
                <div className="card-heading">
                  <h2>Speech recognition</h2>
                  <span>LOCAL TEST AVAILABLE</span>
                </div>
                <p>
                  Open Workspace → Dictation to choose a local model, download
                  it, and test live transcription. Remote transcription is
                  planned.
                </p>
                <div className="provider-row">
                  <span>Local transcription</span>
                  <span>Configure in Workspace</span>
                </div>
                <div className="provider-row">
                  <span>Remote transcription</span>
                  <span>Not configured</span>
                </div>
              </section>
              <p className="privacy-note">
                Local speech tests keep audio on your device and do not call
                JEV.
              </p>
            </>
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
        </div>
      </main>
    </div>
  );
}
