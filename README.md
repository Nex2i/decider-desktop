# Decider Desktop

A voice-first desktop assistant for macOS and Windows. Speech-to-text supplies the words; TypeSafe JEV selects from a bounded set of desktop actions using the user's own API key.

## Current state

This repository contains the desktop foundation and a working **local streaming transcription test**, not the completed desktop-action MVP.

Implemented:

- Tauri 2 desktop shell with a shared React/TypeScript interface and Rust core.
- Workspace with a downloadable model picker, live partial transcripts, Stop/Cancel controls, editable session-only text, copy, and confirmed clear.
- Streaming-only model picker: Zipformer English Compact (about 73 MB), Zipformer Full precision (about 266 MB), Nemotron 3.5 ASR Streaming 0.6B (about 683 MB), and Qwen3-ASR Streaming 0.6B (about 1.88 GB). No JEV key is required.
- Zipformer/Nemotron run through sherpa-onnx on CPU. Qwen uses the native `qwen3-asr` streaming API with Metal on supported Macs and CPU on Windows. Qwen warms up before microphone capture and updates in roughly two-second audio chunks; its larger memory/compute requirements can make it unsuitable for slower machines. No after-stop/batch transcription fallback is included.
- Downloads use pinned model revisions, exact sizes, and SHA-256 verification. Interrupted downloads cannot be loaded; verified files can be reused when retrying. Models live in the OS local application-data directory under `ai.nex2i.decider/models` and can be removed in the picker.
- Settings to save, replace, and remove a user-supplied JEV key through macOS Keychain or Windows Credential Manager. Saving verifies a fresh read of the credential. The saved key is never returned to the interface or persisted in frontend storage. It passes through the interface only while being entered and saved.
- **Test connection** validates the saved key against TypeSafe's authenticated model list and confirms JEV model access. It sends no speech or draft text and makes no inference call. Verification lasts for the current session; replacing/removing the key or refreshing setup clears it. Errors distinguish rejected keys, permissions, billing, rate limits, and connection failures.
- **Desktop actions** discovers running apps, installed-app entries, and OS-exposed control capabilities on macOS and Windows. It lists native accessibility actions/writable attributes on macOS and standard UI Automation patterns on Windows. Results are searchable, paged, and explicitly marked when partial. Labels are opt-in, field contents/password controls are excluded, and no actions execute or snapshots go to JEV. The view also includes a 116-type capability catalog; see [Desktop action catalog and discovery](DESKTOP-ACTIONS.md).
- Native runtime/version status, restrictive content security policy, and no general-purpose shell or URL opener permissions.
- Automatic macOS and Windows checks for frontend compilation, native tests, formatting, and linting. Installer packaging and GitHub release publishing run only through the manual New version workflow.

Not implemented yet: remote STT, global shortcuts, tray/menu integration, external text insertion, arbitrary keyboard/mouse actions, multi-step workflows, and native unsaved-draft close protection. Saved keys remain marked unvalidated until Test connection succeeds. Independent Dictation mode does not invoke JEV.

## Development

Use Node.js **24.22.0 LTS** (pinned in `.nvmrc`) and Rust **1.99.1 stable** (pinned in `rust-toolchain.toml`). Install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). Windows development also requires the Microsoft C++ build tools and WebView2; macOS requires Xcode command line tools.

If you use nvm, run `nvm install` and `nvm use` in this directory first. npm rejects unsupported Node versions to keep local development aligned with CI.

```sh
npm ci
npm run desktop:dev
```

## Test your speech

1. Run the **desktop app** with `npm run tauri dev`.
2. Open **Workspace → Dictation**, choose a model, and click **Download model**. Internet access is needed only for this model setup.
3. Click **Start recording** and allow microphone access if asked. The app uses your default system input device.
4. Speak and watch the live transcript. **Stop & keep text** appends the finalized result to Your text. **Cancel** or Escape discards the current recording and preserves existing text. Recording stops at 60 seconds.
5. Use the picker to compare profiles or remove a download. Switching views cancels an active recording/download.

Every model produces partial text while recording; Stop only flushes the last chunk and keeps the transcript. Audio is processed in memory on your device and is not saved or uploaded. No JEV key or internet connection is required after the model is installed. macOS and Windows microphone privacy settings must allow the app to use your input device. The browser preview cannot download or run the native speech engine.

