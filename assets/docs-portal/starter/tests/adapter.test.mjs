import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { collectBuiltArtifacts, collectPageIds, excerptFor, extractPageRelationships, linkStrictIds, parseMarkdown, publishOwnedCorpus, recoverOwnedCorpus, referencedIds, renderSafeMarkdown, safeRelative, sha256, validateBase, validatePortalConfig, withBase } from "../scripts/lib.mjs";

const adapterPath = fileURLToPath(new URL("../scripts/adapter.mjs", import.meta.url));

test("safe paths reject traversal and platform separators", () => {
  for (const value of ["../x", "/x", "a/../x", "a\\x", ""]) assert.throws(() => safeRelative(value));
  assert.equal(safeRelative("docs/guide.md"), "docs/guide.md");
});

test("frontmatter parsing is strict and deterministic", () => {
  assert.deepEqual(parseMarkdown("---\ntitle: Guide\n---\nBody", "guide.md"), { frontmatter: { title: "Guide" }, body: "Body" });
  assert.throws(() => parseMarkdown("---\ntitle: Guide", "guide.md"), /unclosed/);
  assert.deepEqual(parseMarkdown("\uFEFF---\r\nid: TSK-101\r\n---\r\nBody\r\n", "guide.md"), { frontmatter: { id: "TSK-101" }, body: "Body\n" });
  assert.equal(sha256("same"), sha256("same"));
});

test("raw HTML is escaped by default but fenced examples survive", () => {
  const source = "<!-- editorial note -->\n<script>x</script> and `<tag>&`\n```html\n<div>example</div>\n```";
  const rendered = renderSafeMarkdown(source);
  assert.doesNotMatch(rendered, /editorial note/);
  assert.match(rendered, /&lt;script&gt;/);
  assert.match(rendered, /`<tag>&`/);
  assert.match(rendered, /<div>example<\/div>/);
});

test("strict IDs link outside code but code examples remain literal", () => {
  const routes = new Map([["ADR-0048", "/system/decisions/adr-0048/"]]);
  assert.equal(linkStrictIds("ADR-0048 and `ADR-0048`", routes), "[ADR-0048](/system/decisions/adr-0048/) and `ADR-0048`");
  const examples = "CAP-101\n~~~text\nCAP-102\n~~~\n<!-- CAP-103 -->\n`CAP-104`";
  assert.deepEqual(referencedIds(examples), ["CAP-101"]);
  assert.match(linkStrictIds(examples, new Map([["CAP-101", "/cap/"]])), /^\[CAP-101\]/);
  assert.doesNotMatch(renderSafeMarkdown(examples), /CAP-103/);
});

test("only the canonical capability registry treats YAML fences as identities", () => {
  const capability = "~~~yaml\nid: CAP-101\nepics: [EPC-101]\nadrs: [ADR-101]\n~~~";
  assert.deepEqual(collectPageIds({}, capability, "docs/capabilities.md"), ["CAP-101"]);
  assert.deepEqual(extractPageRelationships({}, capability, "docs/capabilities.md"), [
    { type: "epic", target: "EPC-101", source_id: "CAP-101" }, { type: "decision", target: "ADR-101", source_id: "CAP-101" },
  ]);
  assert.deepEqual(collectPageIds({}, capability, "docs/example.md"), []);
  assert.throws(() => collectPageIds({}, `${capability}\n${capability}`, "docs/capabilities.md"), /duplicate identity/);
});

test("work-record aliases and base paths produce canonical relationships and links", () => {
  const relationships = extractPageRelationships({ epic_id: "EPC-005", specs: ["SPC-005"], depends_on: ["TSK-008"], adrs: ["ADR-0048"] }, "", "project-management/tasks/TSK-009.md");
  assert.deepEqual(relationships.map(({ type }) => type), ["epic", "spec", "depends_on", "decision"]);
  assert.equal(validateBase("/guide/"), "/guide/");
  assert.equal(withBase("/guide/", "system/architecture"), "/guide/system/architecture/");
  for (const invalid of ["guide/", "/guide", "/../guide/", "/guide/?x=1"]) assert.throws(() => validateBase(invalid));
});

