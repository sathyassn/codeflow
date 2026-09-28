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
  // Negative control: one extra dynamic import must fail the pin.
  const widened = { ...app, imports: [...app.imports, { request_path: "/app/assets/chunk-extra-AAAAAAAA.js", kind: "dynamic-import" }] };
  let widenedRefused = false;
  try {
    assertLazyEntryPaths(widened);
  } catch {
    widenedRefused = true;
  }
  if (!widenedRefused) throw new Error("The lazy entry pin accepted an extra dynamic import");
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

// The app entry loads exactly two chunks lazily: syntax highlighting and the
// bundled fonts. Anything else, a diagram renderer included, is a new lazy
// path to review, not a silent addition.
function assertLazyEntryPaths(app) {
  const lazy = [...new Set(app.imports
    .filter((item) => item.kind === "dynamic-import")
    .map((item) => item.request_path.match(/^\/app\/assets\/chunk-([a-z]+)-[A-Z0-9]+\.js$/u)?.[1] ?? item.request_path))].sort();
  if (lazy.join(",") !== "fonts,syntax") {
    throw new Error(`The app entry's dynamic imports must be exactly the syntax and fonts chunks; found ${lazy.join(", ")}`);
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
