import { createHash } from "node:crypto";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { buildAssets } from "./build.mjs";

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const crateRoot = resolve(webRoot, "..");
const repoRoot = resolve(crateRoot, "../..");
const committedAssets = join(crateRoot, "assets");

run("npx", ["tsc", "--noEmit"]);
checkBinaryAttributes();
run("node", ["--test", "scripts/toolchain.test.mjs"]);
await checkSelectorOffsets();
await checkEntityLabelParity();

const scratch = await mkdtemp(join(tmpdir(), "cf-present-check-"));
try {
  const first = join(scratch, "first");
  const second = join(scratch, "second");
  await buildAssets(first);
  await buildAssets(second);
  const firstDigest = await treeDigest(first);
  const secondDigest = await treeDigest(second);
  if (firstDigest !== secondDigest) throw new Error("Two clean asset builds were not byte-identical");

  const builtDigest = await treeDigest(first);
  const committedDigest = await treeDigest(committedAssets, ["supply-chain"]);
  if (builtDigest !== committedDigest) {
    throw new Error("Committed assets differ from a clean build; run npm run build");
  }

  const manifest = JSON.parse(await readFile(join(first, "manifest.json"), "utf8"));
  checkManifest(manifest);
  process.stdout.write(`cf-present web checks passed; reproducible tree ${firstDigest}\n`);
} finally {
  await rm(scratch, { recursive: true, force: true });
}

// The grammar draws the ids and labels the service's entity table resolves
// (SPC-014 B2): both sides are pinned to one golden file, which a Rust test
// in codeflow-present also asserts.
async function checkEntityLabelParity() {
  const { renderFigure } = await import("../src/figure-grammar.mjs");
  const fixtures = join(crateRoot, "tests/fixtures/contract-v2");
  const framed = JSON.parse(await readFile(join(fixtures, "documents/v2-framed.json"), "utf8"));
  const golden = JSON.parse(await readFile(join(fixtures, "entities/landing.json"), "utf8"));
  const declaration = framed.blocks.find((block) => block.id === "landing").declaration;
  const html = renderFigure(declaration, { idPrefix: "parity", number: 1 });
  const unescape = (value) => value.replaceAll("&quot;", '"').replaceAll("&#39;", "'").replaceAll("&lt;", "<").replaceAll("&gt;", ">").replaceAll("&amp;", "&");
  const entities = (text) => [...text.matchAll(/data-cf-entity="([^"]+)" data-cf-entity-label="([^"]*)"/gu)].map((match) => ({ id: match[1], label: unescape(match[2]) }));
  const rows = [];
  for (const variant of ["wide", "narrow"]) {
    const drawing = html.match(new RegExp(`<svg class="cf-fig-svg cf-fig-svg--${variant}"[\\s\\S]*?</svg>`, "u"))?.[0] ?? "";
    rows.push(...entities(drawing).map((entity) => ({ id: entity.id, variant, label: entity.label })));
  }
  const legend = html.match(/<ul class="cf-legend"[\s\S]*?<\/ul>/u)?.[0] ?? "";
  rows.push(...entities(legend).map((entity) => ({ id: entity.id, variant: null, label: entity.label })));
  rows.sort((left, right) => {
    const key = (row) => `${JSON.stringify(row.variant)}|${row.id}`;
    return key(left) < key(right) ? -1 : key(left) > key(right) ? 1 : 0;
  });
  if (JSON.stringify(rows) !== JSON.stringify(golden)) {
    throw new Error(`the grammar's entity table differs from entities/landing.json:\n${JSON.stringify(rows, null, 2)}`);
  }
}

async function checkSelectorOffsets() {
  const { selectorFromOffsets } = await import("../src/selection.ts");
  const text = "Plan 🧭 carefully";
  const selector = selectorFromOffsets(text, 5, 7);
  if (selector.exact !== "🧭" || selector.start_utf16 !== 5 || selector.end_utf16 !== 7) {
    throw new Error("UTF-16 selector contract failed");
  }
  let rejected = false;
  try {
    selectorFromOffsets(text, -1, 2);
  } catch (error) {
    rejected = error instanceof RangeError;
  }
  if (!rejected) throw new Error("Invalid selector offsets were accepted");
}

