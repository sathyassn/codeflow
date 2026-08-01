import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmod, mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { amendmentHeadings, collectPageIds, compareDeterministicText, excerptFor, extractPageRelationships, parseMarkdown, referencedIds, rewriteRepositoryMarkdown, safeRelative, sha256, validateBase, validatePortalConfig, validatePrimitiveTokens, withBase } from "../scripts/lib.mjs";
import { collectBuiltArtifacts, publishOwnedCorpus, readBoundedRegularFile, recoverOwnedCorpus, withWorkflowLease } from "../scripts/publication.mjs";
import { GitSnapshot, hardenedGitEnvironment } from "../scripts/git-snapshot.mjs";

const adapterPath = fileURLToPath(new URL("../scripts/adapter.mjs", import.meta.url));
const libUrl = new URL("../scripts/lib.mjs", import.meta.url).href;

test("safe paths reject traversal and platform separators", () => {
  for (const value of ["../x", "/x", "a/../x", "a\\x", "", ".", "docs/CON.md", "docs/nul.txt", "docs/name. ", "docs/a:b.md", "docs/control\u0001.md", "docs/cafe\u0301.md"]) assert.throws(() => safeRelative(value));
  assert.equal(safeRelative("docs/café.md"), "docs/café.md");
  assert.throws(() => safeRelative(`docs/${"é".repeat(128)}`));
  assert.equal(safeRelative(`docs/${"é".repeat(127)}`), `docs/${"é".repeat(127)}`);
  assert.equal(safeRelative("docs/guide.md"), "docs/guide.md");
});

test("evidence ordering is explicit and independent of the process locale", () => {
  const values = ["ä", "z", "a", "Z"];
  assert.deepEqual([...values].sort(compareDeterministicText), ["Z", "a", "z", "ä"]);
  const program = `import { compareDeterministicText } from ${JSON.stringify(libUrl)}; console.log(JSON.stringify(${JSON.stringify(values)}.sort(compareDeterministicText)));`;
  const outputs = ["en_US.UTF-8", "sv_SE.UTF-8"].map((locale) => spawnSync(process.execPath, ["--input-type=module", "--eval", program], {
    encoding: "utf8", env: { ...process.env, LANG: locale, LC_ALL: locale },
  }));
  for (const result of outputs) assert.equal(result.status, 0, result.stderr);
  assert.equal(outputs[0].stdout, outputs[1].stdout);
});

