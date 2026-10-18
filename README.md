# Decider Desktop

A voice-first desktop assistant for macOS and Windows. Speech-to-text supplies the words; TypeSafe JEV selects from a bounded set of desktop actions using the user's own API key.

## Current state

This repository contains the **initial application foundation**, not the completed MVP.

Implemented:

- Tauri 2 desktop shell with a shared React/TypeScript interface and Rust core.
- Workspace with Dictation and Command views, editable session-only text, copy, and confirmed clear.
- Settings to save, replace, and remove a user-supplied JEV key through macOS Keychain or Windows Credential Manager. The saved key is never returned to the interface or persisted in frontend storage. It passes through the interface only while being entered and saved.
- Native runtime/version status, restrictive content security policy, and no general-purpose shell or URL opener permissions.
- macOS and Windows CI checks for frontend builds, Rust tests, formatting, linting, and desktop compilation.

Not implemented yet: microphone capture, local model downloads/inference, remote STT, JEV connection validation/requests, app discovery/launch, global shortcuts, tray/menu integration, and native unsaved-draft close protection. Recording is disabled and saved keys are explicitly marked unvalidated. Nothing records audio or sends transcripts in this starter.

## Development

Install Node.js 24 LTS or newer supported by Vite and the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/), including stable Rust. Windows development also requires the Microsoft C++ build tools and WebView2; macOS requires Xcode command line tools.

```sh
npm ci
npm run tauri dev
```

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

Native builds must be validated on each target OS. Release signing, notarization, and installer distribution are not configured yet. CI builds an executable for validation and does not publish releases.

## Architecture

- `src/`: shared interface; no persistent draft or credential storage.
- `src-tauri/src/`: Rust entry point and narrow native commands for setup status and credentials.
- `src-tauri/capabilities/`: desktop permission boundary.
- `MVP-PRD.md`: product scope, acceptance criteria, and delivery milestones.

Next: validate the STT engine on both targets and the JEV typed-choice API, then implement the first speech-to-action flow. See [the MVP PRD](MVP-PRD.md).

## Data handling

Draft text exists in memory until the app closes. Copy it before exiting; the current browser unload warning is not a guaranteed native-window close guard. Credentials are stored under the `ai.nex2i.decider` service with a `jev-api-key` account and can be removed in Settings. Credentials may survive uninstall until explicitly removed. Never commit credentials or private audio/transcripts.
