import { test } from "node:test";
import assert from "node:assert/strict";
import { verifyDenyList, DenyListError } from "../src/safety.js";

test("empty requiredDeny always passes", () => {
  assert.doesNotThrow(() => verifyDenyList([], null));
  assert.doesNotThrow(() => verifyDenyList([], { permissions: { deny: [] } }));
});

test("all required patterns present passes", () => {
  assert.doesNotThrow(() =>
    verifyDenyList(["rm -rf /", "shutdown"], {
      permissions: { deny: ["rm -rf /", "shutdown", "reboot"] },
    }),
  );
});

test("a missing pattern throws DenyListError (fail-closed)", () => {
  assert.throws(
    () =>
      verifyDenyList(["rm -rf /", "shutdown"], {
        permissions: { deny: ["rm -rf /"] },
      }),
    DenyListError,
  );
});

test("null cli-config with a non-empty requirement throws", () => {
  assert.throws(() => verifyDenyList(["rm -rf /"], null), DenyListError);
});
