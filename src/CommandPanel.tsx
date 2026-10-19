import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import SpeechPanel from "./SpeechPanel";

type Update = {
  commandId: string;
  phase: "classifying" | "review" | "done" | "error" | "canceled" | "no_action";
  message: string;
  dictation?: string | null;
};
const message = (error: unknown) =>
  typeof error === "string"
    ? error
    : "The command could not complete. Please retry.";

export default function CommandPanel({
  keyConfigured,
  onDictation,
}: {
  keyConfigured: boolean | null;
  onDictation: (text: string) => void;
}) {
  const [text, setText] = useState("");
  const [shareLabels, setShareLabels] = useState(false);
  const [ready, setReady] = useState(false);
  const [update, setUpdate] = useState<Update | null>(null);
  const [active, setActive] = useState(false);
  const [recording, setRecording] = useState(false);
  const resultView = useRef<HTMLDivElement | null>(null);
  const session = useRef<string | null>(null);
  const mounted = useRef(false);
  const callback = useRef(onDictation);
  callback.current = onDictation;
  const desktop = isTauri();

  useEffect(() => {
    mounted.current = true;
    if (!desktop)
      return () => {
        mounted.current = false;
      };
    const subscription = listen<Update>("command-update", ({ payload }) => {
      if (!mounted.current || payload.commandId !== session.current) return;
      setUpdate(payload);
      if (!["classifying", "review"].includes(payload.phase)) {
        // Claim terminal events once, so a duplicate cannot insert dictation twice.
        session.current = null;
        setActive(false);
        if (payload.phase === "done" && payload.dictation)
          callback.current(payload.dictation);
      }
    });
    void subscription
      .then((stop) => {
        if (mounted.current) setReady(true);
        else stop();
      })
      .catch(() => {
        if (mounted.current)
          setUpdate({
            commandId: "",
            phase: "error",
            message: "Could not connect to desktop commands. Reopen the app.",
          });
      });
    return () => {
      mounted.current = false;
      void subscription.then((stop) => stop()).catch(() => {});
      const commandId = session.current;
      session.current = null;
      if (commandId)
        void invoke("resolve_desktop_command", {
          commandId,
          execute: false,
        }).catch(() => {});
    };
  }, [desktop]);

  useEffect(() => {
    if (update)
      resultView.current?.scrollIntoView({
        block: "nearest",
        behavior: "smooth",
      });
  }, [update?.commandId, update?.phase]);

  async function run(transcript: string) {
    if (!ready || !keyConfigured || session.current || !transcript.trim())
      return;
    const commandId = crypto.randomUUID();
    session.current = commandId;
    setActive(true);
    setUpdate({
      commandId,
      phase: "classifying",
      message: "Choosing an action with JEV…",
    });
    try {
      await invoke("start_desktop_command", {
        commandId,
        transcript: transcript.trim(),
        shareControlLabels: shareLabels,
      });
    } catch (error) {
      if (mounted.current && session.current === commandId) {
        session.current = null;
        setActive(false);
        setUpdate({ commandId, phase: "error", message: message(error) });
      }
    }
  }
  async function resolve(execute: boolean) {
    const commandId = session.current;
    if (!commandId) return;
    // Disable repeat approvals immediately; native state also enforces one execution.
    setUpdate({
      commandId,
      phase: "classifying",
      message: execute ? "Running the selected action…" : "Canceling…",
    });
    try {
      await invoke("resolve_desktop_command", { commandId, execute });
    } catch (error) {
      if (mounted.current && session.current === commandId) {
        session.current = null;
        setActive(false);
        setUpdate({ commandId, phase: "error", message: message(error) });
      }
    }
  }

  return (
    <>
      <div className="command-options">
        <p className="fine-print">
          Commands send your transcript and app names to TypeSafe using your
          saved JEV key. Audio stays on this device. App opening and draft
          dictation run directly; control actions are shown for review.
        </p>
        <label className="command-consent">
          <input
            type="checkbox"
            checked={shareLabels}
            disabled={active || recording}
            onChange={(event) => setShareLabels(event.target.checked)}
          />
          Allow JEV to read control labels in the app I name
        </label>
        <p className="fine-print">
          Optional for clicking or focusing controls. Labels can include window
          or document names. Field contents and password controls are excluded.
        </p>
        {keyConfigured === null ? (
          <p className="feedback">
            Checking secure key access. If macOS shows a Keychain prompt for
            Decider, allow access there.
          </p>
        ) : (
          !keyConfigured && (
            <p className="feedback">
              Save your JEV API key in Settings to use Command mode.
            </p>
          )
        )}
      </div>
      <SpeechPanel
        commandMode
        disabled={!keyConfigured || !ready || active}
        onActiveChange={setRecording}
        onTranscript={(transcript) => {
          setText(transcript);
          void run(transcript);
        }}
      />
      <section
        className="settings-card command-panel"
        aria-label="Desktop command"
      >
        <div className="card-heading">
          <h2>Your command</h2>
          <span>JEV CLASSIFIER</span>
        </div>
        <p>
          Say “Open Safari” or “Type remember the milk.” To use a control, name
          its app and label, for example “Click Reload in Google Chrome.” One
          action per recording.
        </p>
        <label htmlFor="spoken-command">Your command</label>
        <textarea
          id="spoken-command"
          value={text}
          maxLength={4000}
          disabled={active || recording}
          onChange={(event) => {
            setText(event.target.value);
            setUpdate(null);
          }}
          placeholder="Your spoken command appears here. You can also type one to test JEV."
        />
        <div className="speech-actions">
          <button
            className="primary"
            disabled={
              !desktop ||
              !ready ||
              !keyConfigured ||
              active ||
              recording ||
              !text.trim()
            }
            onClick={() => void run(text)}
          >
            Run command
          </button>
          {active && (
            <button className="text-button" onClick={() => void resolve(false)}>
              Cancel command
            </button>
          )}
        </div>
        {update && (
          <div
            ref={resultView}
            className={update.phase === "error" ? "feedback error" : "feedback"}
            role={update.phase === "error" ? "alert" : "status"}
          >
            <p>{update.message}</p>
            {update.phase === "review" && (
              <button className="primary" onClick={() => void resolve(true)}>
                Run selected action
              </button>
            )}
          </div>
        )}
      </section>
    </>
  );
}
