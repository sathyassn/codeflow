import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

// TSK-1552 AC-2: a Firefox or WebKit start that fails or times out names the
// browser, its version, the Playwright build, the host system and the launch
// log, and still fails the check. The helper is loaded inside each test so
// the first test reports its own assertion even where the helper is absent.
const here = dirname(fileURLToPath(import.meta.url));
const helper = () => import("./browser-start.mjs");

const FIREFOX = "/cache/ms-playwright/firefox-1538/firefox/Nightly.app/Contents/MacOS/firefox";
const WEBKIT = "/cache/ms-playwright/webkit-2210/pw_run.sh";
const PLIST = "<plist><dict><key>ProductBuildVersion</key>\n\t<string>26B131</string>\n\t<key>ProductVersion</key>\n\t<string>27.2.1</string></dict></plist>";
const WEBKIT_INFO = "<plist version=\"1.0\"><dict>\n\t<key>CFBundleIdentifier</key>\n\t<string>com.apple.WebKit</string>\n\t<key>CFBundleShortVersionString</key>\n\t<string>626+</string>\n</dict></plist>";
const TIMEOUT ="browserType.launch: Timeout 180000ms exceeded.\nCall log:\n  - <launching> firefox -no-remote -headless\n  - <launched> pid=4242";

function files(entries) {
  return async (path) => {
    if (path in entries) return entries[path];
    throw Object.assign(new Error(`ENOENT: ${path}`), { code: "ENOENT" });
  };
}

function failingEngine(executable, failure, seen = []) {
  return {
    executablePath: () => executable,
    launch: async (...args) => { seen.push(args); throw failure; },
  };
}

function clock(...times) {
  return () => times.shift();
}

test("browser-check.mjs starts Firefox and WebKit through the start-failure message", async () => {
  const source = await readFile(join(here, "browser-check.mjs"), "utf8");
  assert.ok(
    source.includes('from "./browser-start.mjs"'),
    "browser-check.mjs launches Firefox and WebKit without the start-failure message, so a failed start ends in a bare timeout",
  );
  assert.ok(!source.includes("engine.launch("), "browser-check.mjs still calls engine.launch directly");
  assert.equal(source.split("launchOrExplain(name, engine)").length - 1, 2, "both engine launches use the start-failure message");
});

test("a Firefox start that times out names the browser, the build, the system and the launch log", async () => {
  const { launchOrExplain } = await helper();
  const seen = [];
  const cause = new Error(TIMEOUT);
  const host = {
    platform: "darwin",
    now: clock(1_000, 181_250),
    readText: files({
      "/cache/ms-playwright/firefox-1538/firefox/Nightly.app/Contents/Resources/application.ini": "[App]\nVendor=Mozilla\nVersion=153.0\nBuildID=20260722115600\n",
      "/System/Library/CoreServices/SystemVersion.plist": PLIST,
    }),
  };
  const error = await launchOrExplain("firefox", failingEngine(FIREFOX, cause, seen), undefined, host).then(
    () => assert.fail("a failed start returned a browser"),
    (thrown) => thrown,
  );
  assert.equal(
    error.message,
    "present-browser: Firefox 153.0 (Playwright build 1538) did not start on macOS 27.2.1 (26B131) within 180250 ms. "
      + `Playwright's launch log: ${TIMEOUT}. `
      + "This is a browser start failure before any page; the host or session is a known candidate, see TSK-348 AC-7. The target fails.",
  );
  assert.equal(error.cause, cause, "the original launch error is kept as the cause");
  assert.deepEqual(seen, [[]], "the launch took no option the check did not give");
});

