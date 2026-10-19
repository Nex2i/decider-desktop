# Stable macOS signing

Accessibility grants and Keychain approvals belong to a signed app identity, not just its display name. Ad-hoc signing binds that identity to a particular compiled version. Rebuilding it can invalidate earlier permission grants, even while an existing Settings entry looks enabled. Use the same certificate and bundle identifier across builds; do not alternate development and distribution identities for a permission-sensitive installation.

The stable bundle identifier is `ai.nex2i.decider`. `npm run desktop:dev` uses `ai.nex2i.decider.dev` and the name **Decider Desktop Dev**, so a debug entry cannot be mistaken for the release app. The browser preview remains `npm run dev`.

## Local testing: Apple Development certificate

1. Install and open the full Xcode app. Command line tools alone cannot manage an Apple account's certificates.
2. In **Xcode → Settings → Accounts**, add your Apple Account and select your team. A personal development team can be used for local development; public distribution requires the Apple Developer Program.
3. Open **Manage Certificates**, choose **+ → Apple Development**. Xcode creates the certificate and its private key in your login Keychain.
4. In this project, run:

   ```sh
   npm run signing:check
   npm run build:macos
   ```

   If more than one certificate is available, choose the exact name printed by `security find-identity -v -p codesigning`:

   ```sh
   export APPLE_SIGNING_IDENTITY='Apple Development: Your Name (YOURTEAMID)'
   npm run build:macos
   ```

The build command checks for a valid identity with its private key **before** compiling, signs the full `.app`, and verifies the signature, bundle identifier, Apple team, and certificate-based designated requirement afterward. It refuses ad-hoc fallback. The application is at `src-tauri/target/release/bundle/macos/Decider Desktop.app`.

Close previous Decider copies before opening this app. After switching from an ad-hoc identity, remove the old **Decider Desktop** permission entry and add this exact `.app` in **System Settings → Privacy & Security → Device Control and Data Access** (called **Accessibility** on earlier macOS versions). If macOS requires authentication, complete it yourself. Reopen the app and scan again. This first identity transition may also prompt for Keychain approval. Keep using this same identity afterward. Signing preserves identity across rebuilds; it does not itself grant Accessibility permission or guarantee that every app exposes its controls.

Do not create self-signed certificates, weaken signature requirements, change system privacy databases, or disable macOS protections as a substitute for Apple signing.

## Public installers: Developer ID Application

For direct distribution, use a **Developer ID Application** certificate (not Developer ID Installer or Apple Distribution) and notarization. This requires Apple Developer Program membership. The account holder can create the certificate through Apple's Certificates, Identifiers & Profiles portal; Xcode can manage eligible certificates. A downloaded `.cer` must be paired with the private key that generated its certificate request. Importing a `.cer` alone does not supply that key.

For local signed distribution builds, install that identity in your login Keychain and set:

```sh
export APPLE_SIGNING_IDENTITY='Developer ID Application: Your Name (YOURTEAMID)'
export APPLE_ID='your-apple-account-email'
export APPLE_TEAM_ID='YOURTEAMID'
# Set APPLE_PASSWORD securely in your local environment to an Apple app-specific password.
# Do not use your Apple Account login password, commit it, or paste it into chat.
npm run build:macos -- --release
```

The release command requires Developer ID Application signing and notarization credentials, then validates the app's stapled notarization ticket and Gatekeeper assessment. Missing credentials or invalid signatures fail the build. Normal local testing with Apple Development does not require these notarization credentials.

## Manual GitHub release configuration

In **Nex2i/decider-desktop → Settings → Secrets and variables → Actions**, add these repository secrets using GitHub's secure interface:

| Secret                       | Value                                                                                         |
| ---------------------------- | --------------------------------------------------------------------------------------------- |
| `APPLE_CERTIFICATE`          | Base64-encoded `.p12` containing the Developer ID Application certificate **and private key** |
| `APPLE_CERTIFICATE_PASSWORD` | Password used to encrypt that exported `.p12`                                                 |
| `APPLE_SIGNING_IDENTITY`     | Exact Developer ID Application identity name or SHA-1 fingerprint                             |
| `APPLE_ID`                   | Apple Account email used for notarization                                                     |
| `APPLE_PASSWORD`             | Apple **app-specific password**, not the login password                                       |
| `APPLE_TEAM_ID`              | Ten-character Apple Developer team identifier                                                 |

Export the certificate and private key as a password-protected `.p12` from **Keychain Access → login → My Certificates**. Encode it into a temporary private file with `openssl base64 -A -in /path/to/certificate.p12 -out /path/to/certificate-base64.txt`, copy its contents into the GitHub secret, then securely handle/remove your temporary export files. Never put certificates, private keys, passwords, or base64 contents in the repository, build artifacts, issues, or chat. `.p12`, `.pfx`, `.p8`, `.pem`, and `.key` files are ignored by this repository, but that is only a guard against accidents.

The existing **New version (manual)** workflow now checks signing configuration before packaging. Only the macOS job imports the certificate into a temporary runner keychain; it removes the keychain and export file even on failure. Signing secrets are not used in push/PR checks. The macOS package must pass signature and notarization verification before either platform is published. Windows signing is separate and remains unconfigured.

Releases remain manual. Adding this configuration does not dispatch a release or bump a version. Keep local builds on the same Developer ID identity as downloaded releases if you want them to share the same privacy identity.

## References

- [Tauri macOS signing and notarization](https://v2.tauri.app/distribute/sign/macos/)
- [Apple: code signing requirements and stable identity](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements)
- [Apple: managing signing certificates in Xcode](https://developer.apple.com/documentation/xcode/managing-signing-certificates)
- [Apple: creating Developer ID certificates](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/)
- [Apple: app-specific passwords](https://support.apple.com/102654)
