import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const certificateName = /^(Apple Development|Developer ID Application): /;

export function parseIdentities(output) {
  return [...output.matchAll(/^\s*\d+\) ([A-Fa-f0-9]{40}) "([^"\r\n]+)"\s*$/gm)]
    .map(([, fingerprint, name]) => ({ fingerprint, name }))
    .filter(({ name }) => certificateName.test(name));
}

export function selectIdentity(identities, requested, release = false) {
  if (requested === "-")
    throw new Error("Ad-hoc signing cannot be used for a stable macOS build.");
  const candidates = identities
    .filter(({ name, fingerprint }) =>
      requested
        ? name === requested ||
          fingerprint.toLowerCase() === requested.toLowerCase()
        : true,
    )
    .filter(
      ({ name }) => !release || name.startsWith("Developer ID Application: "),
    );
  if (candidates.length === 0) {
    throw new Error(
      "No matching valid Apple signing identity with a private key is available. Install an Apple Development certificate for local testing or a Developer ID Application certificate for releases in your login Keychain. Then run npm run signing:check. See MACOS-SIGNING.md.",
    );
  }
  if (candidates.length !== 1)
    throw new Error(
      "Multiple Apple signing identities are available. Set APPLE_SIGNING_IDENTITY to the exact certificate name or SHA-1 fingerprint.",
    );
  return candidates[0];
}

export function validateNotarization(env) {
  for (const name of ["APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"]) {
    if (!env[name]?.trim())
      throw new Error(
        `Signed releases require ${name}; see MACOS-SIGNING.md. No installer was built.`,
      );
  }
}

export function validateSignature(
  details,
  requirement,
  identifier,
  release = false,
) {
  if (
    /^Signature=adhoc$/m.test(details) ||
    !/^Authority=(Apple Development|Developer ID Application): /m.test(details)
  ) {
    throw new Error(
      "The app is not signed with an Apple development or Developer ID certificate.",
    );
  }
  if (!details.split(/\r?\n/).includes(`Identifier=${identifier}`))
    throw new Error("The signed app has an unexpected bundle identifier.");
  if (!/^TeamIdentifier=[A-Z0-9]{10}$/m.test(details))
    throw new Error("The signed app has no valid Apple team identity.");
  if (
    !/^designated => /m.test(requirement) ||
    /^designated => cdhash /m.test(requirement)
  )
    throw new Error(
      "The app has no stable certificate-based designated requirement.",
    );
  if (release && !/^Authority=Developer ID Application: /m.test(details))
    throw new Error(
      "Public macOS releases require Developer ID Application signing.",
    );
}

function capture(command, args) {
  try {
    return execFileSync(command, args, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch {
    throw new Error(
      `${command} failed. Check the signing certificate and Keychain access.`,
    );
  }
}

function verify(app, release) {
  execFileSync(
    "/usr/bin/codesign",
    ["--verify", "--deep", "--strict", "-R", "=anchor apple generic", app],
    {
      stdio: "inherit",
    },
  );
  const inspect = (args) => {
    const result = spawnSync("/usr/bin/codesign", args, { encoding: "utf8" });
    if (result.status !== 0)
      throw new Error("Could not inspect the app signature.");
    return `${result.stdout}\n${result.stderr}`;
  };
  const identifier = JSON.parse(
    readFileSync(resolve(root, "src-tauri/tauri.conf.json")),
  ).identifier;
  validateSignature(
    inspect(["--display", "--verbose=4", app]),
    inspect(["--display", "-r-", app]),
    identifier,
    release,
  );
  if (release) {
    execFileSync("/usr/bin/xcrun", ["stapler", "validate", app], {
      stdio: "inherit",
    });
    if (
      process.env.APPLE_TEAM_ID &&
      !inspect(["--display", "--verbose=4", app])
        .split(/\r?\n/)
        .includes(`TeamIdentifier=${process.env.APPLE_TEAM_ID}`)
    )
      throw new Error(
        "The app signing team does not match the notarization team.",
      );
    execFileSync(
      "/usr/sbin/spctl",
      ["--assess", "--type", "execute", "--verbose=2", app],
      { stdio: "inherit" },
    );
  }
  console.log("Verified Apple certificate signature and stable app identity.");
}

export function main(args) {
  if (process.platform !== "darwin")
    throw new Error("Apple signing must run on macOS.");
  const [command, ...options] = args;
  if (!["check", "build", "verify"].includes(command))
    throw new Error(
      "Usage: macos-signing.mjs check | build [--release] | verify <app> [--release]",
    );
  const release = options.includes("--release");
  if (command === "verify") {
    const paths = options.filter((arg) => arg !== "--release");
    if (paths.length !== 1) throw new Error("Provide the .app path to verify.");
    verify(resolve(paths[0]), release);
    return;
  }
  if (options.some((arg) => arg !== "--release"))
    throw new Error("Only --release is supported.");
  const identity = selectIdentity(
    parseIdentities(
      capture("/usr/bin/security", [
        "find-identity",
        "-v",
        "-p",
        "codesigning",
      ]),
    ),
    process.env.APPLE_SIGNING_IDENTITY,
    release,
  );
  console.log(`Using ${identity.name}`);
  if (release) validateNotarization(process.env);
  if (command === "check") return;
  const config = JSON.stringify({
    bundle: { macOS: { signingIdentity: identity.fingerprint } },
  });
  execFileSync(
    process.execPath,
    [
      resolve(root, "node_modules/@tauri-apps/cli/tauri.js"),
      "build",
      "--config",
      config,
      "--bundles",
      release ? "app,dmg" : "app",
      "--",
      "--locked",
    ],
    {
      cwd: root,
      stdio: "inherit",
      env: { ...process.env, APPLE_SIGNING_IDENTITY: identity.fingerprint },
    },
  );
  verify(
    resolve(root, "src-tauri/target/release/bundle/macos/Decider Desktop.app"),
    release,
  );
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    // Child process errors may contain sensitive command arguments; never print them.
    console.error(
      error.status !== undefined
        ? "Apple signing/build verification failed. No release should be published."
        : error.message,
    );
    process.exitCode = 1;
  }
}
