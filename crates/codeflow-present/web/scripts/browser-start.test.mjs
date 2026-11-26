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
const TIMEOUT = "browserType.launch: Timeout 180000ms exceeded.\nCall log:\n  - <launching> firefox -no-remote -headless\n  - <launched> pid=4242";

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

test("a WebKit start that fails uses the same text, with its version unknown", async () => {
  const { launchOrExplain } = await helper();
  const host = { platform: "darwin", now: clock(0, 42), readText: files({ "/System/Library/CoreServices/SystemVersion.plist": PLIST }) };
  await assert.rejects(
    launchOrExplain("webkit", failingEngine(WEBKIT, new Error("spawn ENOENT")), undefined, host),
    {
      message: "present-browser: WebKit version unknown (Playwright build 2210) did not start on macOS 27.2.1 (26B131) within 42 ms. "
        + "Playwright's launch log: spawn ENOENT. "
        + "This is a browser start failure before any page; the host or session is a known candidate, see TSK-348 AC-7. The target fails.",
    },
  );
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
