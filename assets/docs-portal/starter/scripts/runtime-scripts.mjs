// The runtime's own inline scripts. Page content carries no script, so every
// inline script a built page may carry comes from the runtime's templates:
// the pre-paint display script this portal writes from its configured theme,
// and the fixed scripts Starlight and the display panel emit. The fixed
// scripts' hashes are committed in runtime-scripts.json with the lockfile
// they were built from; regenerate the list whenever the lockfile changes:
//
//   npm run build && node scripts/runtime-scripts.mjs dist
//
// `validate --portal` and the browser gate hold every inline script outside
// the page content to that list and to the pre-paint script for the theme.
import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import committed from "./runtime-scripts.json" with { type: "json" };

export const RUNTIME_SCRIPTS_FILE = "scripts/runtime-scripts.json";
export const REGENERATE = committed.regenerate;
const THEME = "__THEME__";

// The pre-paint display script for a theme, as the page inlines it.
export function prePaintScript(theme, list = committed) {
  return list.pre_paint_template.replace(THEME, JSON.stringify(theme));
}

// A script's text as a browser reads it (line endings normalized), hashed.
export function scriptSha256(text) {
  return createHash("sha256").update(text.replace(/\r\n?/g, "\n"), "utf8").digest("hex");
}

// The hashes an inline script outside the page content may have.
export function allowedInlineScripts(theme, list = committed) {
  return new Set([...list.scripts.map((script) => script.sha256), scriptSha256(prePaintScript(theme, list))]);
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

async function main(argv) {
  const check = argv.includes("--check");
  const dist = argv.filter((argument) => argument !== "--check")[0] ?? "dist";
  const root = process.cwd();
  const config = JSON.parse(await readFile(path.join(root, "portal.config.json"), "utf8"));
  const lock = createHash("sha256").update(await readFile(path.join(root, "package-lock.json"))).digest("hex");
  const next = { ...committed, lock_sha256: lock, scripts: await builtRuntimeScripts(path.resolve(root, dist), config.theme) };
  const text = `${JSON.stringify(next, null, 2)}\n`;
  const file = path.join(root, RUNTIME_SCRIPTS_FILE);
  if (check) {
    if (text !== await readFile(file, "utf8")) {
      console.error(`${RUNTIME_SCRIPTS_FILE} does not list the inline scripts of ${dist}; run: ${REGENERATE}`);
      process.exitCode = 1;
    } else console.log(`${RUNTIME_SCRIPTS_FILE}: ${next.scripts.length} runtime inline script(s), current`);
    return;
  }
  await writeFile(file, text);
  console.log(`${RUNTIME_SCRIPTS_FILE}: recorded ${next.scripts.length} runtime inline script(s) from ${dist}`);
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) await main(process.argv.slice(2));
