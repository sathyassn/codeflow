import { settings, runtime } from "../public/tokens.js";
// The runtime's own inline scripts. Page content carries no script, so every
// inline script a built page may carry comes from the runtime's templates:
// the pre-paint display script this portal writes from its configured theme,
// and the fixed scripts Starlight and the display panel emit. This module is
// the one place that writes the pre-paint script, so it also records that
// script's hash for every theme a configuration may name. runtime-scripts.json
// commits those hashes and the fixed scripts' hashes with the lockfile they
// were built from; regenerate it whenever the lockfile or the tokens change:
//
//   npm run build && node scripts/runtime-scripts.mjs dist
//
// `validate --portal` and the browser gate hold every inline script outside
// the page content to the fixed scripts and to the recorded pre-paint hash
// for the configured theme; neither rebuilds the pre-paint script.
import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { PORTAL_THEMES } from "./lib.mjs";
import committed from "./runtime-scripts.json" with { type: "json" };

export const RUNTIME_SCRIPTS_FILE = "scripts/runtime-scripts.json";
export const RUNTIME_SCRIPTS_SCHEMA = 2;
// A real pre-paint script with its hash, for the Rust validator's tests to
// hash on their side (crates/codeflow-core/src/validate/portal/figures.rs).
// It uses a legacy alias, whatever the portal configures, so changing the
// configured theme never makes it stale.
export const PRE_PAINT_FIXTURE_FILE = "tests/fixtures/pre-paint-script.json";
const PRE_PAINT_FIXTURE_THEME = "signal";
export const REGENERATE = committed.regenerate;
const THEME = "__THEME__";
const SHA256 = /^[0-9a-f]{64}$/;

// The pre-paint display script for a theme, as the page inlines it. Each
// value is inserted by a function so a "$" in it is never read as a
// replacement pattern.
export function prePaintScript(theme, list = committed) {
  return list.pre_paint_template.replace(THEME, () => JSON.stringify(theme)).replace("__DISPLAY__", () => JSON.stringify(settings)).replace("__LAYOUT__", () => JSON.stringify(runtime));
}

// A script's text as a browser reads it (line endings normalized), hashed.
export function scriptSha256(text) {
  return createHash("sha256").update(text.replace(/\r\n?/g, "\n"), "utf8").digest("hex");
}

// The pre-paint script's hash for every theme a configuration may name,
// legacy aliases included, as this module writes the script.
export function prePaintHashes(list = committed) {
  return Object.fromEntries(PORTAL_THEMES.map((theme) => [theme, scriptSha256(prePaintScript(theme, list))]));
}

// What an owner does about a list from before version 2. The old generator
// keeps the committed schema_version, so regenerating alone writes version 1
// again: the version 2 generator comes first. The Rust validator gives the
// same reason (crates/codeflow-core/src/validate/portal/figures.rs).
export const SCHEMA_UPGRADE = `adopt the version ${RUNTIME_SCRIPTS_SCHEMA} generator first (\`codeflow update\` for a managed portal; for a transferred portal, copy scripts/runtime-scripts.mjs from the CodeFlow starter), then regenerate it: ${REGENERATE}; see the changelog entry "The portal's runtime script list is version ${RUNTIME_SCRIPTS_SCHEMA}"`;

// Why a runtime script list cannot be read for a theme, or null when it can.
// The checks and their order match the Rust validator's, so the browser gate
// and `validate --portal` accept and refuse the same lists with the same reason.
export function runtimeScriptsFailure(theme, list) {
  const version = list?.schema_version;
  if (version !== RUNTIME_SCRIPTS_SCHEMA) return `it has schema_version ${Number.isSafeInteger(version) && version >= 0 ? version : "none"}, and this codeflow reads version ${RUNTIME_SCRIPTS_SCHEMA}; ${SCHEMA_UPGRADE}`;
  const hashes = list.pre_paint_sha256;
  if (!hashes || typeof hashes !== "object" || Array.isArray(hashes)) return `it records no pre-paint script hashes; regenerate it: ${REGENERATE}`;
  const recorded = Object.keys(hashes).sort();
  const themes = [...PORTAL_THEMES].sort();
  // Compared name by name: joining with a delimiter would let one key that
  // contains the delimiter stand for several themes.
  if (recorded.length !== themes.length || recorded.some((key, index) => key !== themes[index])) return `its pre-paint script hashes name the themes ${recorded.join(", ")}, not ${themes.join(", ")}; regenerate it: ${REGENERATE}`;
  const prePaint = Object.hasOwn(hashes, theme) ? hashes[theme] : undefined;
  if (typeof prePaint !== "string" || !SHA256.test(prePaint)) return `it records no pre-paint script hash for the theme ${JSON.stringify(theme)}; regenerate it: ${REGENERATE}`;
  if (!Array.isArray(list.scripts)) return "it lists no scripts";
  if (!list.scripts.every((script) => typeof script?.sha256 === "string" && SHA256.test(script.sha256))) return "a script entry has no sha256";
  return null;
}