test("a WebKit start that fails uses the same text, with the version from WebKit.framework", async () => {
  const { launchOrExplain } = await helper();
  const host = {
    platform: "darwin",
    now: clock(0, 42),
    readText: files({
      "/cache/ms-playwright/webkit-2210/WebKit.framework/Resources/Info.plist": WEBKIT_INFO,
      "/System/Library/CoreServices/SystemVersion.plist": PLIST,
    }),
  };
  await assert.rejects(
    launchOrExplain("webkit", failingEngine(WEBKIT, new Error("spawn ENOENT")), undefined, host),
    {
      message: "present-browser: WebKit 626+ (Playwright build 2210) did not start on macOS 27.2.1 (26B131) within 42 ms. "
        + "Playwright's launch log: spawn ENOENT. "
        + "This is a browser start failure before any page; the host or session is a known candidate, see TSK-348 AC-7. The target fails.",
    },
  );
});

test("a WebKit build without its Info.plist reads as version unknown", async () => {
  const { startFailureMessage } = await helper();
  const text = await startFailureMessage("webkit", { executablePath: () => WEBKIT }, 1, new Error("closed"), {
    platform: "darwin",
    readText: files({ "/System/Library/CoreServices/SystemVersion.plist": PLIST }),
  });
  assert.ok(text.startsWith("present-browser: WebKit version unknown (Playwright build 2210) did not start on macOS 27.2.1 (26B131) within 1 ms. "), text);
});

test("an executable path that is not a string reads as unknown and keeps the launch log and cause", async () => {
  const { launchOrExplain } = await helper();
  for (const malformed of [null, undefined, 42, { path: FIREFOX }, ""]) {
    const cause = new Error(TIMEOUT);
    const host = { platform: "darwin", now: clock(0, 7), readText: files({ "/System/Library/CoreServices/SystemVersion.plist": PLIST }) };
    const error = await launchOrExplain("firefox", failingEngine(malformed, cause), undefined, host).then(
      () => assert.fail("a failed start returned a browser"),
      (thrown) => thrown,
    );
    assert.equal(
      error.message,
      "present-browser: Firefox version unknown (Playwright build unknown) did not start on macOS 27.2.1 (26B131) within 7 ms. "
        + `Playwright's launch log: ${TIMEOUT}. `
        + "This is a browser start failure before any page; the host or session is a known candidate, see TSK-348 AC-7. The target fails.",
      `executable path ${String(malformed)}`,
    );
    assert.equal(error.cause, cause, "the original launch error is kept as the cause");
  }
});

test("a stalled metadata read cannot hold back the launch error", async () => {
  const { launchOrExplain } = await helper();
  const cause = new Error(TIMEOUT);
  const host = { platform: "darwin", now: clock(0, 9), metadataLimitMs: 50, readText: () => new Promise(() => {}) };
  let timer;
  const stalled = Symbol("stalled");
  const outcome = await Promise.race([
    launchOrExplain("firefox", failingEngine(FIREFOX, cause), undefined, host).then(
      () => assert.fail("a failed start returned a browser"),
      (thrown) => thrown,
    ),
    new Promise((resolve) => { timer = setTimeout(() => resolve(stalled), 1_000); }),
  ]);
  clearTimeout(timer);
  assert.notEqual(outcome, stalled, "the launch error waited on a stalled metadata read");
  assert.equal(
    outcome.message,
    "present-browser: Firefox version unknown (Playwright build 1538) did not start on macOS (version unknown) within 9 ms. "
      + `Playwright's launch log: ${TIMEOUT}. `
      + "This is a browser start failure before any page; the host or session is a known candidate, see TSK-348 AC-7. The target fails.",
  );
  assert.equal(outcome.cause, cause, "the original launch error is kept as the cause");
});

test("a failure while building the message still throws the engine, the launch log and the cause", async () => {
  const { launchOrExplain } = await helper();
  const cause = new Error(TIMEOUT);
  const host = {
    platform: "linux",
    now: clock(0, 11),
    osType: () => { throw new Error("os type unavailable"); },
    osRelease: () => "6.8.0",
    readText: files({}),
  };
  const error = await launchOrExplain("webkit", failingEngine(WEBKIT, cause), undefined, host).then(
    () => assert.fail("a failed start returned a browser"),
    (thrown) => thrown,
  );
  assert.equal(
    error.message,
    "present-browser: WebKit did not start within 11 ms; its version and the host details could not be read (os type unavailable). "
      + `Playwright's launch log: ${TIMEOUT}. `
      + "This is a browser start failure before any page; the host or session is a known candidate, see TSK-348 AC-7. The target fails.",
  );
  assert.equal(error.cause, cause, "the original launch error is kept as the cause");
});

