// Catalog compatibility checks run in the full docs-portal gate.
// The opt-in portal-build tests are POSIX only.
import assert from "node:assert/strict";
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { spawnSync } from "node:child_process";
import { capabilityFrontmatterRecord, parseMarkdown, sha256 } from "../docs-portal/scripts/lib.mjs";
import { GENERATOR } from "../docs-portal/scripts/generator.mjs";
import { commitFixture, configureFixture, portalFixture, runAdapter, starterRoot } from "../docs-portal/tests/portal-fixture.mjs";
const treeRemoval = { recursive: true, force: true, maxRetries: 10, retryDelay: 100 };

test("capability frontmatter preserves loader delimiter and field bounds", () => {
  const { frontmatter, body } = parseMarkdown("--- \t\r\nid: CAP-001\r\n--- \t\r\nProse", "docs/capabilities/CAP-001-first.md");
  assert.equal(frontmatter.id, "CAP-001");
  assert.equal(body, "Prose");
  const full = { id: "CAP-001", ...Object.fromEntries(Array.from({ length: 31 }, (_, index) => [`field${index}`, "value"])) };
  assert.equal(capabilityFrontmatterRecord(full, "fixture").fields.length, 32);
  assert.throws(() => capabilityFrontmatterRecord({ ...full, extra: "value" }, "fixture"), /more than 32 fields/);
  assert.throws(() => capabilityFrontmatterRecord({ id: "CAP-001", summary: "x".repeat(1025) }, "fixture"), /rendered bounds/);
});

test("the capability value bound applies per list item, as the loader's does", () => {
  const items = Array.from({ length: 64 }, (_, index) => `test: crates/app/tests/case_${index}.rs`);
  assert.ok(items.join(", ").length > 1024);
  assert.equal(capabilityFrontmatterRecord({ id: "CAP-001", verified_by: items }, "fixture").lookup.get("verified_by"), items.join(", "));
  assert.throws(() => capabilityFrontmatterRecord({ id: "CAP-001", verified_by: ["x".repeat(1025)] }, "fixture"), /rendered bounds/);
});

for (const layout of ["legacy", "folder", "mixed"]) {
  test(`capability lookup counts the ${layout} catalog`, async () => {
    const root = await portalFixture();
    try {
      await configureFixture(root, {
        page_classes: [{ source: "docs/capabilities.md", class: "derived-lookup", derive: "capability-registry" }],
      });
      const legacy = "```yaml\nid: CAP-001\nname: first\narea: engine\nstatus: planned\n```\n";
      await writeFile(path.join(root, "docs/capabilities.md"), `# Capabilities\n\n${layout === "folder" ? "" : legacy}`);
      if (layout === "legacy") {
        await mkdir(path.join(root, "docs/capabilities"));
        await writeFile(path.join(root, "docs/capabilities/CAP-001-first.md"), "# First\n\nLegacy prose companion.\n");
      }
      if (layout !== "legacy") {
        await mkdir(path.join(root, "docs/capabilities"));
        await writeFile(path.join(root, "docs/capabilities/CAP-002-second.md"), [
          "---", "id: CAP-002", "uid: 12345678-1234-4234-8234-123456789abc", "name: second",
          "summary: The second capability.", "area: engine", "status: planned", "stage: unassessed",
          "kind: functional", "audience: [agent]", "publish: false", "tags: []", "verified_by: []",
          "since: 4.0.0", "former_ids: []", "epics: []", "adrs: []", "specs: []", "depends_on: []",
          "related: []", "capabilities: []", "---", "", "# Second", "", "The record's prose.", "",
        ].join("\n"));
      }
      commitFixture(root, `add ${layout} catalog`);
      runAdapter(root);
      const evidence = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
      const registry = evidence.pages.find((page) => page.source_path === "docs/capabilities.md");
      assert.equal(registry.stale, false);
      assert.deepEqual(registry.lookup, { derive: "capability-registry", rows: layout === "mixed" ? 2 : 1 });
      const rendered = await readFile(path.join(root, registry.output_markdown), "utf8");
      if (layout !== "legacy") assert.match(rendered, /second/);
      if (layout !== "folder") assert.match(rendered, /first/);
      if (layout === "mixed") {
        await configureFixture(root, { exclude: ["docs/capabilities"] });
        commitFixture(root, "exclude individual record pages");
        runAdapter(root);
        const excluded = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
        assert.equal(excluded.pages.find((page) => page.source_path === "docs/capabilities.md").lookup.rows, 2);
        await writeFile(path.join(root, "docs/capabilities/CAP-002-second.md"), "uncommitted edit\n");
        assert.match(runAdapter(root, false).stderr, /snapshot inputs differ|dirty|uncommitted|changed|does not match/);
      }
    } finally { await rm(root, treeRemoval); }
  });
}