// The hashes an inline script outside the page content may have: the fixed
// scripts and the recorded pre-paint hash for the theme. A list the reader
// cannot read exactly is refused rather than read loosely.
export function allowedInlineScripts(theme, list = committed) {
  const failure = runtimeScriptsFailure(theme, list);
  if (failure) throw new Error(`the runtime's inline script list ${RUNTIME_SCRIPTS_FILE} cannot be read: ${failure}`);
  return new Set([...list.scripts.map((script) => script.sha256), list.pre_paint_sha256[theme]]);
}

// The inline scripts of a built page, in order.
export function inlineScripts(html) {
  return [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script\s*>/gi)].filter(([, attributes]) => !/\ssrc\s*=/i.test(` ${attributes}`)).map(([, , text]) => text.replace(/\r\n?/g, "\n"));
}

// The fixed inline scripts a build emits: every inline script of its pages
// except the pre-paint script for its theme.
export async function builtRuntimeScripts(dist, theme, list = committed) {
  const walk = async (directory) => (await Promise.all((await readdir(directory, { withFileTypes: true })).map((entry) => {
    const file = path.join(directory, entry.name);
    return entry.isDirectory() ? walk(file) : entry.name.endsWith(".html") ? [file] : [];
  }))).flat();
  const prePaint = scriptSha256(prePaintScript(theme, list));
  const found = new Map();
  for (const file of (await walk(dist)).sort()) {
    for (const text of inlineScripts(await readFile(file, "utf8"))) {
      const sha256 = scriptSha256(text);
      if (sha256 !== prePaint && !found.has(sha256)) found.set(sha256, { sha256, starts: text.trim().slice(0, 48) });
    }
  }
  return [...found.values()].sort((a, b) => (a.sha256 < b.sha256 ? -1 : 1));
}

// The committed list names the lockfile it was built from. A lockfile that
// moved on may bring different runtime scripts, so the check workflow fails
// until the list is regenerated. The build does not, since regenerating needs
// a build.
export async function lockDigestFailure(root) {
  const lock = createHash("sha256").update(await readFile(path.join(root, "package-lock.json"))).digest("hex");
  const list = JSON.parse(await readFile(path.join(root, RUNTIME_SCRIPTS_FILE), "utf8"));
  if (list.lock_sha256 === lock) return null;
  return `${RUNTIME_SCRIPTS_FILE} was built from another package-lock.json (recorded ${String(list.lock_sha256).slice(0, 12)}, current ${lock.slice(0, 12)}); regenerate it: ${REGENERATE}`;
}

async function main(argv) {
  const check = argv.includes("--check");
  const dist = argv.filter((argument) => argument !== "--check")[0] ?? "dist";
  const root = process.cwd();
  const config = JSON.parse(await readFile(path.join(root, "portal.config.json"), "utf8"));
  const lock = createHash("sha256").update(await readFile(path.join(root, "package-lock.json"))).digest("hex");
  const next = {
    schema_version: RUNTIME_SCRIPTS_SCHEMA,
    regenerate: committed.regenerate,
    lock_sha256: lock,
    pre_paint_template: committed.pre_paint_template,
    pre_paint_sha256: prePaintHashes(committed),
    scripts: await builtRuntimeScripts(path.resolve(root, dist), config.theme),
  };
  const script = prePaintScript(PRE_PAINT_FIXTURE_THEME, committed);
  const outputs = [
    [RUNTIME_SCRIPTS_FILE, `${JSON.stringify(next, null, 2)}\n`],
    [PRE_PAINT_FIXTURE_FILE, `${JSON.stringify({ theme: PRE_PAINT_FIXTURE_THEME, sha256: scriptSha256(script), script }, null, 2)}\n`],
  ];
  if (check) {
    for (const [name, text] of outputs) {
      if (text !== await readFile(path.join(root, name), "utf8").catch(() => null)) {
        console.error(`${name} does not record the inline scripts of ${dist} and the current tokens; run: ${REGENERATE}`);
        process.exitCode = 1;
      }
    }
    if (!process.exitCode) console.log(`${RUNTIME_SCRIPTS_FILE}: ${next.scripts.length} runtime inline script(s) and ${PORTAL_THEMES.length} pre-paint hash(es), current`);
    return;
  }
  for (const [name, text] of outputs) await writeFile(path.join(root, name), text);
  console.log(`${RUNTIME_SCRIPTS_FILE}: recorded ${next.scripts.length} runtime inline script(s) from ${dist} and ${PORTAL_THEMES.length} pre-paint hash(es)`);
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) await main(process.argv.slice(2));