test("bounded reads refuse oversized, growing, swapped, and symlinked files", async () => {
  const { appendFile, rename, symlink } = await import("node:fs/promises");
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-bounded-read-"));
  try {
    const target = path.join(root, "target.txt");
    const link = path.join(root, "link.txt");
    await writeFile(target, "12345");
    await assert.rejects(readBoundedRegularFile(target, 4, "fixture"), /exceeds/);
    if (process.platform !== "win32") {
      await symlink(target, link);
      await assert.rejects(readBoundedRegularFile(link, 10, "fixture"), /ELOOP|stable regular file/);
    }
    assert.equal((await readBoundedRegularFile(target, 5, "fixture")).toString(), "12345");

    await assert.rejects(readBoundedRegularFile(target, 10, "fixture", {
      afterOpen: () => appendFile(target, "67890"),
    }), /changed while|exceeds/);
    await writeFile(target, "stable");
    await assert.rejects(readBoundedRegularFile(target, 10, "fixture", {
      afterOpen: async () => {
        await rename(target, `${target}.old`);
        await writeFile(target, "replacement");
      },
    }), /changed while/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("unsafe Markdown schemes fail in the AST rewrite and amendments are structural", () => {
  const options = { sourcePath: "docs/guide.md", sourceRoutes: new Map(), base: "/", strictTargets: new Map(), mediaReferences: new Map() };
  for (const value of ["[x](javascript:alert(1))", "[x](data:text/html,x)", "[x](file:///tmp/x)", "[x](vbscript:msgbox(1))", "[x](javascript&colon;alert(1))"]) {
    assert.throws(() => rewriteRepositoryMarkdown(value, options), /unsafe Markdown URL scheme|unsupported Markdown URL/);
  }
  assert.doesNotThrow(() => rewriteRepositoryMarkdown("`[x](javascript:alert(1))`\n```md\n[x](data:x)\n```", options));
  assert.deepEqual(amendmentHeadings("## Update 2026-08-01\n\nUpdate 2026-08-02 in prose.\n```md\n## Correction 2026-08-03\n```"), ["Update 2026-08-01"]);
});

test("frontmatter parsing is strict and deterministic", () => {
  assert.deepEqual(parseMarkdown("---\ntitle: Guide\n---\nBody", "guide.md"), { frontmatter: { title: "Guide" }, body: "Body" });
  assert.throws(() => parseMarkdown("---\ntitle: Guide", "guide.md"), /unclosed/);
  assert.deepEqual(parseMarkdown("\uFEFF---\r\nid: TSK-101\r\n---\r\nBody\r\n", "guide.md"), { frontmatter: { id: "TSK-101" }, body: "Body\n" });
  assert.equal(sha256("same"), sha256("same"));
});

test("the AST rewrite escapes raw HTML while preserving code and GFM", () => {
  const source = "<!-- editorial note -->\n<script>x</script>\n\nInline `<tag>&` remains code.\n\n```html\n<div>example</div>\n```";
  const rendered = rewriteRepositoryMarkdown(source, { sourcePath: "docs/guide.md", sourceRoutes: new Map(), base: "/", strictTargets: new Map(), mediaReferences: new Map() });
  assert.doesNotMatch(rendered, /editorial note/);
  assert.match(rendered, /&lt;script&gt;/);
  assert.match(rendered, /`<tag>&`/);
  assert.match(rendered, /<div>example<\/div>/);
});

test("the AST rewrite permits external links but refuses remote images and ambiguous references", () => {
  const options = { sourcePath: "docs/guide.md", sourceRoutes: new Map(), base: "/", strictTargets: new Map(), mediaReferences: new Map() };
  assert.match(rewriteRepositoryMarkdown("[Site](https://example.com) and [mail](mailto:test@example.com)", options), /https:\/\/example\.com/);
  assert.throws(() => rewriteRepositoryMarkdown("![Remote](https://example.com/image.png)", options), /remote images/);
  assert.throws(() => rewriteRepositoryMarkdown("[Link][same]\n\n![Image][same]\n\n[same]: asset.png", options), /both a link and an image/);
  assert.match(rewriteRepositoryMarkdown("[unused]: asset.png", options), /\[unused\]: asset\.png/);
  assert.throws(() => rewriteRepositoryMarkdown("[Binary](secret.key)", options), /unsupported local media type/);
});

test("strict IDs preview outside code but code examples remain literal", () => {
  const targets = new Map([["ADR-0048", { route: "/system/decisions/adr-0048/", title: "Decision", status: "accepted", source_path: "docs/decisions/ADR-0048.md" }]]);
  const rendered = rewriteRepositoryMarkdown("ADR-0048 and `ADR-0048`", { sourcePath: "docs/guide.md", sourceRoutes: new Map(), base: "/", strictTargets: targets, mediaReferences: new Map() });
  assert.match(rendered, /<a href="\/system\/decisions\/adr-0048\/" aria-describedby="portal-preview-[^"]+">ADR-0048<\/a>/);
  assert.match(rendered, /`ADR-0048`/);
  const examples = "CAP-101\n~~~text\nCAP-102\n~~~\n<!-- CAP-103 -->\n`CAP-104`";
  assert.deepEqual(referencedIds(examples), ["CAP-101"]);
  assert.match(rewriteRepositoryMarkdown(examples, { sourcePath: "docs/guide.md", sourceRoutes: new Map(), base: "/", strictTargets: new Map([["CAP-101", { route: "/cap/", title: "Capability", source_path: "docs/capabilities.md" }]]), mediaReferences: new Map() }), /^<span class="portal-id-preview">/);
  assert.doesNotMatch(rewriteRepositoryMarkdown(examples, { sourcePath: "docs/guide.md", sourceRoutes: new Map(), base: "/", strictTargets: new Map(), mediaReferences: new Map() }), /CAP-103/);
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
  const valid = { schema_version: 1, title: "Guide", description: "Repository guide", theme: "signal", repository_url: null, repository_root: "..", release_version: null, primitive_tokens: null, source_roots: ["docs"], exclude: [], layers: [
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

test("primitive-token influence is narrow, closed, and contrast checked", () => {
  assert.deepEqual(validatePrimitiveTokens({ schema_version: 1, light: { accent: "#005f56" }, dark: { accent: "#72e2cf" } }, "signal"), { schema_version: 1, light: { accent: "#005f56" }, dark: { accent: "#72e2cf" } });
  assert.throws(() => validatePrimitiveTokens({ schema_version: 1, light: { accent: "#ffffff" }, dark: { accent: "#72e2cf" } }, "signal"), /contrast/);
  assert.throws(() => validatePrimitiveTokens({ schema_version: 1, light: { accent: "#005f56", font: "Product" }, dark: { accent: "#72e2cf" } }, "signal"), /exactly one/);
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

test("one workflow lease covers a complete build or check sequence", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-workflow-"));
  try {
    let contender = null;
    await withWorkflowLease(root, async () => {
      contender = await withWorkflowLease(root, async () => "unexpected").catch((error) => error);
      await writeFile(path.join(root, "sequence.txt"), "adapter\nastro\nevidence\n");
    });
    assert.match(String(contender), /workflow already in progress/);
    assert.equal(await readFile(path.join(root, "sequence.txt"), "utf8"), "adapter\nastro\nevidence\n");
    assert.equal(await withWorkflowLease(root, async () => "next"), "next");
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

test("preserved unknown files fail closed on growth and swap-to-secret races", { skip: process.platform === "win32" }, async () => {
  const { rename, symlink } = await import("node:fs/promises");
  for (const race of ["growth", "swap"]) {
    const root = await mkdtemp(path.join(os.tmpdir(), `codeflow-portal-preserve-${race}-`));
    try {
      await mkdir(path.join(root, "public"));
      const preserved = path.join(root, "public/keep.txt");
      const secret = path.join(root, "secret.txt");
      await writeFile(preserved, "safe");
      await writeFile(secret, "DO-NOT-PUBLISH");
      await assert.rejects(publishOwnedCorpus(root, [
        { live: "public", preserveUnknown: true, files: new Map([["llms.txt", "new"]]) },
      ], { testHooks: { afterPreservedOpen: async (source) => {
        if (source !== preserved) return;
        if (race === "growth") await writeFile(source, "safe-but-changed");
        else {
          await rename(source, `${source}.opened`);
          await symlink(secret, source);
        }
      } } }), /changed while it was being read/);
      await assert.rejects(readFile(path.join(root, "public/llms.txt")), /ENOENT/);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("symlinked corpus roots and unsafe ownership inventories fail closed", { skip: process.platform === "win32" }, async () => {
  const { symlink } = await import("node:fs/promises");
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-root-symlink-"));
  try {
    await mkdir(path.join(root, "outside"));
    await writeFile(path.join(root, "outside/sentinel.txt"), "outside");
    await symlink(path.join(root, "outside"), path.join(root, "public"));
    await assert.rejects(publishOwnedCorpus(root, [{ live: "public", preserveUnknown: true, files: new Map([["llms.txt", "new"]]) }]), /root is not a regular directory/);
    assert.equal(await readFile(path.join(root, "outside/sentinel.txt"), "utf8"), "outside");
  } finally { await rm(root, { recursive: true, force: true }); }

  for (const inventory of [
    { schema_version: 1, files: [], extra: true },
    { schema_version: 1, files: Array.from({ length: 10_001 }, (_, index) => `x-${index}`) },
  ]) {
    const owned = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-inventory-"));
    try {
      await mkdir(path.join(owned, "public"));
      await writeFile(path.join(owned, "public/.codeflow-generated.json"), JSON.stringify(inventory));
      await assert.rejects(publishOwnedCorpus(owned, [{ live: "public", preserveUnknown: true, files: new Map([["llms.txt", "new"]]) }]), /invalid generated ownership inventory/);
    } finally { await rm(owned, { recursive: true, force: true }); }
  }
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

test("Git snapshot reads scale by corpus phase and disable configured fsmonitor execution", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-git-snapshot-"));
  try {
    await mkdir(path.join(root, "docs"));
    for (let index = 0; index < 256; index += 1) {
      await writeFile(path.join(root, `docs/page-${String(index).padStart(3, "0")}.md`), `# Page ${index}\n`);
    }
    git(root, ["init", "-q"]);
    git(root, ["config", "user.email", "portal-tests@codeflow.invalid"]);
    git(root, ["config", "user.name", "CodeFlow portal tests"]);
    commitFixture(root, "large snapshot fixture");
    const sentinel = path.join(root, "fsmonitor-ran");
    const monitor = path.join(root, "monitor.sh");
    await writeFile(monitor, `#!/bin/sh\nprintf ran > "${sentinel}"\n`);
    await chmod(monitor, 0o755);
    git(root, ["config", "core.fsmonitor", monitor]);
    const commands = [];
    const snapshot = new GitSnapshot(root, { onCommand: (args) => commands.push(args[0]) });
    const commit = snapshot.resolveHead();
    snapshot.loadInventory(commit);
    const records = snapshot.requireDirectory("docs", "source_root");
    const blobs = snapshot.readBlobs(records, { perObjectBytes: 1024, totalBytes: 512 * 1024, label: "fixture corpus" });
    snapshot.assertClean(["docs"]);
    assert.equal(blobs.size, 256);
    assert.deepEqual(commands, ["rev-parse", "ls-tree", "cat-file", "status"]);
    await assert.rejects(readFile(sentinel), /ENOENT/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("Git snapshot child environments exclude inherited credentials and injection controls", () => {
  const environment = hardenedGitEnvironment({
    PATH: "/safe/bin",
    TMPDIR: "/safe/tmp",
    AWS_SECRET_ACCESS_KEY: "aws-canary",
    OPENAI_API_KEY: "openai-canary",
    ANTHROPIC_API_KEY: "anthropic-canary",
    GIT_CONFIG_COUNT: "1",
    GIT_CONFIG_KEY_0: "core.fsmonitor",
    GIT_CONFIG_VALUE_0: "/tmp/attacker",
    LD_PRELOAD: "/tmp/inject.so",
  });
  assert.equal(environment.PATH, "/safe/bin");
  assert.equal(environment.TMPDIR, "/safe/tmp");
  for (const key of ["AWS_SECRET_ACCESS_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY", "GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0", "LD_PRELOAD"]) {
    assert.equal(key in environment, false, `${key} escaped the child-environment allowlist`);
  }
});

test("the adapter emits one bounded non-searchable current-source stub without ancestor content", async () => {
  const root = await portalFixture();
  try {
    const source = path.join(root, "docs/guide.md");
    await writeFile(source, "---\nid: TSK-101\ntitle: Guide\ndepends_on: [TSK-102]\n---\n\n# Guide\n\nGrounded content references TSK-102.\n");
    await writeFile(path.join(root, "docs/target.md"), "---\nid: TSK-102\ntitle: Current target\n---\n\n# Target\n\nThis current page references TSK-101.\n");
    commitFixture(root, "add valid guide");
    runAdapter(root);
    const prior = await readFile(path.join(root, "src/content/docs/reference/guide.md"), "utf8");
    assert.match(prior, /Grounded content/);
    await writeFile(source, "---\ntitle: broken\n");
    commitFixture(root, "break guide");
    runAdapter(root);
    const output = path.join(root, "src/content/docs/reference/guide.md");
    const first = await readFile(output, "utf8");
    assert.equal(first.match(/Source unavailable:/g)?.length, 1);
    assert.doesNotMatch(first, /Grounded content|Current target/);
    runAdapter(root);
    const second = await readFile(output, "utf8");
    assert.equal(second, first);
    const evidence = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    const stale = evidence.pages.find((page) => page.source_path === "docs/guide.md");
    const target = evidence.pages.find((page) => page.source_path === "docs/target.md");
    assert.equal(stale.stale, true);
    assert.equal(stale.searchable, false);
    assert.deepEqual(stale.snippets, []);
    assert.deepEqual(stale.ids, []);
    assert.deepEqual(stale.relationships, []);
    assert.equal(typeof stale.stale_reason, "string");
    assert.equal("last_good_commit" in stale, false);
    assert.equal("last_good_source_sha256" in stale, false);
    assert.deepEqual(target.backlinks, []);
    const targetOutput = await readFile(path.join(root, "src/content/docs/reference/target.md"), "utf8");
    assert.doesNotMatch(targetOutput, /stale — excluded from the current graph/);
    assert.doesNotMatch(targetOutput, /### Inverse links/);
    const landing = await readFile(path.join(root, "src/content/docs/reference/index.md"), "utf8");
    assert.doesNotMatch(landing, /Guide/);
    const llms = await readFile(path.join(root, "public/llms.txt"), "utf8");
    assert.doesNotMatch(llms, /reference\/guide/);
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
    assert.match(runAdapter(configRoot, false).stderr, /symlink refused|must match HEAD exactly/);

    await writeFile(path.join(outputRoot, "docs/guide.md"), "# Guide\n");
    commitFixture(outputRoot, "add output fixture source");
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

test("the adapter rejects reserved routes and stubs non-UTF-8 source bytes", async () => {
  const reservedRoot = await portalFixture();
  const encodingRoot = await portalFixture();
  try {
    await writeFile(path.join(reservedRoot, "docs/index.md"), "# Reserved\n");
    commitFixture(reservedRoot, "reserved source");
    assert.match(runAdapter(reservedRoot, false).stderr, /reserved generated route/);
    await writeFile(path.join(encodingRoot, "docs/binary.md"), Buffer.from([0xff, 0xfe, 0xfd]));
    commitFixture(encodingRoot, "binary source");
    runAdapter(encodingRoot);
    const stub = await readFile(path.join(encodingRoot, "src/content/docs/reference/binary.md"), "utf8");
    assert.match(stub, /Source unavailable:/);
    assert.doesNotMatch(stub, /data-pagefind-body|data-codeflow-search-root/);
  } finally {
    await rm(reservedRoot, { recursive: true, force: true });
    await rm(encodingRoot, { recursive: true, force: true });
  }
});

test("the adapter refuses dirty snapshot states and dangerous source links", async () => {
  for (const state of ["modified", "staged", "untracked", "deleted"]) {
    const root = await portalFixture();
    try {
      const source = path.join(root, "docs/guide.md");
      await writeFile(source, "# Guide\n");
      commitFixture(root, "add guide");
      if (state === "modified") await writeFile(source, "# Modified\n");
      if (state === "staged") { await writeFile(source, "# Staged\n"); git(root, ["add", "docs/guide.md"]); }
      if (state === "untracked") await writeFile(path.join(root, "docs/new.md"), "# New\n");
      if (state === "deleted") await rm(source);
      assert.match(runAdapter(root, false).stderr, /must match HEAD exactly/);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
  const unsafe = await portalFixture();
  try {
    await writeFile(path.join(unsafe, "docs/guide.md"), "# Guide\n\n[unsafe](javascript:alert(1))\n");
    commitFixture(unsafe, "unsafe link");
    assert.match(runAdapter(unsafe, false).stderr, /unsafe Markdown URL scheme/);
  } finally { await rm(unsafe, { recursive: true, force: true }); }
});

test("committed blobs defeat assume-unchanged and skip-worktree source masking", async () => {
  for (const flag of ["--assume-unchanged", "--skip-worktree"]) {
    const root = await portalFixture();
    try {
      const source = path.join(root, "docs/guide.md");
      await writeFile(source, "# Committed guide\n\nCommitted truth.\n");
      commitFixture(root, "add committed guide");
      git(root, ["update-index", flag, "docs/guide.md"]);
      await writeFile(source, "# Hidden worktree edit\n\nMust not be published.\n");
      runAdapter(root);
      const output = await readFile(path.join(root, "src/content/docs/reference/guide.md"), "utf8");
      assert.match(output, /Committed truth/);
      assert.doesNotMatch(output, /Hidden worktree edit/);
    } finally { await rm(root, { recursive: true, force: true }); }
  }

  const runtime = await portalFixture();
  try {
    await writeFile(path.join(runtime, "docs/guide.md"), "# Guide\n");
    commitFixture(runtime, "add runtime fixture source");
    git(runtime, ["update-index", "--assume-unchanged", "portal.config.json"]);
    const config = JSON.parse(await readFile(path.join(runtime, "portal.config.json"), "utf8"));
    config.title = "Hidden runtime mutation";
    await writeFile(path.join(runtime, "portal.config.json"), `${JSON.stringify(config)}\n`);
    assert.match(runAdapter(runtime, false).stderr, /runtime input does not match/);
  } finally { await rm(runtime, { recursive: true, force: true }); }
});

test("a source-root pathspec does not capture a similarly prefixed ignored directory", async () => {
  const root = await portalFixture();
  try {
    await writeFile(path.join(root, "docs/guide.md"), "# Guide\n");
    await writeFile(path.join(root, ".gitignore"), "docs-cache/\n");
    commitFixture(root, "add literal source root");
    await mkdir(path.join(root, "docs-cache"));
    await writeFile(path.join(root, "docs-cache/generated.md"), "# Not a source\n");
    runAdapter(root);
    const evidence = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    assert.deepEqual(evidence.pages.map((page) => page.source_path), ["docs/guide.md"]);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("source roots are committed directories with publishable Markdown", async () => {
  for (const scenario of ["file", "no-markdown"]) {
    const root = await portalFixture();
    try {
      const configPath = path.join(root, "portal.config.json");
      const config = JSON.parse(await readFile(configPath, "utf8"));
      if (scenario === "file") {
        await writeFile(path.join(root, "README.md"), "# Root file\n");
        config.source_roots = ["README.md"];
      } else {
        await writeFile(path.join(root, "docs/notes.txt"), "not Markdown\n");
      }
      await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
      commitFixture(root, `source root ${scenario}`);
      assert.match(runAdapter(root, false).stderr, scenario === "file" ? /committed directory, not a file/ : /no publishable Markdown/);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("monorepo source roots produce one deterministic global-to-area route graph", async () => {
  const root = await portalFixture();
  try {
    await mkdir(path.join(root, "apps/web/docs"), { recursive: true });
    await mkdir(path.join(root, "services/api/docs"), { recursive: true });
    await writeFile(path.join(root, "docs/product.md"), "# Product\n");
    await writeFile(path.join(root, "apps/web/docs/journey.md"), "# Web journey\n");
    await writeFile(path.join(root, "services/api/docs/contract.md"), "# API contract\n");
    const configPath = path.join(root, "portal.config.json");
    const config = JSON.parse(await readFile(configPath, "utf8"));
    config.source_roots = ["docs", "apps/web/docs", "services/api/docs"];
    config.layers = [
      { id: "orient", label: "Orient", description: "Global", paths: ["docs/product.md"] },
      { id: "web", label: "Web", description: "Web surface", prefixes: ["apps/web/docs"] },
      { id: "api", label: "API", description: "API area", prefixes: ["services/api/docs"] },
      { id: "reference", label: "Reference", description: "Reference", fallback: true },
    ];
    await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
    commitFixture(root, "monorepo route fixture");
    runAdapter(root);
    const evidence = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    assert.deepEqual(evidence.pages.map(({ source_path, route }) => [source_path, route]), [
      ["apps/web/docs/journey.md", "web/apps/web/docs/journey"],
      ["docs/product.md", "orient/product"],
      ["services/api/docs/contract.md", "api/services/api/docs/contract"],
    ]);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("generated strict-ID previews are source-grounded and keyboard-native", async () => {
  const root = await portalFixture();
  try {
    await mkdir(path.join(root, "docs/decisions"), { recursive: true });
    await writeFile(path.join(root, "docs/guide.md"), "# Guide\n\nSee ADR-0001.\n");
    await writeFile(path.join(root, "docs/decisions/ADR-0001.md"), "---\nid: ADR-0001\ntitle: Keep source truth\nstatus: accepted\n---\n\n# Decision\n");
    commitFixture(root, "add linked records");
    runAdapter(root);
    const rendered = await readFile(path.join(root, "src/content/docs/reference/guide.md"), "utf8");
    assert.match(rendered, /<span class="portal-id-preview"><a href="\/system\/decisions\/ADR-0001\/" aria-describedby="portal-preview-[^"]+">ADR-0001<\/a>/);
    assert.match(rendered, /role="tooltip"><strong>Keep source truth<\/strong><span>Status: accepted<\/span><span>Source: <code>docs\/decisions\/ADR-0001.md<\/code><\/span>/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("pinned source links use known provider routes and fall back visibly", async () => {
  for (const [repositoryUrl, expected] of [
    ["https://github.com/example/repository", "/blob/"],
    ["https://gitlab.com/example/repository", "/-/blob/"],
    ["https://bitbucket.org/example/repository", "/src/"],
    ["https://source.example/repository", null],
  ]) {
    const root = await portalFixture();
    try {
      const configPath = path.join(root, "portal.config.json");
      const config = JSON.parse(await readFile(configPath, "utf8"));
      config.repository_url = repositoryUrl;
      config.exclude = ["docs/excluded.md"];
      await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
      await writeFile(path.join(root, "docs/excluded.md"), "# Excluded but committed\n");
      await writeFile(path.join(root, "docs/guide (one).md"), "# Guide\n\n[Excluded source](excluded.md)\n\n[Excluded reference][excluded]\n\n[excluded]: excluded.md\n");
      commitFixture(root, "configure source provider");
      const commit = git(root, ["rev-parse", "HEAD"]).trim();
      runAdapter(root);
      const output = await readFile(path.join(root, "src/content/docs/reference/guide%20%28one%29.md"), "utf8");
      assert.match(output, /<code>docs\/guide \(one\)\.md<\/code> at <code>[a-f0-9]{12}<\/code>/);
      if (expected === null) {
        assert.doesNotMatch(output, /<a[^>]+>Excluded source<\/a>/);
        assert.match(output, /Excluded source \(<code>docs\/excluded\.md<\/code> at <code>[a-f0-9]{12}<\/code>\)/);
        assert.match(output, /Excluded reference \(<code>docs\/excluded\.md<\/code> at <code>[a-f0-9]{12}<\/code>\)/);
      } else {
        assert.match(output, new RegExp(`${expected.replaceAll("/", "\\/")}${commit}\\/docs\\/guide%20%28one%29\\.md`));
        assert.match(output, new RegExp(`${expected.replaceAll("/", "\\/")}${commit}\\/docs\\/excluded\\.md`));
        assert.match(output, /Excluded reference/);
      }
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("source and configuration metadata cannot inject active generated Markdown", async () => {
  const root = await portalFixture();
  try {
    const configPath = path.join(root, "portal.config.json");
    const config = JSON.parse(await readFile(configPath, "utf8"));
    config.layers[2].label = "Reference <script>globalThis.pwned=1</script>";
    config.layers[2].description = "![probe](https://attacker.invalid/layer.png)";
    await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
    await writeFile(path.join(root, "docs/guide.md"), `---\nid: TSK-0101\ntitle: ${JSON.stringify("Guide <script>globalThis.pwned=2</script>")}\nstatus: ${JSON.stringify("![probe](https://attacker.invalid/status.png)")}\n---\n\n# Safe body\n`);
    commitFixture(root, "add hostile metadata");
    runAdapter(root);
    const landing = await readFile(path.join(root, "src/content/docs/reference/index.md"), "utf8");
    const landingBody = landing.slice(landing.indexOf("---", 4) + 3);
    assert.doesNotMatch(landingBody, /<script>|!\[probe\]\(https:/);
    assert.match(landingBody, /&lt;script&gt;/);
    assert.equal(landingBody.includes("https&#58;//attacker\\.invalid"), true);
    const page = await readFile(path.join(root, "src/content/docs/reference/guide.md"), "utf8");
    const body = page.slice(page.indexOf("---", 4) + 3);
    assert.doesNotMatch(body, /<script>|!\[probe\]\(https:/);
    assert.equal(body.includes("https&#58;//attacker\\.invalid"), true);

    await writeFile(path.join(root, "docs/guide.md"), "---\ntitle: [![probe](https://attacker.invalid/stale.png)\n---\n\n# Broken\n");
    commitFixture(root, "break hostile source");
    runAdapter(root);
    const stale = await readFile(path.join(root, "src/content/docs/reference/guide.md"), "utf8");
    const staleBody = stale.slice(stale.indexOf("---", 4) + 3);
    assert.doesNotMatch(staleBody, /<script>|!\[probe\]\(https:|data-pagefind-body|data-codeflow-search-root/);
    assert.match(staleBody, /Source unavailable:/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("stale output never republishes authenticated ancestors or forged prior generated bytes", async () => {
  const root = await portalFixture();
  try {
    const source = path.join(root, "docs/guide.md");
    await writeFile(source, "---\nid: TSK-0102\ntitle: Authenticated guide\n---\n\n# Safe ancestor\n\nGrounded content.\n");
    commitFixture(root, "add valid ancestor");
    runAdapter(root);
    const output = path.join(root, "src/content/docs/reference/guide.md");
    await writeFile(output, "<script>globalThis.pwned=1</script><img src=https://attacker.invalid/pixel.png>");
    const evidencePath = path.join(root, ".portal/generated/evidence.json");
    const forged = JSON.parse(await readFile(evidencePath, "utf8"));
    forged.pages[0].built_from_commit = "f".repeat(40);
    forged.pages[0].source_sha256 = sha256("forged");
    forged.pages[0].output_markdown_sha256 = sha256(await readFile(output));
    await writeFile(evidencePath, JSON.stringify(forged));

    await writeFile(source, "---\ntitle: [broken\n---\n");
    commitFixture(root, "break current source");
    runAdapter(root);
    const stale = await readFile(output, "utf8");
    assert.match(stale, /Source unavailable:/);
    assert.match(stale, /the previous version of this page is not shown/);
    assert.doesNotMatch(stale, /# Safe ancestor|Grounded content|globalThis\.pwned|<img|attacker\.invalid|built_from_commit=f{40}|codeflow-last-good-provenance/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("the AST adapter rewrites cross-layer documents and copies bounded committed media", async () => {
  const root = await portalFixture();
  try {
    await mkdir(path.join(root, "docs/decisions"), { recursive: true });
    await mkdir(path.join(root, "docs/media"), { recursive: true });
    await writeFile(path.join(root, "docs/decisions/ADR-0001.md"), "---\nid: ADR-0001\ntitle: Decision\n---\n\n# Decision\n");
    const png = pngHeader(1, 1);
    await writeFile(path.join(root, "docs/media/flow.png"), png);
    await writeFile(path.join(root, "docs/guide.md"), "# Guide\n\n[Decision](decisions/ADR-0001.md#outcome)\n\n![Flow](media/flow.png)\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n`[literal](missing.md)`\n");
    commitFixture(root, "add AST fixture");
    runAdapter(root);
    const rendered = await readFile(path.join(root, "src/content/docs/reference/guide.md"), "utf8");
    assert.match(rendered, /\[Decision\]\(\/system\/decisions\/ADR-0001\/#outcome\)/);
    const mediaRoute = `media/${sha256("docs/media/flow.png").slice(0, 16)}-flow.png`;
    assert.match(rendered, new RegExp(`!\\[Flow\\]\\(\\/${mediaRoute.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\)`));
    assert.deepEqual(await readFile(path.join(root, "public", mediaRoute)), png);
    assert.match(rendered, /\| A \| B \|/);
    assert.match(rendered, /`\[literal\]\(missing\.md\)`/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("the adapter rejects raster truncation, type mismatch, and dimension bombs", async () => {
  for (const [name, bytes, expected] of [
    ["truncated.png", Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), /truncated or invalid/],
    ["mismatch.jpg", pngHeader(1, 1), /do not match the approved raster type/],
    ["bomb.png", pngHeader(8192, 8192), /dimensions exceed the portal limit/],
  ]) {
    const root = await portalFixture();
    try {
      await mkdir(path.join(root, "docs/media"));
      await writeFile(path.join(root, `docs/media/${name}`), bytes);
      await writeFile(path.join(root, "docs/guide.md"), `# Guide\n\n![Fixture](media/${name})\n`);
      commitFixture(root, `add ${name}`);
      assert.match(runAdapter(root, false).stderr, expected);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("the AST adapter fails broken documents and repository traversal", async () => {
  for (const link of ["missing.md", "../../outside.png"]) {
    const root = await portalFixture();
    try {
      await writeFile(path.join(root, "docs/guide.md"), `# Guide\n\n[Broken](${link})\n`);
      commitFixture(root, "add broken link");
      assert.match(runAdapter(root, false).stderr, /does not exist|stay beneath/);
    } finally { await rm(root, { recursive: true, force: true }); }
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
    release_version: null,
    primitive_tokens: null,
    source_roots: ["docs"],
    exclude: [],
    layers: [
      { id: "orient", label: "Orient", description: "Orientation", paths: ["docs/product.md"] },
      { id: "system", label: "System", description: "System", prefixes: ["docs/decisions"] },
      { id: "reference", label: "Reference", description: "Reference", fallback: true },
    ],
    base: "/",
  }, null, 2)}\n`);
  git(root, ["init", "-q"]);
  git(root, ["config", "user.email", "portal-tests@codeflow.invalid"]);
  git(root, ["config", "user.name", "CodeFlow portal tests"]);
  commitFixture(root, "initialize fixture");
  return root;
}

function commitFixture(root, message, allowEmpty = false) {
  git(root, ["add", "-A"]);
  git(root, ["commit", "-q", ...(allowEmpty ? ["--allow-empty"] : []), "-m", message]);
}

function git(root, args) {
  const result = spawnSync("git", ["-C", root, ...args], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout;
}

function runAdapter(root, expectSuccess = true) {
  const result = spawnSync(process.execPath, [adapterPath], { cwd: root, encoding: "utf8" });
  if (expectSuccess) assert.equal(result.status, 0, result.stderr);
  else assert.notEqual(result.status, 0, result.stdout);
  return result;
}

function pngHeader(width, height) {
  const bytes = Buffer.alloc(24);
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).copy(bytes);
  bytes.writeUInt32BE(13, 8);
  bytes.write("IHDR", 12, "ascii");
  bytes.writeUInt32BE(width, 16);
  bytes.writeUInt32BE(height, 20);
  return bytes;
}