// Opt in with CODEFLOW_CATALOG_GATE=1 and CODEFLOW_BIN pointing at the task build.
// Each layout runs the
// actual docs-portal target and Rust fidelity check in a committed repository.
for (const layout of ["legacy", "folder", "mixed"]) {
  test(`docs-portal gate agrees with Rust for the ${layout} catalog`, { skip: process.env.CODEFLOW_CATALOG_GATE !== "1", timeout: 600_000 }, async () => {
    assert.ok(process.env.CODEFLOW_BIN, "CODEFLOW_BIN must point at the task build for opt-in portal tests");
    const root = await mkdtemp(path.join(starterRoot, ".portal-test-runtime-"));
    try {
      const portal = path.join(root, "portal");
      await mkdir(portal);
      for (const item of [".gitignore", ".node-version", "astro.config.mjs", "package.json", "package-lock.json", "scripts", "src", "public", "tsconfig.json"]) {
        await cp(path.join(starterRoot, item), path.join(portal, item), { recursive: true });
      }
      await mkdir(path.join(root, ".codeflow"));
      await mkdir(path.join(root, "docs/capabilities"), { recursive: true });
      await writeFile(path.join(root, ".codeflow/project.toml"), 'schema_version = 1\nareas = ["engine"]\n');
      const config = {
        schema_version: 1, title: "Catalog fixture", description: "Catalog compatibility", theme: "signal",
        repository_url: null, repository_root: "..", release_version: null, primitive_tokens: null,
        source_roots: ["docs"], exclude: [], base: "/",
        layers: [
          { id: "orient", label: "Orient", description: "Orientation", paths: ["docs/product.md"] },
          { id: "system", label: "System", description: "System", prefixes: ["docs/decisions"] },
          { id: "reference", label: "Reference", description: "Catalog", fallback: true },
        ],
        page_classes: [{ source: "docs/capabilities.md", class: "derived-lookup", derive: "capability-registry" }],
        figures: [],
      };
      const configText = `${JSON.stringify(config, null, 2)}\n`;
      await writeFile(path.join(portal, "portal.config.json"), configText);
      await writeFile(path.join(root, ".codeflow/docs-portal.json"), `${JSON.stringify({
        schema_version: 2, root: "portal", starter_version: GENERATOR.version, runtime_ownership: "managed", generator: GENERATOR,
        files: { "portal.config.json": { ownership: "user-owned", pristine_sha256: sha256(configText) } },
      })}\n`);
      const legacy = "```yaml\nid: CAP-001\nname: first\narea: engine\nstatus: planned\n```\n";
      await writeFile(path.join(root, "docs/capabilities.md"), `# Capabilities\n\n${layout === "folder" ? "" : legacy}`);
      if (layout !== "legacy") {
        await writeFile(path.join(root, "docs/capabilities/CAP-002-second.md"), [
          "--- \t", "id: CAP-002", "uid: 12345678-1234-4234-8234-123456789abc", "name: second",
          "summary: The second capability.", "area: engine", "status: planned", "kind: functional",
          "audience: [agent]", "--- \t", "", "# Second", "", "The second capability.", "",
        ].join("\n"));
        await writeFile(path.join(root, "docs/capabilities/vocabulary.yaml"), "inventory: incomplete\ngroups: []\ntags: []\n");
      }
      const quote = (value) => "'" + value.replaceAll("'", "'\"'\"'") + "'";
      const node = quote(process.execPath);
      const binary = path.resolve(process.env.CODEFLOW_BIN);
      const astro = quote(path.join(starterRoot, "node_modules/astro/bin/astro.mjs"));
      const command = `(cd portal && ${node} scripts/adapter.mjs && ${node} ${astro} check && ${node} ${astro} build && ${node} scripts/evidence.mjs) && ${quote(binary)} validate --portal portal`;
      await writeFile(path.join(root, ".codeflow/test-config.json"), `${JSON.stringify({
        schema_version: "1.0", execution: { parallel: false, max_parallel: 1, fail_fast: true }, defaults: {},
        targets: [{ name: "docs-portal", enabled: true, runner: "custom", modes: { full: { command } }, timeout_seconds: 300 }],
      }, null, 2)}\n`);
      for (const args of [["init", "-q"], ["config", "user.name", "Portal tests"], ["config", "user.email", "portal-tests@codeflow.invalid"]]) {
        const result = spawnSync("git", args, { cwd: root, encoding: "utf8" });
        assert.equal(result.status, 0, result.stderr);
      }
      commitFixture(root, `add ${layout} catalog fixture`);
      const gate = spawnSync(binary, ["test", "--only", "docs-portal"], { cwd: root, encoding: "utf8", timeout: 500_000 });
      assert.equal(gate.status, 0, gate.stdout + gate.stderr);
      console.log(`${layout}: ${gate.stdout.trim()}`);
      const evidence = JSON.parse(await readFile(path.join(portal, ".portal/generated/evidence.json"), "utf8"));
      const registry = evidence.pages.find((page) => page.source_path === "docs/capabilities.md");
      assert.equal(registry.lookup.rows, layout === "mixed" ? 2 : 1);
      const html = await readFile(path.join(portal, "dist/reference/capabilities/index.html"), "utf8");
      const table = html.match(/<table\b[^>]*>[\s\S]*?<\/table>/)?.[0];
      assert.ok(table, "the built registry contains its summary table");
      const body = table.match(/<tbody[^>]*>([\s\S]*?)<\/tbody>/)?.[1] ?? "";
      assert.equal((body.match(/<tr\b/g) ?? []).length, registry.lookup.rows);
      // A forged count must fail the Rust check, even though the site built.
      registry.lookup.rows += 1;
      await writeFile(path.join(portal, ".portal/generated/evidence.json"), JSON.stringify(evidence));
      const forged = spawnSync(binary, ["validate", "--portal", "portal"], { cwd: root, encoding: "utf8" });
      assert.notEqual(forged.status, 0);
      assert.match(forged.stdout + forged.stderr, /derived lookup does not match/);
    } finally { await rm(root, treeRemoval); }
  });
}