The model catalog is in `src-tauri/models.json`. It pins the published revisions and hashes for [Zipformer](https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-26) (Apache-2.0), [Nemotron](https://huggingface.co/csukuangfj2/sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11) (OpenMDW-1.1), and [Qwen3-ASR](https://huggingface.co/Qwen/Qwen3-ASR-0.6B) (Apache-2.0). The native runtime reads only these verified local files; its automatic model-hub downloader is disabled. The Qwen tokenizer reconstruction code preserves its upstream MIT notice.

## Spoken desktop commands

In **Workspace → Command**, use a downloaded speech model and your saved JEV key. Speak one action, then choose **Stop & run command**. Only the final transcript is classified; streaming partials never execute. You can also type a command and choose **Run command**.

- **Open Safari**: JEV selects a known app; native code opens its inventoried bundle or activates its running process. On Windows, new launches support Start menu shortcuts to local `.exe` targets without arguments; other shortcuts must be opened manually.
- **Type remember the milk** or **Dictate: remember the milk**: JEV selects draft dictation, then a deterministic parser appends the literal text to Decider's draft. It does not write into another app.
- **Click Reload in Google Chrome**: enable **Allow JEV to read control labels in the app I name**, name the app and one control, then review the proposed app/control/operation and choose **Run selected action** within 30 seconds. Only supported, enabled named controls become candidates. macOS supports Press, ShowMenu, Raise and Focus; Windows supports Invoke, Toggle, Select, Expand, Collapse and Focus. Some controls and hidden menus are absent; open the relevant window or menu first.

JEV receives the command and bounded app names. Opting into control commands additionally sends labels, roles, operations, and window/menu context for the selected app, which may reveal private document names. Audio, field values, passwords, clipboard contents, screenshots, local launch paths and the rest of the draft are not uploaded. The key stays in native credential storage. Command state and results stay in memory; there is no transcript/action history on disk.

The native adapter uses TypeSafe's [typed Choice API](https://docs.typesafe.ai/api), with an explicit no-action choice, at most 251 candidates per question (including no-action), probability at least 0.80, confidence at least 0.70, and a 0.15 margin. App candidates are ranked by spoken name overlap before shortlisting; JEV still makes the selection. Partial control scans are reported. An absent/ambiguous choice, provider failure, invalid response, low confidence, cancellation, or expired approval executes nothing. No automatic HTTP retries occur. The provider returns only known IDs, never executable paths, scripts, action arguments, or generated text. Named native control references stay on one worker thread through classification/review and are rechecked before dispatch. A successful dispatch reports the OS/app accepted the request; it does not prove the resulting workflow completed.

App opening and in-app dictation run directly. Other control operations require explicit review because a button/menu command can have consequences. This is a single-action controller, not an autonomous workflow engine; the broader capability catalog remains a roadmap. Switching away cancels a pending command, but an OS action already dispatched cannot be undone by cancellation. The read-only Desktop actions inspector remains independent.

## Browser preview

For a browser-only UI preview:

```sh
npm run dev
```

The preview cannot access native credentials. No API key is needed to run the initial UI. In the desktop app, open Settings to enter your own JEV key. Saving confirms secure storage; **Test connection** checks provider access using [TypeSafe's documented model-list endpoint](https://docs.typesafe.ai/models#listing-models). A successful check does not enable desktop commands yet.

```sh
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --locked -- -D warnings
npm run tauri build -- --no-bundle
```

Native builds must be validated on each target OS. sherpa-onnx downloads its platform-specific static native libraries during the first Rust build. Windows builds use the static CRT to match those libraries. For macOS testing that uses Accessibility or Keychain, use `npm run signing:check` and `npm run build:macos`. These require a valid Apple signing certificate and refuse ad-hoc fallback, then verify the app signature and stable identity. Use `npm run desktop:dev` for a separate debug app identity. The low-level Tauri command still supports ad-hoc builds, whose privacy grants may be invalidated on rebuild. See [macOS signing setup](MACOS-SIGNING.md) for Xcode certificate setup, local builds, and GitHub secrets.

## Manual versions and releases

Pushes and pull requests run checks only; they do not package installers, upload build artifacts, create tags, or publish releases.

When you explicitly want a new version:

```sh
npm run version:set -- 0.2.0
npm run check:versions
```

Commit and push the version files to `main`, then choose **Actions → New version (manual) → Run workflow**, enter the committed version (for example `0.2.0`), and run it on `main`. This is the only release trigger; tags and ordinary pushes do not publish anything. The workflow validates consistent versions, refuses existing version tags/releases, builds both platforms, and publishes a GitHub release only after both packages succeed. It pins both packages to the same source commit.

The release contains a macOS Apple Silicon DMG and a Windows x64 NSIS installer. macOS releases require Developer ID Application signing and notarization; missing signing secrets fail the workflow before packaging. Windows installers remain unsigned. See [macOS signing setup](MACOS-SIGNING.md). Models download separately from the application. Signing setup does not dispatch a release.

## Dependency policy

Use the latest stable releases, choosing the latest LTS line where the upstream project offers one. Avoid prerelease, beta, canary, and nightly builds. As of October 18, 2026, Node 24 is the latest LTS line; Node 26 has not entered LTS. React, TypeScript, Tauri, and Rust use current stable releases rather than an LTS channel for this project.

Direct dependencies and toolchains are pinned, and npm/Cargo lockfiles are committed. Upgrade them deliberately together, review migration requirements, and run the macOS and Windows checks before accepting an update. Keep `@types/node` on the selected Node LTS major.

## Architecture

- `src/`: shared interface; no persistent draft or credential storage.
- `src-tauri/src/`: Rust entry point and narrow native commands for setup status and credentials.
- `src-tauri/capabilities/`: desktop permission boundary.
- `MVP-PRD.md`: product scope, acceptance criteria, and delivery milestones.

Next: benchmark the speech profiles on both target platforms, validate the JEV typed-choice API, then implement the first speech-to-action flow. See [the MVP PRD](MVP-PRD.md).

## Data handling

Draft text exists in memory until the app closes. Copy it before exiting; the current browser unload warning is not a guaranteed native-window close guard. Credentials are stored under the `ai.nex2i.decider` service with a `jev-api-key` account and can be removed in Settings. Credentials may survive uninstall until explicitly removed. Never commit credentials or private audio/transcripts.