test("browser-check.mjs runs its cleanup through cleanUpAfter, so a cleanup failure cannot replace a start failure", async () => {
  const source = await readFile(join(here, "browser-check.mjs"), "utf8");
  assert.ok(
    source.includes("await cleanUpAfter(checkOutcome, async () => {"),
    "browser-check.mjs cleans up in a bare finally, so a cleanup failure replaces a Firefox or WebKit start failure",
  );
  assert.ok(!source.includes('} finally {\n  await closeOwned("chromium", browser);'), "the cleanup still runs in a bare finally");
});

test("a cleanup failure after a check failure keeps the check failure first and names both", async () => {
  const { cleanUpAfter } = await helper();
  assert.equal(typeof cleanUpAfter, "function", "browser-start.mjs has no cleanup helper, so a cleanup failure replaces the start failure");
  const startFailure = new Error("present-browser: Firefox 153.0 (Playwright build 1538) did not start");
  const cleanupFailure = new Error("chromium: its profile outlived the browser: playwright_chromiumdev_profile-x");
  const error = await cleanUpAfter({ failed: true, error: startFailure }, async () => { throw cleanupFailure; }).then(
    () => assert.fail("a failed check passed"),
    (thrown) => thrown,
  );
  assert.ok(error instanceof AggregateError, "both failures are kept");
  assert.deepEqual(error.errors, [startFailure, cleanupFailure], "the start failure comes first");
  assert.equal(error.cause, startFailure);
  assert.equal(
    error.message,
    "present-browser: Firefox 153.0 (Playwright build 1538) did not start\nThe cleanup after it also failed: chromium: its profile outlived the browser: playwright_chromiumdev_profile-x",
  );
});

test("the cleanup helper still fails on either failure alone and passes when both succeed", async () => {
  const { cleanUpAfter } = await helper();
  assert.equal(typeof cleanUpAfter, "function", "browser-start.mjs has no cleanup helper");
  const startFailure = new Error("start failed");
  const cleanupFailure = new Error("close failed");
  let cleaned = 0;
  await assert.rejects(cleanUpAfter({ failed: true, error: startFailure }, async () => { cleaned += 1; }), (thrown) => thrown === startFailure);
  await assert.rejects(cleanUpAfter({ failed: false, error: undefined }, async () => { throw cleanupFailure; }), (thrown) => thrown === cleanupFailure);
  assert.equal(await cleanUpAfter({ failed: false, error: undefined }, async () => { cleaned += 1; }), undefined);
  assert.equal(cleaned, 2, "the cleanup runs in every case");
});

test("on a host other than macOS the message names the system type and release", async () => {
  const { startFailureMessage } = await helper();
  const text = await startFailureMessage("firefox", { executablePath: () => "/pw/firefox-1538/firefox/firefox" }, 5, new Error("closed"), {
    platform: "linux",
    osType: () => "Linux",
    osRelease: () => "6.8.0",
    readText: files({ "/pw/firefox-1538/firefox/application.ini": "Version=153.0\n" }),
  });
  assert.ok(text.startsWith("present-browser: Firefox 153.0 (Playwright build 1538) did not start on Linux 6.8.0 within 5 ms. "), text);
});

test("a start that succeeds returns the browser and passes the options through unchanged", async () => {
  const { launchOrExplain } = await helper();
  const browser = { name: "stub browser" };
  const seen = [];
  const engine = { executablePath: () => FIREFOX, launch: async (...args) => { seen.push(args); return browser; } };
  assert.equal(await launchOrExplain("firefox", engine), browser);
  const options = { headless: true };
  assert.equal(await launchOrExplain("firefox", engine, options), browser);
  assert.deepEqual(seen, [[], [options]]);
  assert.equal(seen[1][0], options, "the options object is passed as given");
});
