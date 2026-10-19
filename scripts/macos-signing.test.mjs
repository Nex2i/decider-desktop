import { test } from "node:test";
import assert from "node:assert/strict";
import {
  parseIdentities,
  selectIdentity,
  validateNotarization,
  validateSignature,
} from "./macos-signing.mjs";

const development = {
  fingerprint: "A".repeat(40),
  name: "Apple Development: Test Developer (1234567890)",
};
const distribution = {
  fingerprint: "B".repeat(40),
  name: "Developer ID Application: Test Company (ABCDEFGHIJ)",
};

test("only valid Apple application signing identities are accepted", () => {
  const output = `  1) ${development.fingerprint} "${development.name}"\n  2) ${distribution.fingerprint} "${distribution.name}"\n  3) ${"C".repeat(40)} "Developer ID Installer: Test"\n  4) ${"D".repeat(40)} "Apple Development: Expired" (CSSMERR_TP_CERT_EXPIRED)\n  4 valid identities found\n`;
  assert.deepEqual(parseIdentities(output), [development, distribution]);
  assert.throws(() => selectIdentity([], undefined), /No matching valid/);
  assert.throws(() => selectIdentity([development], "-"), /Ad-hoc/);
  assert.throws(() => selectIdentity([development, distribution]), /Multiple/);
  assert.equal(
    selectIdentity([development, distribution], development.name),
    development,
  );
  assert.equal(
    selectIdentity(
      [development, distribution],
      distribution.fingerprint.toLowerCase(),
      true,
    ),
    distribution,
  );
  assert.throws(
    () => selectIdentity([development], development.name, true),
    /No matching valid/,
  );
});

test("release preflight requires all notarization credentials without disclosing them", () => {
  const env = {
    APPLE_ID: "test@example.invalid",
    APPLE_PASSWORD: "secret-test-value",
    APPLE_TEAM_ID: "ABCDEFGHIJ",
  };
  assert.doesNotThrow(() => validateNotarization(env));
  for (const name of Object.keys(env)) {
    assert.throws(
      () => validateNotarization({ ...env, [name]: "" }),
      (error) =>
        error.message.includes(name) &&
        !error.message.includes(env.APPLE_PASSWORD),
    );
  }
});

const details = `Identifier=ai.nex2i.decider\nAuthority=${distribution.name}\nTeamIdentifier=ABCDEFGHIJ\n`;
const requirement =
  'designated => identifier "ai.nex2i.decider" and anchor apple generic and certificate leaf[subject.OU] = ABCDEFGHIJ';
test("verification rejects ad-hoc, wrong bundle, missing team and development releases", () => {
  assert.doesNotThrow(() =>
    validateSignature(details, requirement, "ai.nex2i.decider", true),
  );
  for (const [badDetails, badRequirement] of [
    [details + "Signature=adhoc\n", requirement],
    [details.replace("ai.nex2i.decider", "ai.nex2i.decider.dev"), requirement],
    [
      details.replace("TeamIdentifier=ABCDEFGHIJ", "TeamIdentifier=not set"),
      requirement,
    ],
    [details, 'designated => cdhash H"1234"'],
    [details, ""],
    [details.replace(distribution.name, development.name), requirement],
  ])
    assert.throws(() =>
      validateSignature(badDetails, badRequirement, "ai.nex2i.decider", true),
    );
});
