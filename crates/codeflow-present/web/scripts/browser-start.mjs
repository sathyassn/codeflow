import { readFile } from "node:fs/promises";
import { release, type } from "node:os";
import { dirname, join } from "node:path";

/**
 * Starts a Playwright engine other than the check's own Chromium and, when the
 * start fails or times out, throws an error that names the browser, its
 * version, the Playwright build revision, the host system and Playwright's
 * launch log (TSK-1552 AC-2). The launch itself is unchanged: the options go
 * to `engine.launch` as given, and a start that fails still fails the check.
 * The message only replaces a bare timeout with the facts a reader needs to
 * tell a browser start failure from a failure in the page under test.
 *
 * Building the message never replaces or holds back the launch error: the
 * metadata reads share one short time limit, and any failure while building
 * the message still throws an error with the engine name, the launch log and
 * the launch error as its cause.
 */
export async function launchOrExplain(name, engine, options, host = {}) {
  const now = host.now ?? Date.now;
  const started = now();
  try {
    return options === undefined ? await engine.launch() : await engine.launch(options);
  } catch (error) {
    let elapsed = "an unknown number of";
    try { elapsed = Math.max(0, Math.round(now() - started)); } catch { /* keep the unknown elapsed time */ }
    let message;
    try {
      message = await startFailureMessage(name, engine, elapsed, error, host);
    } catch (builderError) {
      message = plainStartFailure(name, elapsed, error, builderError);
    }
    throw new Error(message, { cause: error });
  }
}

const ENGINE_NAMES = { chromium: "Chromium", firefox: "Firefox", webkit: "WebKit" };
const METADATA_LIMIT_MS = 2000;
const CLOSING = "This is a browser start failure before any page; the host or session is a known candidate, see TSK-348 AC-7. The target fails.";

export async function startFailureMessage(name, engine, elapsedMs, error, host = {}) {
  const platform = host.platform ?? process.platform;
  const label = engineLabel(name);
  const executable = executableOf(engine);
  const revision = executable.match(/(?:chromium|firefox|webkit)[-_](\d+)/u)?.[1] ?? "unknown";
  const { readText, stop } = boundedReader(host.readText ?? ((path) => readFile(path, "utf8")), host.metadataLimitMs ?? METADATA_LIMIT_MS);
  try {
    const [version, system] = await Promise.all([browserVersion(executable, readText), systemName(platform, readText, host)]);
    return `present-browser: ${label} ${version} (Playwright build ${revision}) did not start on ${system} within ${elapsedMs} ms. `
      + `Playwright's launch log: ${launchLog(error)}. ${CLOSING}`;
  } finally {
    stop();
  }
}

// The text used when the full message cannot be built. It keeps what matters
// most: which engine failed and what Playwright said.
function plainStartFailure(name, elapsedMs, error, builderError) {
  let reason = "unknown";
  try { reason = launchLog(builderError); } catch { /* keep the unknown reason */ }
  let log = "(unreadable)";
  try { log = launchLog(error); } catch { /* keep the unreadable log */ }
  return `present-browser: ${engineLabel(name)} did not start within ${elapsedMs} ms; its version and the host details could not be read (${reason}). `
    + `Playwright's launch log: ${log}. ${CLOSING}`;
}

function engineLabel(name) {
  return Object.hasOwn(ENGINE_NAMES, name) ? ENGINE_NAMES[name] : String(name);
}

function launchLog(error) {
  return (error instanceof Error ? error.message : String(error)).trim() || "(empty)";
}

// Playwright returns the executable path as a string; anything else, an empty
// string or a throw reads as unknown.
function executableOf(engine) {
  try {
    const path = engine.executablePath();
    return typeof path === "string" ? path : "";
  } catch {
    return "";
  }
}

// Every read made for one message shares one deadline. A read still pending
// at the deadline fails, and the field it fed reads as unknown.
function boundedReader(read, limitMs) {
  let timer;
  const deadline = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`the browser and host details took longer than ${limitMs} ms to read`)), limitMs);
  });
  deadline.catch(() => {});
  return {
    readText: (path) => Promise.race([Promise.resolve().then(() => read(path)), deadline]),
    stop: () => clearTimeout(timer),
  };
}

function plistString(plist, key) {
  return plist.match(new RegExp(`<key>${key}</key>\\s*<string>([^<]+)</string>`, "u"))?.[1]?.trim();
}

// Firefox keeps its version in application.ini, beside the binary on Linux
// and in Contents/Resources inside a macOS bundle. Playwright's WebKit build
// on macOS starts from pw_run.sh beside WebKit.framework, whose Info.plist
// carries the version in CFBundleShortVersionString.
async function browserVersion(executable, readText) {
  if (!executable) return "version unknown";
  const folder = dirname(executable);
  for (const path of [join(folder, "application.ini"), join(folder, "../Resources/application.ini")]) {
    let text;
    try { text = await readText(path); } catch { continue; }
    const version = String(text).match(/^Version=(.+)$/mu)?.[1]?.trim();
    if (version) return version;
  }
  try {
    const version = plistString(String(await readText(join(folder, "WebKit.framework/Resources/Info.plist"))), "CFBundleShortVersionString");
    if (version) return version;
  } catch { /* fall through to unknown */ }
  return "version unknown";
}

async function systemName(platform, readText, host) {
  if (platform === "darwin") {
    try {
      const plist = String(await readText("/System/Library/CoreServices/SystemVersion.plist"));
      const version = plistString(plist, "ProductVersion");
      const build = plistString(plist, "ProductBuildVersion");
      if (version) return `macOS ${version} (${build ?? "build unknown"})`;
    } catch { /* fall through to the generic name */ }
    return "macOS (version unknown)";
  }
  const osType = host.osType ?? type;
  const osRelease = host.osRelease ?? release;
  return `${osType()} ${osRelease()}`;
}

/**
 * Runs the check's cleanup after its work, and keeps the work's failure as
 * the reported error when the cleanup also fails (TSK-1552). With no work
 * failure, a cleanup failure is the error. With both, an AggregateError
 * carries the work failure first, the cleanup failure second, a message that
 * names both, and the work failure as its cause.
 */
export async function cleanUpAfter(outcome, cleanup) {
  try {
    await cleanup();
  } catch (cleanupError) {
    if (!outcome.failed) throw cleanupError;
    throw new AggregateError(
      [outcome.error, cleanupError],
      `${safeMessage(outcome.error)}\nThe cleanup after it also failed: ${safeMessage(cleanupError)}`,
      { cause: outcome.error },
    );
  }
  if (outcome.failed) throw outcome.error;
}

function safeMessage(error) {
  try { return launchLog(error); } catch { return "(unreadable)"; }
}
