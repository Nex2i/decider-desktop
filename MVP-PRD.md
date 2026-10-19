# Decider Desktop — MVP Product Requirements

Status: MVP in progress
Date: October 18, 2026
Platforms: macOS and Windows
Working name: Decider Desktop

Command routing update (October 19, 2026): Workspace → Command now connects finalized local speech (or typed commands) to JEV typed Choice classification. Known app opening/activation and literal in-app dictation run directly; a supported named native control is presented for explicit review with a 30-second expiry before dispatch. Control-label sharing is opt-in. No arbitrary model-generated code, paths, keyboard input, or multi-step workflows execute. A successful inference authenticates the saved key; separate Test connection remains available. Remote STT, external text insertion, global shortcuts, and the rest of the catalog remain planned. See README for supported operations and limits.

Desktop discovery update (October 19, 2026): The Desktop actions view adds a read-only inventory of running apps, standard-location installed apps, and OS-exposed control capabilities, plus a 116-type implementation catalog. macOS uses accessibility action/writable-attribute enumeration; Windows uses standard UI Automation patterns. Coverage limits and permission gaps are explicit. Snapshots are local and labels are opt-in. This discovery layer does not execute actions or send desktop context to JEV yet. See [Desktop action catalog and discovery](DESKTOP-ACTIONS.md).

Implementation update (October 18, 2026): The app now offers a local streaming transcription test with downloadable Zipformer profiles, Nemotron streaming, and native Qwen3-ASR streaming. Microphone capture, partial/final transcript display, verified model downloads, cancellation, and app-managed model removal are implemented. JEV keys use verified OS credential storage, and an explicit connection test checks authenticated JEV model access without inference or transcript uploads. JEV desktop actions and remote STT remain planned. Automatic CI runs checks; installer packaging and publishing require a manually dispatched version workflow.

## 1. Product summary

Build a cross-platform desktop app for macOS and Windows that turns speech into a small set of useful desktop actions. A speech-to-text (STT) engine transcribes the user’s voice, JEV selects a supported action from structured choices, and the app executes that action through controlled native integrations.

Phase 1 supports two outcomes:

1. Open or activate an installed application by speaking its name.
2. Dictate text into a text area provided inside Decider Desktop.

The MVP validates whether speech recognition plus JEV decision routing feels fast, understandable, and reliable enough for everyday use.

## 2. Intended user and problem

The initial user is a Mac or Windows PC owner who wants to perform simple desktop tasks and capture text by speaking instead of navigating or typing. Existing manual workflows interrupt their current activity; a general desktop agent introduces more complexity than these first tasks require.

The first release is a single-user, English-language product. It does not require a Decider account or a hosted Decider backend.

## 3. Scope assumptions

- “Provided text area” means an editable field within this app. Inserting text into arbitrary third-party apps is a later phase.
- Speech capture is explicitly started and stopped by the user. Always-on listening and wake words are excluded.
- Local and remote STT are interchangeable transcription paths. Local transcription does not make the full command workflow offline: JEV is treated as a remote service.
- JEV chooses among predefined actions and installed-app candidates. The app owns execution, validation, and user feedback.
- JEV uses a user-supplied API key. The desktop app calls JEV directly using that key; the MVP does not provide a shared key, proxy service, or managed JEV billing. Any JEV usage charges belong to the user’s provider account.
- Initial hardware targets: Apple Silicon Macs and x64 Windows PCs. Confirm minimum supported OS versions and reference hardware for each during feasibility. Linux, Intel Macs, and Windows on ARM are deferred.
- These are proposed product decisions for an initial MVP, subject to implementation validation.

## 4. Goals and success criteria

| Goal                                        | MVP release target                                                                                                                                                         |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Correctly open an unambiguous installed app | At least 95% end-to-end success across 100 labeled launch utterances on each platform’s reference setup                                                                    |
| Capture useful dictated text                | At most 10% word error rate on a fixed 50-utterance English sample in a quiet room; report results separately for each platform and STT mode                               |
| Avoid incorrect actions                     | Zero incorrect executions across a separate 100-case suite of ambiguous, unsupported, silent, and noisy inputs; clarification or no action is acceptable                   |
| Feel responsive                             | p95 of stop-recording to confirmed result at most 3 seconds for launch commands and dictation clips up to 15 seconds, with a warm local model or healthy remote connection |
| Recover clearly                             | Every tested permission, model, connectivity, and execution failure produces a visible explanation and retry path                                                          |

Measure full workflow latency, including transcription and JEV. Track cold model startup and download time separately. Run all release criteria independently on macOS and Windows; an aggregate result must not hide a failing platform. These are validation targets, not claims about vendor performance.

## 5. Primary user journeys

### First launch

