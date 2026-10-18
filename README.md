# Decider Desktop

A voice-first desktop assistant for macOS and Windows. Speech-to-text supplies the words; TypeSafe JEV selects from a bounded set of desktop actions using the user's own API key.

## Current state

This repository contains the desktop foundation and a working **local streaming transcription test**, not the completed desktop-action MVP.

Implemented:

- Tauri 2 desktop shell with a shared React/TypeScript interface and Rust core.
- Workspace with a downloadable model picker, live partial transcripts, Stop/Cancel controls, editable session-only text, copy, and confirmed clear.
- Two English Zipformer profiles: Compact (int8, about 73 MB) and Full precision (about 266 MB), using sherpa-onnx on CPU. No JEV key is required for transcription.
- Downloads use pinned model revisions, exact sizes, and SHA-256 verification. Interrupted downloads cannot be loaded; verified files can be reused when retrying. Models live in the OS local application-data directory under `ai.nex2i.decider/models` and can be removed in the picker.
- Settings to save, replace, and remove a user-supplied JEV key through macOS Keychain or Windows Credential Manager. The saved key is never returned to the interface or persisted in frontend storage. It passes through the interface only while being entered and saved.
- Native runtime/version status, restrictive content security policy, and no general-purpose shell or URL opener permissions.
- Automatic macOS and Windows checks for frontend compilation, native tests, formatting, and linting. Installer packaging and GitHub release publishing run only through the manual New version workflow.

Not implemented yet: remote STT, JEV connection validation/requests, app discovery/launch, global shortcuts, tray/menu integration, and native unsaved-draft close protection. Saved keys are explicitly marked unvalidated. Command mode is a placeholder; local transcription does not invoke JEV.

## Development

Use Node.js **24.22.0 LTS** (pinned in `.nvmrc`) and Rust **1.99.1 stable** (pinned in `rust-toolchain.toml`). Install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). Windows development also requires the Microsoft C++ build tools and WebView2; macOS requires Xcode command line tools.

If you use nvm, run `nvm install` and `nvm use` in this directory first. npm rejects unsupported Node versions to keep local development aligned with CI.

```sh
npm ci
npm run tauri dev
```

## Test your speech

1. Run the **desktop app** with `npm run tauri dev`.
2. Open **Workspace → Dictation**, choose a model, and click **Download model**. Internet access is needed only for this model setup.
3. Click **Start recording** and allow microphone access if asked. The app uses your default system input device.
4. Speak and watch the live transcript. **Stop & keep text** appends the finalized result to Your text. **Cancel** or Escape discards the current recording and preserves existing text. Recording stops at 60 seconds.
5. Use the picker to compare profiles or remove a download. Switching views cancels an active recording/download.

Audio is processed in memory on your device and is not saved or uploaded. No JEV key or internet connection is required after the model is installed. macOS and Windows microphone privacy settings must allow the app to use your input device. The browser preview cannot download or run the native speech engine.

The model catalog is in `src-tauri/models.json`; it pins [the published Zipformer checkpoint](https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-26), licensed Apache-2.0. These are two precision profiles of the same model, not two model families. Qwen and Nemotron are not bundled yet.

## Browser preview

For a browser-only UI preview:

```sh
npm run dev
```

The preview cannot access native credentials. No API key is needed to run the initial UI. In the desktop app, open Settings to enter your own JEV key. Saving confirms secure storage only; it does not verify provider access or enable commands yet.

```sh
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --locked -- -D warnings
npm run tauri build -- --no-bundle
```

Native builds must be validated on each target OS. sherpa-onnx downloads its platform-specific static native libraries during the first Rust build. Windows builds use the static CRT to match those libraries. Release signing and macOS notarization are not configured yet.

## Manual versions and releases

Pushes and pull requests run checks only; they do not package installers, upload build artifacts, create tags, or publish releases.

When you explicitly want a new version:

```sh
npm run version:set -- 0.2.0
npm run check:versions
```

Commit and push the version files to `main`, then choose **Actions → New version (manual) → Run workflow**, enter the committed version (for example `0.2.0`), and run it on `main`. This is the only release trigger; tags and ordinary pushes do not publish anything. The workflow validates consistent versions, refuses existing version tags/releases, builds both platforms, and publishes a GitHub release only after both packages succeed. It pins both packages to the same source commit.

The release contains a macOS Apple Silicon DMG and a Windows x64 NSIS installer. They are development installers until signing/notarization is configured. Models download separately from the application. No release is dispatched by the changes that add this workflow.

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