function checkManifest(manifest) {
  if (manifest.schema_version !== 1) throw new Error("Unexpected asset manifest schema");
  const assets = manifest.service.assets;
  if (!Array.isArray(assets) || !assets.length) throw new Error("Service asset manifest is empty");
  if (assets.some((asset) => asset.content_encoding !== "br" || !asset.stored_path.endsWith(".br"))) {
    throw new Error("Service payloads must be Brotli-only");
  }
  const appPath = manifest.service.entrypoints["present.app"];
  const app = assets.find((asset) => asset.request_path === appPath);
  if (!app) throw new Error("Canonical app entrypoint is missing");
  assertLazyEntryPaths(app);
  // Negative controls: an extra dynamic import must fail the pin, whether its
  // prefix is new or repeats an allowed one.
  for (const extra of ["/app/assets/chunk-extra-AAAAAAAA.js", "/app/assets/chunk-syntax-ZZZZZZZZ.js"]) {
    const widened = { ...app, imports: [...app.imports, { request_path: extra, kind: "dynamic-import" }] };
    let widenedRefused = false;
    try {
      assertLazyEntryPaths(widened);
    } catch {
      widenedRefused = true;
    }
    if (!widenedRefused) throw new Error(`The lazy entry pin accepted an extra dynamic import ${extra}`);
  }
  const grammarChunks = assets.filter((asset) => /bash|diff|javascript|json|python|rust|toml|typescript|yaml/iu.test(asset.request_path));
  if (grammarChunks.length < 9) throw new Error("The nine curated grammar paths were not emitted separately");
  const exportAsset = manifest.export["present.export"];
  if (exportAsset.content_encoding !== "gzip" || !exportAsset.stored_path.endsWith(".gz")) {
    throw new Error("Universal export renderer must be one gzip payload");
  }
  if (!manifest.service.inline["present.prepaint"]?.csp_sha256?.startsWith("sha256-")) {
    throw new Error("Pre-paint script is missing its CSP hash");
  }
}

// The app entry loads exactly three chunks lazily: syntax highlighting, the
// figure grammar and the bundled fonts. Anything else is a new lazy path to
// review, not a silent addition.
function assertLazyEntryPaths(app) {
  const paths = [...new Set(app.imports
    .filter((item) => item.kind === "dynamic-import")
    .map((item) => item.request_path))].sort();
  const lazy = paths
    .map((path) => path.match(/^\/app\/assets\/chunk-([a-z]+)-[A-Z0-9]+\.js$/u)?.[1] ?? path)
    .sort();
  if (lazy.join(",") !== "figure,fonts,syntax") {
    throw new Error(`The app entry's dynamic imports must be exactly one syntax, one figure and one fonts chunk; found ${paths.join(", ")}`);
  }
}

function checkBinaryAttributes() {
  const paths = [
    "crates/codeflow-present/assets/service/qualification.js.br",
    "crates/codeflow-present/assets/export/qualification.js.gz",
  ];
  const result = spawnSync("git", ["check-attr", "-z", "diff", "merge", "text", "--", ...paths], {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: "pipe",
  });
  if (result.status !== 0) {
    throw new Error(`git check-attr failed\n${result.stdout}\n${result.stderr}`);
  }
  const fields = result.stdout.split("\0").filter(Boolean);
  const attributes = new Map();
  for (let index = 0; index < fields.length; index += 3) {
    attributes.set(`${fields[index]}:${fields[index + 1]}`, fields[index + 2]);
  }
  for (const path of paths) {
    for (const attribute of ["diff", "merge", "text"]) {
      if (attributes.get(`${path}:${attribute}`) !== "unset") {
        throw new Error(`${path} must declare the binary ${attribute} attribute`);
      }
    }
  }
}

async function treeDigest(root, ignoredNames = []) {
  const hash = createHash("sha256");
  const visit = async (directory, prefix = "") => {
    const entries = (await readdir(directory, { withFileTypes: true })).sort((left, right) => left.name.localeCompare(right.name));
    for (const entry of entries) {
      if (ignoredNames.includes(entry.name)) continue;
      const path = join(directory, entry.name);
      const relativePath = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) await visit(path, relativePath);
      else {
        hash.update(relativePath).update("\0").update(await readFile(path)).update("\0");
      }
    }
  };
  await visit(root);
  return hash.digest("hex");
}

function run(command, args) {
  const result = spawnSync(command, args, { cwd: webRoot, encoding: "utf8", stdio: "pipe" });
  if (result.status !== 0) throw new Error(`${command} failed\n${result.stdout}\n${result.stderr}`);
}
