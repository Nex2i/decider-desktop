import { readFileSync, writeFileSync } from "node:fs";
import assert from "node:assert/strict";

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const packagePath = "package.json";
const lockPath = "package-lock.json";
const configPath = "src-tauri/tauri.conf.json";
const cargoPath = "src-tauri/Cargo.toml";
const cargoLockPath = "src-tauri/Cargo.lock";
const pkg = readJson(packagePath);
const lock = readJson(lockPath);
const config = readJson(configPath);
let cargo = readFileSync(cargoPath, "utf8");
let cargoLock = readFileSync(cargoLockPath, "utf8");
const packageSection =
  /\[\[package\]\]\nname = "decider-desktop"\nversion = "([^"]+)"/;
const cargoVersion = /^version = "([^"]+)"/m;
const version = process.argv[2];
if (version && version !== "--check") {
  assert.match(
    version,
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/,
    "Use a stable version such as 0.2.0.",
  );
  assert(
    cargoVersion.test(cargo) && packageSection.test(cargoLock),
    "Could not find Rust package version.",
  );
  pkg.version =
    lock.version =
    lock.packages[""].version =
    config.version =
      version;
  cargo = cargo.replace(cargoVersion, `version = "${version}"`);
  cargoLock = cargoLock.replace(
    packageSection,
    `[[package]]\nname = "decider-desktop"\nversion = "${version}"`,
  );
  for (const [path, value] of [
    [packagePath, pkg],
    [lockPath, lock],
    [configPath, config],
  ])
    writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
  writeFileSync(cargoPath, cargo);
  writeFileSync(cargoLockPath, cargoLock);
}
assert.match(pkg.version, /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/);
assert.equal(lock.version, pkg.version);
assert.equal(lock.packages[""].version, pkg.version);
assert.equal(config.version, pkg.version);
assert.equal(cargo.match(cargoVersion)?.[1], pkg.version);
assert.equal(cargoLock.match(packageSection)?.[1], pkg.version);
console.log(`Version ${pkg.version}: all manifests agree.`);