1. The user sees a brief explanation of recording, transcription, and action selection.
2. The user chooses local or remote STT and sees what data leaves the device for that configuration.
3. For local STT, the app offers a supported model download or selection of a compatible existing model. Show download size, destination, progress, and cancel/retry controls.
4. For remote STT, the user configures credentials and tests the connection. Separately, the user enters their own JEV API key and tests it before enabling Command mode. They can skip JEV setup and use Dictation mode with a configured STT engine.
5. The app requests microphone permission when the user starts their first recording.
6. A test utterance demonstrates opening an app or capturing text.

### Open an application

1. The user activates the app through its macOS menu bar entry, Windows system tray entry, or configurable global shortcut.
2. They start recording and say, for example, “Open Safari,” then stop recording.
3. The app transcribes the audio and asks JEV to choose the action and, when relevant, an installed-app candidate.
4. A confident, valid result opens Safari or brings its existing window forward. The app shows the recognized command and outcome.
5. If the app name is ambiguous, the user chooses from visible candidates. If no installed match exists, the app explains that it cannot find the app.

### Dictate into the provided text area

1. The user selects Dictation mode and records, for example, “Open Safari after lunch.”
2. The transcript is inserted literally into the app’s text area. Command-like wording is treated as text in this mode.
3. The user can edit, select, copy, or clear the text. Each new recording appends a new paragraph without replacing existing content.
4. The field remains available if JEV or the network is unavailable when local STT is ready.

### Explicit dictation from Command mode

1. The user says, “Type: Remember to book the tickets.”
2. JEV classifies the utterance as dictation. A deterministic parser removes only a documented leading marker such as “Type:” or “Dictate:”.
3. The remaining transcript is appended to the provided text area. JEV does not rewrite or generate the text.
4. If a dictation marker cannot be identified reliably, show the transcript for review instead of guessing which words to remove.

## 6. Functional requirements

### Capture and transcription

- Provide clear Idle, Recording, Transcribing, Deciding, Needs clarification, Executing, Done, and Error states. Dictation mode skips Deciding.
- Provide explicit start/stop controls and a configurable global shortcut; Escape cancels pending work.
- Allow one active recording or action at a time. Repeated shortcuts must not create duplicate requests or executions.
- Limit recordings to 60 seconds, show the limit, and stop capture visibly at that limit.
- Support one local STT engine/model configuration and one remote STT provider in the MVP. Final implementations are selected during feasibility work.
- Offer an explicit provider setting; never silently switch from local transcription to remote transcription.
- Handle no speech, unintelligible audio, missing microphone access, model load failures, and remote failures without executing an action.
- Require live partial transcription from every selectable STT model, including future remote providers. Exclude after-stop/batch-only recognition and automatic offline-mode fallbacks. Stop may flush a final streaming chunk.
- Preserve STT wording and available punctuation. Translation, summarization, and stylistic rewriting are excluded.

### JEV decision routing

- Require a configured, successfully tested user-supplied JEV API key before enabling Command mode. If the key is missing or rejected, explain how to add or replace it; keep independent Dictation mode available.
- In Command mode, route nonempty transcripts through a JEV adapter using typed questions and a bounded action vocabulary: `OPEN_APP`, `DICTATE_TEXT`, and `NO_ACTION`.
- Separate action selection from app selection: first determine intent; only an app-opening intent needs a second question selecting from installed candidates plus a no-match option.
- Supply only the transcript, explicit mode, and necessary candidate app labels/IDs. Screen contents, documents, and clipboard contents are not needed.
- Represent the internal decision as an action, optional installed-app ID, probability/confidence information supplied by the API, and execution status. This is an application contract, not an assertion about JEV’s wire format.
- Validate all returned choices and IDs before execution. A valid schema does not establish that the chosen action is correct.
- Use configurable, experimentally validated confidence thresholds. Low confidence, close competing choices, missing candidates, or contradictory results lead to clarification or no action.
- Never execute shell commands, arbitrary code, paths, URLs, or generated instructions returned by a model.
- Treat transcript contents as task data; spoken attempts to change the supported action set cannot expand app capabilities.

### Application opening

- Discover launchable apps through a platform adapter: standard system/user Applications locations on macOS; supported Start menu entries and packaged-app registrations on Windows. Support an explicit refresh. Define the supported Windows app types in the feasibility decision record; arbitrary portable executables are excluded from initial discovery.
- Maintain app display names, stable identifiers, and optional user aliases. Resolve duplicate display names through a visible picker.
- Bound the candidate list to the verified API limit. If deterministic name/alias matching cannot produce a useful shortlist, ask the user to choose or refine the name.
- Open or activate the resolved installed application using the appropriate native application-launch APIs on each platform. Do not install apps or navigate inside them.
- Opening an app must work on both platforms. Bringing an existing window forward is best effort under each OS’s focus rules; report that distinction accurately.
- Record whether the launch request succeeded; do not claim a task inside the target app was completed.

