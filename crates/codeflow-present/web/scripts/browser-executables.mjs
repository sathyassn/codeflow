import { access } from "node:fs/promises";

/**
 * The installed Chrome or Chromium a check drives: `CF_PRESENT_BROWSER` when
 * it is set, else the first of the usual macOS and Linux install paths.
 */
export async function installedChrome() {
  const candidates = [
    process.env.CF_PRESENT_BROWSER,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
  ].filter(Boolean);
  for (const candidate of candidates) {
    try {
      await access(candidate);
      return candidate;
    } catch {
      // Continue to the next explicit executable candidate.
    }
  }
  throw new Error("No qualified browser executable found; set CF_PRESENT_BROWSER");
}

/**
 * The Playwright build of `engine` that the locked playwright-core expects.
 * Playwright resolves it for this platform and honours
 * `PLAYWRIGHT_BROWSERS_PATH`; a missing build fails with its install command.
 */
export async function playwrightBuild(engine) {
  const path = engine.executablePath();
  try {
    await access(path);
  } catch {
    throw new Error(`No Playwright ${engine.name()} build at ${path}; run \`npx playwright-core install ${engine.name()}\``);
  }
  return path;
}
