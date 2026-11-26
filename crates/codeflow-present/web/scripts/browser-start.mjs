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
 */
export async function launchOrExplain(name, engine, options, host = {}) {
  const now = host.now ?? Date.now;
  const started = now();
  try {
    return options === undefined ? await engine.launch() : await engine.launch(options);
  } catch (error) {
    const elapsed = Math.max(0, Math.round(now() - started));
    throw new Error(await startFailureMessage(name, engine, elapsed, error, host), { cause: error });
  }
}

const ENGINE_NAMES = { chromium: "Chromium", firefox: "Firefox", webkit: "WebKit" };

export async function startFailureMessage(name, engine, elapsedMs, error, host = {}) {
  const readText = host.readText ?? ((path) => readFile(path, "utf8"));
  const platform = host.platform ?? process.platform;
  const label = ENGINE_NAMES[name] ?? name;
  let executable = "";
  try { executable = engine.executablePath(); } catch { executable = ""; }
  const revision = executable.match(/(?:chromium|firefox|webkit)[-_](\d+)/u)?.[1] ?? "unknown";
  const version = await browserVersion(executable, readText);
  const system = await systemName(platform, readText, host);
  const log = (error instanceof Error ? error.message : String(error)).trim() || "(empty)";
  return `present-browser: ${label} ${version} (Playwright build ${revision}) did not start on ${system} within ${elapsedMs} ms. `
    + `Playwright's launch log: ${log}. `
    + "This is a browser start failure before any page; the host or session is a known candidate, see TSK-348 AC-7. The target fails.";
}

// Firefox keeps its version in application.ini, beside the binary on Linux
// and in Contents/Resources inside a macOS bundle. WebKit has no such file.
async function browserVersion(executable, readText) {
  if (!executable) return "version unknown";
  for (const path of [join(dirname(executable), "application.ini"), join(dirname(executable), "../Resources/application.ini")]) {
    let text;
    try { text = await readText(path); } catch { continue; }
    const version = text.match(/^Version=(.+)$/mu)?.[1]?.trim();
    if (version) return version;
  }
  return "version unknown";
}

async function systemName(platform, readText, host) {
  if (platform === "darwin") {
    try {
      const plist = await readText("/System/Library/CoreServices/SystemVersion.plist");
      const field = (key) => plist.match(new RegExp(`<key>${key}</key>\\s*<string>([^<]+)</string>`, "u"))?.[1];
      const version = field("ProductVersion");
      const build = field("ProductBuildVersion");
      if (version) return `macOS ${version} (${build ?? "build unknown"})`;
    } catch { /* fall through to the generic name */ }
    return "macOS (version unknown)";
  }
  const osType = host.osType ?? type;
  const osRelease = host.osRelease ?? release;
  return `${osType()} ${osRelease()}`;
}
