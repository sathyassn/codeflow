// Shared fixture scaffolding for the portal test suites: a bare repository
// with a portal configuration, a self-contained copy of the runtime that can
// be built end to end, and the git and adapter helpers both use. Every suite
// builds its fixtures from here so a scaffolding fix reaches all of them.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cp, mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const adapterPath = fileURLToPath(new URL("../scripts/adapter.mjs", import.meta.url));
export const starterRoot = fileURLToPath(new URL("..", import.meta.url));

const FIXTURE_LAYERS = [
  { id: "orient", label: "Orient", description: "Orientation", paths: ["docs/product.md"] },
  { id: "system", label: "System", description: "System", prefixes: ["docs/decisions"] },
  { id: "reference", label: "Reference", description: "Reference", fallback: true },
];

export async function configureFixture(root, overrides) {
  const configPath = path.join(root, "portal.config.json");
  const config = JSON.parse(await readFile(configPath, "utf8"));
  await writeFile(configPath, `${JSON.stringify({ ...config, ...overrides }, null, 2)}\n`);
}

export async function portalFixture() {
  return initializedFixture(path.join(os.tmpdir(), "codeflow-portal-adapter-"), async (root) => {
    await mkdir(path.join(root, ".codeflow"));
    await mkdir(path.join(root, "docs"));
    await writeFile(path.join(root, ".codeflow/project.toml"), "schema_version = 1\n");
    await writeFile(path.join(root, "portal.config.json"), `${JSON.stringify({
      schema_version: 1,
      title: "Fixture",
      description: "Adapter fixture",
      theme: "signal",
      repository_url: null,
      repository_root: ".",
      release_version: null,
      primitive_tokens: null,
      source_roots: ["docs"],
      exclude: [],
      layers: FIXTURE_LAYERS,
      base: "/",
    }, null, 2)}\n`);
    git(root, ["init", "-q"]);
    git(root, ["config", "user.email", "portal-tests@codeflow.invalid"]);
    git(root, ["config", "user.name", "CodeFlow portal tests"]);
    commitFixture(root, "initialize fixture");
  });
}

export async function selfContainedPortalFixture() {
  return initializedFixture(path.join(starterRoot, ".portal-test-runtime-"), async (root) => {
    for (const item of [".gitignore", ".node-version", "astro.config.mjs", "package.json", "package-lock.json", "portal.config.json", "scripts", "src", "public", "tsconfig.json"]) {
      await cp(path.join(starterRoot, item), path.join(root, item), { recursive: true });
    }
    await mkdir(path.join(root, ".codeflow"));
    await mkdir(path.join(root, "docs"));
    await writeFile(path.join(root, ".codeflow/project.toml"), "schema_version = 1\n");
    const configPath = path.join(root, "portal.config.json");
    const config = JSON.parse(await readFile(configPath, "utf8"));
    Object.assign(config, {
      repository_root: ".",
      source_roots: ["docs"],
      exclude: [],
      primitive_tokens: null,
      repository_url: null,
      release_version: null,
      records: { enabled: false, layer: null, pointers: [] },
      layers: FIXTURE_LAYERS,
      base: "/",
      // The consumer's own carriers, classes and bindings name its sources,
      // which this fixture does not carry.
      page_carriers: [],
      page_classes: [],
      figures: [],
    });
    await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
    await writeFile(path.join(root, "docs/seed.md"), "# Seed\n");
    git(root, ["init", "-q"]);
    git(root, ["config", "user.email", "portal-tests@codeflow.invalid"]);
    git(root, ["config", "user.name", "CodeFlow portal tests"]);
    commitFixture(root, "initialize self-contained fixture");
  });
}

export async function initializedFixture(prefix, initialize) {
  const root = await mkdtemp(prefix);
  try {
    await initialize(root);
    return root;
  } catch (error) {
    try { await rm(root, { recursive: true, force: true }); }
    catch (cleanupError) {
      throw new AggregateError([error, cleanupError], "Fixture initialization and owned-root cleanup failed");
    }
    throw error;
  }
}

export function commitFixture(root, message, allowEmpty = false) {
  git(root, ["add", "-A"]);
  git(root, ["commit", "-q", ...(allowEmpty ? ["--allow-empty"] : []), "-m", message]);
}

export function git(root, args) {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout;
}

export function runAdapter(root, expectSuccess = true) {
  const result = spawnSync(process.execPath, [adapterPath], { cwd: root, encoding: "utf8" });
  if (expectSuccess) assert.equal(result.status, 0, result.stderr);
  else assert.notEqual(result.status, 0, result.stdout);
  return result;
}

export function runLocalAdapter(root, expectSuccess = true) {
  const result = spawnSync(process.execPath, [path.join(root, "scripts/adapter.mjs")], { cwd: root, encoding: "utf8" });
  if (expectSuccess) assert.equal(result.status, 0, result.stderr);
  else assert.notEqual(result.status, 0, result.stdout);
  return result;
}

export function buildFixture(root, expectSuccess = true) {
  const result = spawnSync(process.execPath, [path.join(starterRoot, "node_modules/astro/bin/astro.mjs"), "build"], { cwd: root, encoding: "utf8", timeout: 110_000 });
  if (expectSuccess) assert.equal(result.status, 0, result.stderr || result.stdout);
  return result;
}