test("portal configuration is closed, bounded, and deeply typed", () => {
  const valid = { schema_version: 1, title: "Guide", description: "Repository guide", theme: "signal", repository_url: null, repository_root: "..", source_roots: ["docs"], exclude: [], layers: [
    { id: "orient", label: "Orient", description: "Start", paths: ["docs/product.md"] },
    { id: "system", label: "System", description: "Architecture", prefixes: ["docs/decisions"] },
    { id: "reference", label: "Reference", description: "Other", fallback: true },
  ], base: "/" };
  assert.equal(validatePortalConfig(valid), valid);
  assert.throws(() => validatePortalConfig({ ...valid, allow_html: true }), /unknown key/);
  assert.throws(() => validatePortalConfig({ ...valid, source_roots: "docs" }), /source_roots/);
  assert.throws(() => validatePortalConfig({ ...valid, repository_url: "https://user:secret@example.com/repo" }), /credentials/);
  assert.throws(() => validatePortalConfig({ ...valid, layers: valid.layers.map((layer) => ({ ...layer, fallback: true })) }), /exactly one/);
});

test("source excerpts skip metadata, comments, headings, and example fences", () => {
  const excerpt = excerptFor("---\nid: TSK-101\n---\n# Title\n<!-- note -->\n~~~text\nnot evidence\n~~~\n\nFirst grounded line.\nSecond line.\n");
  assert.deepEqual(excerpt, { start: 10, end: 11, text: "First grounded line.\nSecond line." });
});

