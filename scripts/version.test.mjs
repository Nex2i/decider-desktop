import { test } from "node:test";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  copyFileSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";

const fixtures = [
  "package.json",
  "package-lock.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
];
function withFixture(run) {
  const directory = mkdtempSync(join(tmpdir(), "decider-version-"));
  mkdirSync(join(directory, "src-tauri"));
  copyFileSync("scripts/version.mjs", join(directory, "version.mjs"));
  for (const file of fixtures) copyFileSync(file, join(directory, file));
  try {
    run(directory);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}
const execute = (directory, version) =>
  execFileSync(process.execPath, ["version.mjs", version], {
    cwd: directory,
    stdio: "pipe",
    encoding: "utf8",
  });

test("version update keeps npm, Cargo, and desktop manifests consistent", () =>
  withFixture((directory) => {
    execute(directory, "0.2.0");
    assert.match(execute(directory, "--check"), /Version 0.2.0/);
    assert.equal(
      JSON.parse(readFileSync(join(directory, "package.json"))).version,
      "0.2.0",
    );
    assert.match(
      readFileSync(join(directory, "src-tauri/Cargo.lock"), "utf8"),
      /name = "decider-desktop"\nversion = "0.2.0"/,
    );
  }));

test("invalid or prerelease version leaves all manifests untouched", () =>
  withFixture((directory) => {
    const before = fixtures.map((file) =>
      readFileSync(join(directory, file), "utf8"),
    );
    for (const version of [
      "0.2.0-beta.1",
      "v0.2.0",
      "1.2.3; echo bad",
      "01.2.3",
    ])
      assert.throws(() => execute(directory, version));
    assert.deepEqual(
      fixtures.map((file) => readFileSync(join(directory, file), "utf8")),
      before,
    );
  }));