### Text area and settings

- Provide a multiline editable text area with Copy and Clear controls and a visible destination label.
- Keep the draft in memory during the session. Warn before closing with a nonempty draft; do not persist dictated text across launches in the MVP.
- Expose STT mode, model readiness, model download/removal, credentials, connection test, shortcut, and app aliases in settings.
- Credentials are stored through an OS credential-store adapter (macOS Keychain or Windows Credential Manager), never in frontend storage, and never included in logs.
- Provide a masked JEV API key field with test, replace, and remove controls. Removing the key deletes it from the OS credential store and disables Command mode until a replacement is validated. Handle authentication, quota, and rate-limit failures with actionable feedback and bounded retries; do not automatically retry rejected credentials.
- Local models are downloaded only after user initiation; validate integrity before loading, reject incompatible files, and allow removal of app-managed downloads. Never delete a user-supplied model file.

## 7. Interaction and privacy requirements

- Use a macOS menu bar entry or Windows system tray entry plus a compact window containing mode selection, recording controls, transcript/result feedback, and the text area. Settings may use a separate window.
- Always show when the microphone is active and which STT mode is selected. Make errors and clarification choices keyboard accessible.
- Explain that local STT keeps audio on the device, while Command mode still sends transcript/context to JEV. Remote STT additionally sends audio to the selected transcription provider.
- Request only permissions needed for this scope. Cross-app text insertion is excluded; macOS Accessibility/Screen Recording permissions and Windows administrator privileges are not planned for the MVP. Validate microphone privacy settings and actual framework permissions on both platforms during feasibility.
- Keep audio in memory where supported; remove temporary audio after transcription, cancellation, or failure. Do not persist audio, transcripts, or text content in diagnostic logs.
- Retain only bounded, local operational diagnostics such as timings and error codes. Provide a clear-diagnostics control. No product analytics upload is required for MVP validation.
- Provider retention policies and data handling must be documented before release; do not promise zero retention without verification.

## 8. Proposed technical approach

Recommended stack: Tauri 2 with a Rust application core and a shared React/TypeScript interface. Use the latest stable dependencies and toolchains, selecting LTS releases where officially offered; exclude prerelease/nightly builds. Pin verified versions and commit lockfiles, and validate upgrades on macOS and Windows. This is a proposed architecture to validate during feasibility. Swift supports Windows as a language, but Apple’s SwiftUI does not provide a supported Windows UI; a SwiftUI-first implementation would require another Windows presentation layer.

- Shared frontend: onboarding, recording controls, mode selection, text area, clarification, settings, and status feedback.
- Shared Rust core: interaction lifecycle, STT orchestration, JEV HTTP integration, policy validation, model downloads, and diagnostics. Keep API credentials and authenticated provider requests in the core; the frontend receives only masked configuration state and results.
- Platform adapters: microphone capture/permissions, app discovery and launch/activation, global shortcuts, tray/menu integration, credential storage, and model storage locations. Use native bindings or small native helpers where required.
- Local STT: select an engine that can ship on both target platforms. Hardware acceleration may differ; validate CPU fallback and performance independently. Rust orchestrates inference but does not require the inference engine itself to be written in Rust.
- Keep model loading and inference off the UI thread; use a worker or separate process with cancellation so recording controls stay responsive.
- Package and test on each target OS. Tauri shares much of the application, while OS integrations, signing, and installers still require platform-specific work.

Confirm the local engine’s packaging and hardware requirements before fixing minimum supported OS versions.

```text
User starts recording → Audio capture → Selected STT adapter → Transcript
  Command mode → JEV intent → Optional JEV app choice → Policy validation
    → Open/activate installed app | Append dictated text | Clarify/no action
  Dictation mode → Append transcript literally
```

The executor accepts only validated internal actions. Associate work with a unique interaction ID; discard canceled, stale, and duplicate results before execution. A retry must not repeat an already completed launch or text insertion. Check cancellation immediately before execution; cancellation cannot undo an action that has already completed.

JEV is a classification dependency, not an STT engine or text generator. No alternate decision model is required for the MVP. If JEV fails, Command mode shows an error and executes nothing; independent local dictation can remain available.

## 9. Acceptance scenarios

