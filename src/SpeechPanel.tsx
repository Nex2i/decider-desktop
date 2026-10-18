import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import catalog from "../src-tauri/models.json";

type ModelInfo = {
  id: string;
  name: string;
  description: string;
  downloadBytes: number;
  installed: boolean;
  streaming: boolean;
  maxRecordingSeconds: number;
};
type Progress = { modelId: string; downloaded: number; total: number };
type SpeechUpdate = {
  sessionId: string;
  phase:
    | "loading"
    | "listening"
    | "partial"
    | "finalizing"
    | "final"
    | "error"
    | "stopped";
  text: string;
  message?: string;
};
type Phase = "idle" | "loading" | "listening" | "finalizing";
const previewModels: ModelInfo[] = catalog.map((model) => ({
  ...model,
  downloadBytes: model.files.reduce((sum, file) => sum + file.size, 0),
  installed: false,
  streaming: true,
}));
const megabytes = (bytes: number) =>
  bytes >= 1_000_000_000
    ? `${(bytes / 1_000_000_000).toFixed(2)} GB`
    : `${Math.ceil(bytes / 1_000_000)} MB`;
const message = (error: unknown) =>
  typeof error === "string" ? error : "Something went wrong. Please retry.";

export default function SpeechPanel({
  onTranscript,
}: {
  onTranscript: (text: string) => void;
}) {
  const desktop = isTauri();
  const [models, setModels] = useState<ModelInfo[]>(previewModels);
  const [modelId, setModelId] = useState(previewModels[0].id);
  const [phase, setPhase] = useState<Phase>("idle");
  const [liveText, setLiveText] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [ready, setReady] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [canceling, setCanceling] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [removing, setRemoving] = useState(false);
  const session = useRef<string | null>(null);
  const canceled = useRef(false);
  const stopping = useRef(false);
  const mounted = useRef(false);
  const downloadActive = useRef(false);
  const callback = useRef(onTranscript);
  callback.current = onTranscript;
  const selected = models.find((model) => model.id === modelId)!;
  const active = phase !== "idle";

  async function refreshModels() {
    const result = await invoke<ModelInfo[]>("list_models");
    if (mounted.current) setModels(result);
    return result;
  }

  useEffect(() => {
    mounted.current = true;
    if (!desktop)
      return () => {
        mounted.current = false;
      };
    let disposed = false;
    const subscriptions = Promise.all([
      listen<Progress>("model-download-progress", ({ payload }) => {
        if (!disposed && downloadActive.current) setProgress(payload);
      }),
      listen<SpeechUpdate>("speech-update", ({ payload }) => {
        if (disposed || payload.sessionId !== session.current) return;
        if (payload.phase === "stopped") {
          session.current = null;
          setPhase("idle");
          setLiveText("");
        } else if (payload.phase === "error") {
          setError(
            payload.message ?? "Speech recognition failed. Please retry.",
          );
        } else if (payload.phase === "final") {
          if (!canceled.current) {
            if (payload.text.trim()) {
              callback.current(payload.text.trim());
              setNotice(
                "Transcript added to your text. Nothing was sent to JEV.",
              );
            } else
              setNotice(
                payload.message ?? "No speech recognized. Please retry.",
              );
          }
        } else {
          if (payload.text) setLiveText(payload.text);
          if (!stopping.current)
            setPhase(payload.phase === "partial" ? "listening" : payload.phase);
        }
      }),
    ]);
    void subscriptions
      .then(async (unlisten) => {
        if (disposed) {
          unlisten.forEach((stop) => stop());
          return;
        }
        try {
          const result = await refreshModels();
          if (!disposed) {
            setModelId(
              result.find((model) => model.installed)?.id ?? result[0].id,
            );
            setReady(true);
          }
        } catch (error) {
          if (!disposed) setError(message(error));
        }
      })
      .catch(() => {
        if (!disposed)
          setError(
            "Could not connect to the speech engine. Restart the desktop app.",
          );
      });
    return () => {
      disposed = true;
      mounted.current = false;
      void subscriptions
        .then((unlisten) => unlisten.forEach((stop) => stop()))
        .catch(() => {});
      if (session.current) {
        canceled.current = true;
        void invoke("stop_transcription", {
          sessionId: session.current,
          cancel: true,
        }).catch(() => {});
      }
      if (downloadActive.current)
        void invoke("cancel_model_download").catch(() => {});
    };
  }, [desktop]);

  useEffect(() => {
    if (phase !== "listening") return;
    const timer = window.setInterval(
      () =>
        setElapsed((seconds) =>
          Math.min(selected.maxRecordingSeconds, seconds + 1),
        ),
      1000,
    );
    return () => window.clearInterval(timer);
  }, [phase, selected.maxRecordingSeconds]);

  async function stop(cancel: boolean) {
    if (!session.current) return;
    canceled.current = cancel;
    stopping.current = true;
    setPhase("finalizing");
    try {
      await invoke("stop_transcription", {
        sessionId: session.current,
        cancel,
      });
      if (cancel && mounted.current)
        setNotice("Recording canceled. Your existing text was kept.");
    } catch (error) {
      if (mounted.current) setError(message(error));
    }
  }

  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && session.current) void stop(true);
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, []);

  async function download() {
    setError("");
    setNotice("");
    setProgress(null);
    setDownloading(true);
    setCanceling(false);
    downloadActive.current = true;
    try {
      await invoke("download_model", { modelId });
      if (mounted.current) {
        await refreshModels();
        setNotice("Model downloaded and verified. You can start recording.");
      }
    } catch (error) {
      if (mounted.current) {
        if (message(error) === "Download canceled.")
          setNotice("Download canceled.");
        else setError(message(error));
      }
    } finally {
      downloadActive.current = false;
      if (mounted.current) {
        setDownloading(false);
        setCanceling(false);
      }
    }
  }

  async function start() {
    setError("");
    setNotice("");
    setLiveText("");
    setElapsed(0);
    canceled.current = false;
    stopping.current = false;
    const id = crypto.randomUUID();
    session.current = id;
    setPhase("loading");
    try {
      await invoke("start_transcription", { modelId, sessionId: id });
    } catch (error) {
      if (mounted.current && session.current === id) {
        session.current = null;
        setPhase("idle");
        setError(message(error));
      }
    }
  }

  async function remove() {
    if (!window.confirm(`Remove the downloaded ${selected.name} model?`))
      return;
    setError("");
    setNotice("");
    setRemoving(true);
    try {
      await invoke("remove_model", { modelId });
      await refreshModels();
    } catch (error) {
      if (mounted.current) setError(message(error));
    } finally {
      if (mounted.current) setRemoving(false);
    }
  }

  const percentage =
    progress?.modelId === modelId
      ? Math.min(100, Math.floor((progress.downloaded / progress.total) * 100))
      : 0;
  return (
    <section className="speech-panel" aria-label="Local speech test">
      <div className="card-heading">
        <h2>Local speech test</h2>
        <span>ON YOUR DEVICE</span>
      </div>
      <label htmlFor="speech-model">Speech model</label>
      <select
        id="speech-model"
        value={modelId}
        disabled={active || downloading || removing}
        onChange={(event) => {
          setModelId(event.target.value);
          setError("");
          setNotice("");
          setProgress(null);
        }}
      >
        {models.map((model) => (
          <option key={model.id} value={model.id}>
            {model.name} · {megabytes(model.downloadBytes)}
            {model.installed ? " · Downloaded" : ""}
          </option>
        ))}
      </select>
      <p className="model-description">{selected.description}</p>
      <div className="speech-actions">
        {!selected.installed ? (
          <button
            className="primary"
            disabled={!desktop || !ready || downloading || active || removing}
            onClick={() => void download()}
          >
            {downloading
              ? `Downloading · ${percentage}%`
              : `Download model · ${megabytes(selected.downloadBytes)}`}
          </button>
        ) : (
          <>
            <button
              className="primary"
              disabled={!desktop || !ready || active || downloading || removing}
              onClick={() => void start()}
            >
              Start recording
            </button>
            <button
              className="text-button"
              disabled={active || downloading || removing}
              onClick={() => void remove()}
            >
              {removing ? "Removing…" : "Remove download"}
            </button>
          </>
        )}
        {downloading && (
          <button
            className="secondary"
            disabled={canceling}
            onClick={() => {
              setCanceling(true);
              void invoke("cancel_model_download").catch((error) => {
                setError(message(error));
                setCanceling(false);
              });
            }}
          >
            {canceling ? "Canceling…" : "Cancel download"}
          </button>
        )}
        {active && (
          <>
            <button
              className="secondary"
              disabled={phase === "finalizing"}
              onClick={() => void stop(false)}
            >
              Stop & keep text
            </button>
            <button className="text-button" onClick={() => void stop(true)}>
              Cancel
            </button>
          </>
        )}
      </div>
      {downloading && (
        <progress
          aria-label="Model download progress"
          value={percentage}
          max={100}
        />
      )}
      {active && (
        <div className="live-transcript" role="status">
          <div className="listening-label">
            <span className="small-dot" />
            {phase === "loading"
              ? "Preparing live transcription…"
              : phase === "finalizing"
                ? "Finishing…"
                : `Listening · ${elapsed}s / ${selected.maxRecordingSeconds}s`}
          </div>
          <p>
            {liveText || "Your speech will appear here as it is recognized."}
          </p>
        </div>
      )}
      <p className="fine-print">
        {desktop
          ? `Live transcription only. Uses your default microphone and stops after ${selected.maxRecordingSeconds} seconds. Stop keeps the live transcript below; Escape cancels.`
          : "Open the desktop app to download models and use your microphone. The browser preview cannot run the native speech engine."}
      </p>
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
    </section>
  );
}