test("corpus publication preserves unknown files and rolls every directory back on failure", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-test-"));
  try {
    const corpus = (version) => [
      { live: ".portal/generated", files: new Map([["evidence.json", `evidence-${version}`]]) },
      { live: "src/content/docs", files: new Map([["index.md", `content-${version}`]]) },
      { live: "public", preserveUnknown: true, files: new Map([["llms.txt", `public-${version}`]]) },
    ];
    await mkdir(path.join(root, "public"), { recursive: true });
    await writeFile(path.join(root, "public/favicon.svg"), "user-owned");
    await publishOwnedCorpus(root, corpus("one"));
    await writeFile(path.join(root, "public/notes.txt"), "keep me");
    await assert.rejects(publishOwnedCorpus(root, corpus("two"), { faultAt: "after-publish-1" }), /injected publication failure/);
    assert.equal(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"), "evidence-one");
    assert.equal(await readFile(path.join(root, "src/content/docs/index.md"), "utf8"), "content-one");
    assert.equal(await readFile(path.join(root, "public/llms.txt"), "utf8"), "public-one");
    assert.equal(await readFile(path.join(root, "public/notes.txt"), "utf8"), "keep me");
    await publishOwnedCorpus(root, corpus("two"));
    assert.equal(await readFile(path.join(root, "public/notes.txt"), "utf8"), "keep me");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("publication recovery is coherent at every crash boundary", async () => {
  const boundaries = ["after-journal", "after-prepare-0", "after-prepare-1", "after-prepare-2", "after-prepared", "after-backup-0", "after-publish-0", "after-backup-1", "after-publish-1", "after-backup-2", "after-publish-2", "after-commit"];
  for (const boundary of boundaries) {
    const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-crash-"));
    const corpus = (version) => [
      { live: ".portal/generated", files: new Map([["evidence.json", `evidence-${version}`]]) },
      { live: "src/content/docs", files: new Map([["index.md", `content-${version}`]]) },
      { live: "public", preserveUnknown: true, files: new Map([["llms.txt", `public-${version}`]]) },
    ];
    try {
      await mkdir(path.join(root, "public"), { recursive: true });
      await writeFile(path.join(root, "public/favicon.svg"), "user-owned");
      await publishOwnedCorpus(root, corpus("one"));
      await assert.rejects(publishOwnedCorpus(root, corpus("two"), { faultAt: boundary, simulateCrash: true }), /injected publication failure/);
      await recoverOwnedCorpus(root);
      const expected = boundary === "after-commit" ? "two" : "one";
      assert.equal(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"), `evidence-${expected}`);
      assert.equal(await readFile(path.join(root, "src/content/docs/index.md"), "utf8"), `content-${expected}`);
      assert.equal(await readFile(path.join(root, "public/llms.txt"), "utf8"), `public-${expected}`);
      assert.equal(await readFile(path.join(root, "public/favicon.svg"), "utf8"), "user-owned");
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("forged publication journals fail closed without touching outside files", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-journal-"));
  try {
    await mkdir(path.join(root, ".portal"), { recursive: true });
    const sentinel = path.join(root, "sentinel.txt");
    await writeFile(sentinel, "safe");
    const forged = { schema_version: 1, phase: "prepared", stage_root: "../..", backup_root: ".portal/.publish-backup-x", groups: [
      { live: "public", stage: "../../0", backup: ".portal/.publish-backup-x/0", had_live: true },
    ] };
    await writeFile(path.join(root, ".portal/publish-transaction.json"), JSON.stringify(forged));
    await assert.rejects(recoverOwnedCorpus(root), /path|journal|stage_root/);
    assert.equal(await readFile(sentinel, "utf8"), "safe");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("an active publication lease blocks contenders without changing live output", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-lease-"));
  try {
    const now = Date.now();
    await mkdir(path.join(root, ".portal/publish.lock"), { recursive: true });
    await writeFile(path.join(root, ".portal/publish.lock/owner.json"), JSON.stringify({
      schema_version: 1, token: "a".repeat(48), created_at_ms: now, heartbeat_at_ms: now,
    }));
    await mkdir(path.join(root, "public"), { recursive: true });
    await writeFile(path.join(root, "public/sentinel.txt"), "untouched");
    await assert.rejects(publishOwnedCorpus(root, [
      { live: "public", preserveUnknown: true, files: new Map([["llms.txt", "new"]]) },
    ]), /already in progress/);
    assert.equal(await readFile(path.join(root, "public/sentinel.txt"), "utf8"), "untouched");
    await assert.rejects(readFile(path.join(root, ".portal/publish-transaction.json")), /ENOENT/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("a stale publication lease is replaced before recovery and publication", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-stale-lease-"));
  try {
    const stale = Date.now() - 31 * 60 * 1000;
    await mkdir(path.join(root, ".portal/publish.lock"), { recursive: true });
    await writeFile(path.join(root, ".portal/publish.lock/owner.json"), JSON.stringify({
      schema_version: 1, token: "b".repeat(48), created_at_ms: stale, heartbeat_at_ms: stale,
    }));
    await publishOwnedCorpus(root, [
      { live: "public", preserveUnknown: true, files: new Map([["llms.txt", "recovered"]]) },
    ]);
    assert.equal(await readFile(path.join(root, "public/llms.txt"), "utf8"), "recovered");
    await assert.rejects(readFile(path.join(root, ".portal/publish.lock/owner.json")), /ENOENT/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("unknown symlinks are refused instead of copied into a staged corpus", { skip: process.platform === "win32" }, async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-symlink-"));
  try {
    await mkdir(path.join(root, "public"), { recursive: true });
    await writeFile(path.join(root, "outside.txt"), "outside");
    const { symlink } = await import("node:fs/promises");
    await symlink(path.join(root, "outside.txt"), path.join(root, "public/linked.txt"));
    await assert.rejects(publishOwnedCorpus(root, [
      { live: "public", preserveUnknown: true, files: new Map([["llms.txt", "new"]]) },
    ]), /symlink refused/);
    assert.equal(await readFile(path.join(root, "outside.txt"), "utf8"), "outside");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("built artifact evidence is deterministic and bounded", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-artifacts-"));
  try {
    await mkdir(path.join(root, "nested/deeper"), { recursive: true });
    await writeFile(path.join(root, "z.txt"), "z");
    await writeFile(path.join(root, "nested/a.txt"), "a");
    await writeFile(path.join(root, "nested/deeper/b.txt"), "b");
    assert.deepEqual(await collectBuiltArtifacts(root), [
      { path: "dist/nested/a.txt", sha256: sha256("a") },
      { path: "dist/nested/deeper/b.txt", sha256: sha256("b") },
      { path: "dist/z.txt", sha256: sha256("z") },
    ]);
    await assert.rejects(collectBuiltArtifacts(root, { maximumFiles: 1 }), /count exceeds/);
    await assert.rejects(collectBuiltArtifacts(root, { maximumTotalBytes: 1 }), /corpus exceeds/);
    await assert.rejects(collectBuiltArtifacts(root, { maximumDepth: 1 }), /depth exceeds/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("the adapter preserves exactly one non-searchable last-good page across repeated source failures", async () => {
  const root = await portalFixture();
  try {
    const source = path.join(root, "docs/guide.md");
    await writeFile(source, "---\nid: TSK-101\ntitle: Guide\n---\n\n# Guide\n\nGrounded content.\n");
    runAdapter(root);
    await writeFile(source, "---\ntitle: broken\n");
    runAdapter(root);
    const output = path.join(root, "src/content/docs/reference/guide.md");
    const first = await readFile(output, "utf8");
    assert.equal(first.match(/Stale rendering:/g)?.length, 1);
    runAdapter(root);
    assert.equal(await readFile(output, "utf8"), first);
    const evidence = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    assert.equal(evidence.pages[0].stale, true);
    assert.equal(evidence.pages[0].searchable, false);
    assert.deepEqual(evidence.pages[0].snippets, []);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("the adapter refuses config and output symlinks without changing their targets", { skip: process.platform === "win32" }, async () => {
  const { symlink } = await import("node:fs/promises");
  const configRoot = await portalFixture();
  const outputRoot = await portalFixture();
  try {
    const outsideConfig = path.join(configRoot, "outside-config.json");
    const config = await readFile(path.join(configRoot, "portal.config.json"));
    await writeFile(outsideConfig, config);
    await rm(path.join(configRoot, "portal.config.json"));
    await symlink(outsideConfig, path.join(configRoot, "portal.config.json"));
    assert.match(runAdapter(configRoot, false).stderr, /symlink refused/);

    const sentinel = path.join(outputRoot, "sentinel.txt");
    await writeFile(sentinel, "safe");
    await mkdir(path.join(outputRoot, "public"));
    await symlink(sentinel, path.join(outputRoot, "public/llms.txt"));
    assert.match(runAdapter(outputRoot, false).stderr, /symlink refused/);
    assert.equal(await readFile(sentinel, "utf8"), "safe");
  } finally {
    await rm(configRoot, { recursive: true, force: true });
    await rm(outputRoot, { recursive: true, force: true });
  }
});

test("the adapter rejects reserved routes and non-UTF-8 source bytes", async () => {
  const reservedRoot = await portalFixture();
  const encodingRoot = await portalFixture();
  try {
    await writeFile(path.join(reservedRoot, "docs/index.md"), "# Reserved\n");
    assert.match(runAdapter(reservedRoot, false).stderr, /reserved generated route/);
    await writeFile(path.join(encodingRoot, "docs/binary.md"), Buffer.from([0xff, 0xfe, 0xfd]));
    assert.match(runAdapter(encodingRoot, false).stderr, /not valid UTF-8/);
  } finally {
    await rm(reservedRoot, { recursive: true, force: true });
    await rm(encodingRoot, { recursive: true, force: true });
  }
});

async function portalFixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-adapter-"));
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
    source_roots: ["docs"],
    exclude: [],
    layers: [
      { id: "orient", label: "Orient", description: "Orientation", paths: ["docs/product.md"] },
      { id: "system", label: "System", description: "System", prefixes: ["docs/decisions"] },
      { id: "reference", label: "Reference", description: "Reference", fallback: true },
    ],
    base: "/",
  }, null, 2)}\n`);
  return root;
}

function runAdapter(root, expectSuccess = true) {
  const result = spawnSync(process.execPath, [adapterPath], { cwd: root, encoding: "utf8" });
  if (expectSuccess) assert.equal(result.status, 0, result.stderr);
  else assert.notEqual(result.status, 0, result.stdout);
  return result;
}