| Scenario                                                             | Expected result                                                                           |
| -------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| “Open Safari” in Command mode, app installed and selection confident | Safari opens or activates once; status confirms the request                               |
| “Open [unknown app]”                                                 | No launch; explain no installed match                                                     |
| Two installed apps match the same spoken name                        | Show labeled choices; execute only the user’s selection                                   |
| “Delete my downloads”                                                | Unsupported; no action                                                                    |
| “Open Safari” spoken in Dictation mode                               | Text area receives those words; no app launch                                             |
| “Type: Remember to book the tickets” in Command mode                 | Append “Remember to book the tickets” after successful routing and marker validation      |
| Dictate a second passage                                             | Preserve the first passage and append a new paragraph                                     |
| Silence, noise, or an empty transcript                               | Show no-speech/retry feedback; no action                                                  |
| JEV timeout, invalid choice, or low confidence                       | Error or clarification; no execution                                                      |
| JEV API key is missing, invalid, or removed                          | Command mode is unavailable with setup guidance; configured Dictation mode remains usable |
| JEV reports exhausted quota or a rate limit                          | Explain the provider error; no action executes and no unbounded retries occur             |
| Remote STT fails while local mode is available                       | Explain failure and offer an explicit mode switch                                         |
| User cancels while a request is pending                              | Stop capture/work where possible; ignore late results                                     |
| Shortcut is pressed repeatedly during processing                     | One interaction completes without duplicate text or launches                              |
| Microphone access is denied                                          | Explain how to enable it and allow retry                                                  |
| Local model download is interrupted                                  | Model remains unavailable; retry succeeds without loading a partial file                  |

## 10. Delivery milestones

1. **Feasibility:** Verify JEV access, authentication, actual choice/confidence semantics, candidate limits, and data policies. Select and test one local STT engine and one remote STT provider. Confirm model licensing, download integrity, minimum OS versions, hardware, distribution requirements, and measured latency on both platforms. Validate the proposed Tauri/Rust stack with microphone capture, credential storage, app discovery/launch, and shortcuts on a Mac and a Windows PC. Deliver a short implementation decision record and working speech-to-decision spike.
2. **Core vertical slice:** Capture voice, transcribe through one working adapter, route with JEV, and launch an installed app. Include validation, cancellation, and visible failures.
3. **Complete Phase 1:** Add the second STT adapter, local model setup, text area, explicit dictation behavior, settings, and ambiguity handling.
4. **Release candidate:** Run the fixed evaluation suites in both STT modes, test clean-machine setup and recovery paths, and deliver a signed/notarized macOS build and a signed Windows installer for direct-download testing. Verify installation, launch, microphone access, model setup, credentials, and uninstall behavior on a clean machine for each platform. Resolve release-blocking failures before calling the MVP complete.

## 11. Risks and unresolved decisions

| Risk / decision                                     | Resolution before release                                                                                                                   |
| --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| JEV access or API behavior differs from assumptions | Validate in milestone 1; do not substitute another model without a product decision                                                         |
| Local model is too slow or too large                | Benchmark on both reference machines, show requirements before download, and choose the supported configuration based on results            |
| Remote STT provider remains unspecified             | Select one after validating streaming/batch behavior, privacy, credential handling, and cost; streaming partial transcripts are required    |
| Names and accents cause incorrect routing           | Evaluate realistic utterances, tune aliases and thresholds, and require clarification when uncertain                                        |
| API expense or network latency hurts usability      | Measure per-interaction usage and latency; avoid continuous inference and cap request timeouts/retries                                      |
| Command/dictation confusion                         | Keep mode visible and default the text capture flow to explicit Dictation mode                                                              |
| Distribution affects model loading or shortcuts     | Validate platform-specific signing/installers, macOS notarization, model storage, and shortcut behavior on clean macOS and Windows machines |

## 12. Explicitly out of scope

Cross-app typing or pasting, UI clicking/navigation, screenshots or screen understanding, autonomous multi-step workflows, browser automation, file operations, terminal commands, sending messages, purchases, wake words, always-on listening, multilingual support, cloud sync, accounts, subscriptions, Linux support, mobile apps, and a plugin marketplace.

Possible next phase: focused-field dictation in other apps, with destination checks and the necessary platform-specific permissions, after Phase 1 meets its reliability targets.

## 13. Source notes

- [TypeSafe: Introducing System One Models & JEV](https://typesafe.ai/blog/introducing-system-one-models-and-jev), published September 15, 2026. Describes JEV as a model for structured decisions rather than free-form text generation. Vendor performance claims are not MVP guarantees.
- [TypeSafe documentation: Introduction](https://docs.typesafe.ai/introduction), reviewed October 18, 2026. Describes typed questions, structured results, probability information, and decomposing decisions into questions combined in application code.

All UX choices, architecture proposals, thresholds, and release targets in this document are product proposals rather than capabilities verified by these sources. Exact API integration and provider choices remain feasibility work.

- [Swift platform support](https://www.swift.org/platform-support/) and [Apple SwiftUI overview](https://developer.apple.com/swiftui/): distinguish Swift language availability on Windows from SwiftUI’s Apple-platform scope.
- [Tauri overview](https://v2.tauri.app/start/) and [process model](https://v2.tauri.app/concept/process-model/): describe its Rust core and platform webviews. The stack recommendation above is an engineering judgment based on this app’s requirements.
