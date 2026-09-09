import assert from "node:assert/strict";
import test from "node:test";
import { assertToolchain } from "./build.mjs";

const qualified = { node: "26.4.0", zlib: "1.3.2.1-motley-3246f1b", brotli: "1.2.0" };
const npm = "npm/11.17.0 node/v26.4.0";

test("qualified runtime and direct Node invocations retain their contract", () => {
  assert.doesNotThrow(() => assertToolchain(qualified, npm));
  assert.doesNotThrow(() => assertToolchain(qualified, ""));
});

test("Node and npm version mismatches fail before asset generation", () => {
  assert.throws(() => assertToolchain({ ...qualified, node: "26.3.0" }, npm), /Node 26\.4\.0 is required/);
  assert.throws(() => assertToolchain(qualified, "npm/11.16.0 node/v26.4.0"), /npm 11\.17\.0 is required/);
});

test("same Node version with different or absent compressors is rejected", () => {
  for (const [name, other] of [["zlib", "1.2.12"], ["brotli", "1.1.0"]]) {
    for (const value of [other, undefined]) {
      assert.throws(() => assertToolchain({ ...qualified, [name]: value }, npm),
        (error) => error.message.includes(`${name} ${qualified[name]} is required`)
          && error.message.includes("Use the official Node 26.4.0 distribution"));
    }
  }
});
