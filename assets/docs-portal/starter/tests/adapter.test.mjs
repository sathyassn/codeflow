import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { EventEmitter } from "node:events";
import { chmod, cp, link, lstat, mkdtemp, mkdir, readFile, readdir, rename, rm, symlink, utimes, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { amendmentHeadings, checkoutEquivalentBytes, collectPageIds, compareDeterministicText, decorateAltitude, excerptFor, extractPageRelationships, headingAnchors, localRouteFor, parseMarkdown, pinnedSourceUrl, recoverUnavailableIds, referencedIds, renderStageFences, rewriteRepositoryMarkdown, safeRelative, sha256, stripLeadingTitleHeading, titleFor, validateBase, validatePageMetadata, validatePortalConfig, validatePrimitiveTokens, validRepositoryUrl, withBase } from "../scripts/lib.mjs";
import { assertExpectedPageArtifacts, assertToolOutputRoots, collectBuiltArtifacts, hashBoundedRegularFile, publishOwnedCorpus, readBoundedRegularFile, recoverOwnedCorpus, withWorkflowLease } from "../scripts/publication.mjs";
import { boundedPathspecBatches, GitSnapshot, hardenedGitEnvironment } from "../scripts/git-snapshot.mjs";
import { assertEvidenceEnvelope, assertEvidencePageLimits, EVIDENCE_LIMITS } from "../scripts/limits.mjs";
import { assertArtifactClaims, discoverSurfaceRoutes, meaningfulRuntimeDiagnostics } from "../scripts/browser-verify.mjs";
import { assertReviewedInstallScripts, REVIEWED_IGNORED_LIFECYCLE_SCRIPTS } from "../scripts/install-dependencies.mjs";
import { stopChild } from "../scripts/child-lifecycle.mjs";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";
import { GENERATOR, assertGeneratorIdentity } from "../scripts/generator.mjs";

const adapterPath = fileURLToPath(new URL("../scripts/adapter.mjs", import.meta.url));
const starterRoot = fileURLToPath(new URL("..", import.meta.url));
const libUrl = new URL("../scripts/lib.mjs", import.meta.url).href;

test("actual generator identity is bounded, closed and release-pinned", async () => {
  const packageInfo = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"));
  assert.deepEqual(GENERATOR, { name: packageInfo.name, version: packageInfo.version });
  const lock = JSON.parse(await readFile(new URL("../package-lock.json", import.meta.url), "utf8"));
  for (const entry of [lock, lock.packages[""]]) {
    assert.deepEqual(GENERATOR, { name: entry.name, version: entry.version });
  }
  assert.doesNotThrow(() => assertGeneratorIdentity({ ...GENERATOR }));
  for (const value of [null, [], {}, { ...GENERATOR, extra: true },
    { ...GENERATOR, name: "@project/fork" }, { ...GENERATOR, version: "0.0.0" },
    { ...GENERATOR, name: " " }, { ...GENERATOR, version: 2 },
    { ...GENERATOR, name: "é".repeat(65) }]) {
    assert.throws(() => assertGeneratorIdentity(value), /generator does not match/);
  }
});

test("an actual fork identity validates its own UTF-8 bounds before module use", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-generator-identity-"));
  try {
    const source = await readFile(new URL("../scripts/generator.mjs", import.meta.url), "utf8");
    const original = `Object.freeze({ name: "${GENERATOR.name}", version: "${GENERATOR.version}" })`;
    for (const [index, [identity, accepted]] of [
      [{ name: " ", version: "2" }, false],
      [{ name: "é".repeat(65), version: "2" }, false],
      [{ name: "fork", version: "x".repeat(129) }, false],
      [{ name: "é".repeat(64), version: "x".repeat(128) }, true],
    ].entries()) {
      const changed = source.replace(original, `Object.freeze(${JSON.stringify(identity)})`);
      assert.notEqual(changed, source);
      const modulePath = path.join(root, `generator-${index}.mjs`);
      await writeFile(modulePath, changed);
      const result = spawnSync(process.execPath, [modulePath], { encoding: "utf8" });
      assert.equal(result.status === 0, accepted, result.stderr);
      if (!accepted) assert.match(result.stderr, /generator does not match this runtime/);
    }
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("a renamed runtime emits its own identity and rejects substituted evidence", async () => {
  const root = await selfContainedPortalFixture();
  try {
    const identity = {
      name: GENERATOR.name === "@example/project-guide" ? "@example/alternate-guide" : "@example/project-guide",
      version: "7.0.0",
    };
    assert.notDeepEqual(identity, GENERATOR);
    const generatorPath = path.join(root, "scripts/generator.mjs");
    const generatorSource = await readFile(generatorPath, "utf8");
    await writeFile(generatorPath, generatorSource.replace(
      `name: "${GENERATOR.name}", version: "${GENERATOR.version}"`,
      `name: "${identity.name}", version: "${identity.version}"`,
    ));
    const packagePath = path.join(root, "package.json");
    const packageInfo = JSON.parse(await readFile(packagePath, "utf8"));
    Object.assign(packageInfo, identity);
    await writeFile(packagePath, `${JSON.stringify(packageInfo, null, 2)}\n`);
    commitFixture(root, "version project generator");
    runLocalAdapter(root);
    const evidencePath = path.join(root, ".portal/generated/evidence.json");
    const evidence = JSON.parse(await readFile(evidencePath, "utf8"));
    assert.equal(evidence.schema_version, 1);
    assert.deepEqual(evidence.generator, identity);
    evidence.generator = { ...GENERATOR };
    const forged = `${JSON.stringify(evidence, null, 2)}\n`;
    await writeFile(evidencePath, forged);
    const result = spawnSync(process.execPath, [path.join(root, "scripts/evidence.mjs")], { cwd: root, encoding: "utf8" });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /generator does not match this runtime/);
    assert.equal(await readFile(evidencePath, "utf8"), forged);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("managed and forked runtimes render their version in pages, stale stubs and the landing", async () => {
  for (const [version, renderedVersion] of [
    ["2.0.0", "2.0.0"],
    ["7.0.0", "7.0.0"],
    ['7--><img src=x>&"\r\n', '7--&gt;&lt;img src=x&gt;&amp;&quot;&#13;&#10;'],
  ]) {
    const root = await selfContainedPortalFixture();
    try {
      const generatorPath = path.join(root, "scripts/generator.mjs");
      const source = await readFile(generatorPath, "utf8");
      const currentVersion = `version: ${JSON.stringify(GENERATOR.version)}`;
      assert(source.includes(currentVersion));
      await writeFile(generatorPath, source.replace(currentVersion, `version: ${JSON.stringify(version)}`));
      await writeFile(path.join(root, "docs/broken.md"), "---\ntitle: [broken\n---\n");
      commitFixture(root, "exercise rendered generator provenance");
      runLocalAdapter(root);
      const evidence = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
      assert.equal(evidence.generator.version, version);
      assert.equal(evidence.pages.length, 2);
      assert.equal(evidence.pages.filter((page) => page.stale).length, 1);
      for (const page of evidence.pages) {
        const rendered = await readFile(path.join(root, "src/content/docs", `${page.route}.md`), "utf8");
        const marker = `<!-- codeflow-page-provenance source_sha256=${page.source_sha256} built_from_commit=${evidence.repository.commit} portal_version=${renderedVersion} release_version=none -->`;
        assert(rendered.split("\n").includes(marker), rendered);
        assert(rendered.includes(` · portal <code>${renderedVersion}</code>`), rendered);
        assert(!rendered.includes("<img"), rendered);
      }
      const landing = await readFile(path.join(root, "src/content/docs/index.md"), "utf8");
      assert(landing.includes(` · portal <code>${renderedVersion}</code>`), landing);
      assert(!landing.includes("<img"), landing);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("semantic route fixtures stay in parity with the Rust validator", async () => {
  const fixture = JSON.parse(await readFile(new URL("./fixtures/route-contract.json", import.meta.url), "utf8"));
  for (const item of fixture.accepted) {
    assert.equal(localRouteFor(item.source_path, item.source_roots), item.local_route, item.source_path);
  }
  for (const item of fixture.rejected) {
    assert.throws(() => localRouteFor(item.source_path, item.source_roots), new RegExp(item.error), item.source_path);
  }
});

test("safe paths reject traversal and platform separators", () => {
  for (const value of ["../x", "/x", "a/../x", "a\\x", "", ".", "docs/CON.md", "docs/nul.txt", "docs/name. ", "docs/a:b.md", "docs/control\u0001.md", "docs/cafe\u0301.md"]) assert.throws(() => safeRelative(value));
  assert.equal(safeRelative("docs/café.md"), "docs/café.md");
  assert.throws(() => safeRelative(`docs/${"é".repeat(128)}`));
  assert.equal(safeRelative(`docs/${"é".repeat(127)}`), `docs/${"é".repeat(127)}`);
  assert.equal(safeRelative("docs/guide.md"), "docs/guide.md");
});

test("checkout byte comparison permits only reversible text newlines", () => {
  assert.equal(checkoutEquivalentBytes(Buffer.from("alpha\nbeta\n"), Buffer.from("alpha\r\nbeta\r\n")), true);
  assert.equal(checkoutEquivalentBytes(Buffer.from("alpha\nbeta\n"), Buffer.from("alpha\nbeta changed\n")), false);
  assert.equal(checkoutEquivalentBytes(Buffer.from("alpha\nbeta\n"), Buffer.from("alpha\rbeta\n")), false);
  assert.equal(checkoutEquivalentBytes(Buffer.from([0xff, 0x00]), Buffer.from([0xff, 0x00])), true);
  assert.equal(checkoutEquivalentBytes(Buffer.from([0xff, 0x00]), Buffer.from([0xfe, 0x00])), false);
});

test("stale identity recovery matches the verifier grammar and ordering", () => {
  assert.deepEqual(recoverUnavailableIds("\uFEFF---\r\nid: 'TSK-101' # note\r\n", "docs/guide.md"), ["TSK-101"]);
  assert.deepEqual(recoverUnavailableIds('id: "TSK-101\'\n', "docs/guide.md"), []);
  assert.deepEqual(recoverUnavailableIds("id: CAP-001-002\n", "docs/guide.md"), []);
  assert.deepEqual(recoverUnavailableIds("id: TSK-200\nid: TSK-100\n", "docs/guide.md"), ["TSK-200"]);
  assert.deepEqual(recoverUnavailableIds("id: CAP-002\nid: CAP-001\n", "docs/capabilities.md"), ["CAP-001", "CAP-002"]);
  assert.deepEqual(recoverUnavailableIds("id: TSK-999\n", "tasks/TSK-123.md"), ["TSK-123"]);
});

test("producer evidence limits reject every verifier boundary at N plus one", () => {
  const page = {
    ids: Array(EVIDENCE_LIMITS.idsPerPage).fill("CAP-001"),
    unavailable_ids: Array(EVIDENCE_LIMITS.idsPerPage).fill("CAP-002"),
    relationships: Array(EVIDENCE_LIMITS.relationshipsPerPage).fill({}),
    backlinks: Array(EVIDENCE_LIMITS.backlinksPerPage).fill({}),
    snippets: Array(EVIDENCE_LIMITS.snippetsPerPage).fill({}),
  };
  assert.doesNotThrow(() => assertEvidencePageLimits(page));
  for (const field of ["ids", "unavailable_ids", "relationships", "backlinks", "snippets"]) {
    const over = { ...page, [field]: [...page[field], {}] };
    assert.throws(() => assertEvidencePageLimits(over), new RegExp(`${field} count exceeds`));
  }
  assert.doesNotThrow(() => assertEvidenceEnvelope(Array(EVIDENCE_LIMITS.pages), "{}"));
  assert.throws(() => assertEvidenceEnvelope(Array(EVIDENCE_LIMITS.pages + 1), "{}"), /page count exceeds/);
  assert.doesNotThrow(() => assertEvidenceEnvelope([], "x".repeat(EVIDENCE_LIMITS.manifestBytes)));
  assert.throws(() => assertEvidenceEnvelope([], "x".repeat(EVIDENCE_LIMITS.manifestBytes + 1)), /manifest exceeds/);
});

test("built page identity is case-sensitive", () => {
  const pages = [{ route: "records/EPC-001" }];
  assert.doesNotThrow(() => assertExpectedPageArtifacts(pages, [{ path: "dist/records/EPC-001/index.html" }]));
  assert.throws(() => assertExpectedPageArtifacts(pages, [{ path: "dist/records/epc-001/index.html" }]), /exact page route/);
});

test("browser evidence binds actual dist bytes and known source providers", () => {
  const claim = [{ path: "dist/index.html", sha256: "a".repeat(64) }];
  assert.doesNotThrow(() => assertArtifactClaims(claim, structuredClone(claim), "fixture"));
  assert.throws(() => assertArtifactClaims(claim, [{ ...claim[0], sha256: "b".repeat(64) }], "fixture"), /dist bytes/);
  const commit = "c".repeat(40);
  const source = "docs/Mixed Case + café.md";
  assert.equal(pinnedSourceUrl("https://github.com/acme/repo.git", commit, source), `https://github.com/acme/repo/blob/${commit}/docs/Mixed%20Case%20%2B%20caf%C3%A9.md`);
  assert.equal(pinnedSourceUrl("https://gitlab.com/acme/repo", commit, source), `https://gitlab.com/acme/repo/-/blob/${commit}/docs/Mixed%20Case%20%2B%20caf%C3%A9.md`);
  assert.equal(pinnedSourceUrl("https://bitbucket.org/acme/repo", commit, source), `https://bitbucket.org/acme/repo/src/${commit}/docs/Mixed%20Case%20%2B%20caf%C3%A9.md`);
  assert.equal(pinnedSourceUrl("https://git.example.com/acme/repo", commit, source), null);
});

test("browser surface discovery scans beyond the first 64 pages", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-browser-surfaces-"));
  try {
    const pages = [];
    for (let index = 0; index < 70; index += 1) {
      const relative = `generated/page-${index}.md`;
      const text = index === 66 || index === 67 ? "# Page\n\n<div class=\"portal-altitude-tabs\" role=\"tablist\"></div>\n" : index === 68 ? "# Page\n\n## Deep target\n" : index === 69 ? "# Page\n\n<span class=\"portal-id-preview\">CAP-001</span>\n" : "# Page\n";
      await mkdir(path.dirname(path.join(root, relative)), { recursive: true });
      await writeFile(path.join(root, relative), text);
      pages.push({ stale: false, route: `reference/page-${index}`, output_markdown: relative, output_markdown_sha256: sha256(text) });
    }
    assert.deepEqual(await discoverSurfaceRoutes(pages, root), {
      deepLink: "reference/page-68",
      strictPreview: "reference/page-69",
      altitudeTabs: ["reference/page-66", "reference/page-67"],
      layerSamples: { reference: ["reference/page-0", "reference/page-69"] },
    });
    pages[69].output_markdown_sha256 = "0".repeat(64);
    await assert.rejects(discoverSurfaceRoutes(pages, root), /hash mismatch/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("browser diagnostics ignore only navigation-cancelled local loads", () => {
  const paired = meaningfulRuntimeDiagnostics({
    console: [],
    page: ["TypeError: Importing a module script failed."],
    request: ["GET http://127.0.0.1/_astro/ui.js: cancelled", "GET http://127.0.0.1/favicon.svg: NS_BINDING_ABORTED"],
    remote: [],
  });
  assert.deepEqual(paired, { console: [], page: [], request: [], remote: [] });
  const material = meaningfulRuntimeDiagnostics({
    console: ["application failure"],
    page: ["TypeError: Importing a module script failed."],
    request: ["GET http://127.0.0.1/_astro/ui.js: connection reset"],
    remote: ["https://example.com/tracker"],
  });
  assert.equal(material.page.length, 1);
  assert.equal(material.request.length, 1);
  assert.equal(material.console.length, 1);
  assert.equal(material.remote.length, 1);
});

test("Astro preserves the explicit canonical route in output and links", { skip: process.platform === "win32", timeout: 120_000 }, async () => {
  const root = await selfContainedPortalFixture();
  try {
    const name = "Mixed Case + café.md";
    await writeFile(path.join(root, "docs", name), "# Exact route\n\n## Deep target\n\n[Jump](#deep-target)\n");
    const configPath = path.join(root, "portal.config.json");
    const config = JSON.parse(await readFile(configPath, "utf8"));
    config.base = "/guide/";
    await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
    commitFixture(root, "add exact route source");
    runLocalAdapter(root);
    const result = spawnSync(process.execPath, [path.join(starterRoot, "node_modules/astro/bin/astro.mjs"), "build"], { cwd: root, encoding: "utf8", timeout: 110_000 });
    assert.equal(result.status, 0, result.stderr || result.stdout);
    const route = "reference/Mixed Case + café";
    assert.equal((await readFile(path.join(root, `src/content/docs/${route}.md`), "utf8")).split("\n").includes(`slug: ${JSON.stringify(route)}`), true);
    assert.match(await readFile(path.join(root, "public/llms.txt"), "utf8"), /\.\/markdown\/reference\/Mixed%20Case%20%2B%20caf%C3%A9\.md/);
    const referenceHtml = await readFile(path.join(root, "dist/reference/index.html"), "utf8");
    assert.match(referenceHtml, /href="\/guide\/reference\/Mixed%20Case%20%2B%20caf%C3%A9\/"/);
    assert.match(referenceHtml, /href="\/guide\/favicon\.svg"/);
    assert.doesNotMatch(referenceHtml, /href="\/guide\/guide\/favicon\.svg"/);
    const routeHtml = await readFile(path.join(root, `dist/${route}/index.html`), "utf8");
    assert.match(routeHtml, /data-codeflow-search-root="reference\/Mixed Case \+ café"/);
    assert.match(routeHtml, /id="deep-target"/);
    assert.match(routeHtml, /href="#deep-target"/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("tool output roots never traverse external symlinks", { skip: process.platform === "win32" }, async () => {
  const { symlink } = await import("node:fs/promises");
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-output-root-"));
  const outside = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-output-canary-"));
  try {
    const sentinel = path.join(outside, "sentinel.txt");
    await writeFile(sentinel, "must survive\n");
    await symlink(outside, path.join(root, "dist"));
    await assert.rejects(assertToolOutputRoots(root, ["dist", ".astro", "node_modules/.astro", "node_modules/.vite"]), /symlink refused/);
    assert.equal(await readFile(sentinel, "utf8"), "must survive\n");
    await rm(path.join(root, "dist"));
    await mkdir(path.join(root, "dist"));
    await assertToolOutputRoots(root, ["dist", ".astro", "node_modules/.astro", "node_modules/.vite"]);
    assert.equal(await readFile(sentinel, "utf8"), "must survive\n");
  } finally {
    await rm(root, { recursive: true, force: true });
    await rm(outside, { recursive: true, force: true });
  }
});

test("attribute-breaking base paths fail before generated output", async () => {
  for (const base of ["/\"><script>alert(1)</script><a href=\"/", "/\" autofocus onfocus=\"alert(1)\" x=\"/"]) {
    const root = await portalFixture();
    try {
      const configPath = path.join(root, "portal.config.json");
      const config = JSON.parse(await readFile(configPath, "utf8"));
      config.base = base;
      await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
      commitFixture(root, "add hostile base path");
      const result = runAdapter(root, false);
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /base:/);
      await assert.rejects(readFile(path.join(root, "src/content/docs/index.md")), /ENOENT/);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("Astro independently rejects remote and encoded base paths", { timeout: 120_000 }, async () => {
  for (const base of ["//attacker.invalid/", "/%2e/", "/%252e/", "/\" onfocus=\"alert(1)\"/"]) {
    const root = await selfContainedPortalFixture();
    try {
      const configPath = path.join(root, "portal.config.json");
      const config = JSON.parse(await readFile(configPath, "utf8"));
      config.base = base;
      await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
      const result = spawnSync(process.execPath, [path.join(starterRoot, "node_modules/astro/bin/astro.mjs"), "build"], { cwd: root, encoding: "utf8", timeout: 110_000 });
      assert.notEqual(result.status, 0, base);
      assert.match(`${result.stderr}\n${result.stdout}`, /base:/, base);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("repository URLs and bounded page metadata use the portable contract", () => {
  for (const value of ["https://github.com/example/repository", "https://xn--bcher-kva.example/repo", "https://127.0.0.1:8443/repo", "https://[2001:db8::1]:443/repo"]) assert.equal(validRepositoryUrl(value), true, value);
  for (const value of ["http://example.com/repo", "https://user:secret@example.com/repo", "https://bücher.example/repo", "https://bad_host.example/repo", "https://example.com:/repo", "https://[2001:db8::1/repo", "https://example.com/repo?token=x"]) assert.equal(validRepositoryUrl(value), false, value);
  assert.equal(validatePageMetadata({ title: "Title", status: "planned" }, "docs/page.md").title, "Title");
  assert.throws(() => validatePageMetadata({ title: "" }, "docs/page.md"), /title is invalid/);
  assert.throws(() => validatePageMetadata({ status: 1 }, "docs/page.md"), /status is invalid/);
});

test("derived titles use the same UTF-16 boundary as declared titles", () => {
  const astral = "🚀";
  assert.equal(titleFor({}, `# ${"a".repeat(254)}${astral}\n`, "docs/page.md").length, 256);
  assert.throws(() => titleFor({}, `# ${"a".repeat(255)}${astral}\n`, "docs/page.md"), /derived title is invalid/);
  assert.equal(titleFor({}, "Body only\n", `docs/${"a".repeat(256)}.md`).length, 256);
  assert.throws(() => titleFor({}, "Body only\n", `docs/${"a".repeat(257)}.md`), /derived title is invalid/);
});

test("the adapter accepts 256-unit derived titles and stubs 257-unit titles", async () => {
  const root = await portalFixture();
  try {
    const source = path.join(root, "docs/page.md");
    await writeFile(source, `# ${"a".repeat(254)}🚀\n`);
    commitFixture(root, "add bounded derived title");
    runAdapter(root);
    let page = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8")).pages[0];
    assert.equal(page.stale, false);
    assert.equal(page.title.length, 256);

    await writeFile(source, `# ${"a".repeat(255)}🚀\n`);
    commitFixture(root, "exceed derived title boundary");
    runAdapter(root);
    page = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8")).pages[0];
    assert.equal(page.stale, true);
    assert.match(page.stale_reason, /derived title is invalid/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("shared authority contract is enforced by the JavaScript producer", async () => {
  const fixture = JSON.parse(await readFile(new URL("./fixtures/authority-contract.json", import.meta.url), "utf8"));
  for (const value of fixture.repository_urls.accepted) assert.equal(validRepositoryUrl(value), true, value);
  for (const value of fixture.repository_urls.rejected) assert.equal(validRepositoryUrl(value), false, value);
  const baseConfig = { schema_version: 1, title: "Guide", description: "Repository guide", theme: "signal", repository_url: null, repository_root: "..", release_version: null, primitive_tokens: null, source_roots: ["docs"], exclude: [], layers: [
    { id: "orient", label: "Orient", description: "Start", paths: ["docs/product.md"] },
    { id: "system", label: "System", description: "Architecture", prefixes: ["docs/decisions"] },
    { id: "reference", label: "Reference", description: "Other", fallback: true },
  ], base: "/" };
  for (const value of fixture.release_versions.accepted) assert.doesNotThrow(() => validatePortalConfig({ ...baseConfig, release_version: value }), value);
  for (const value of fixture.release_versions.rejected) assert.throws(() => validatePortalConfig({ ...baseConfig, release_version: value }), /release_version/, value);
  for (const value of fixture.portal_bases.accepted) assert.equal(validateBase(value), value);
  for (const value of fixture.portal_bases.rejected) assert.throws(() => validateBase(value), /base/, value);
  for (const value of fixture.page_titles.accepted) assert.doesNotThrow(() => validatePageMetadata({ title: value }, "fixture.md"), value);
  for (const value of fixture.page_titles.rejected) assert.throws(() => validatePageMetadata({ title: value }, "fixture.md"), /title/, value);
  for (const value of fixture.page_statuses.accepted) assert.doesNotThrow(() => validatePageMetadata({ status: value }, "fixture.md"), value);
  for (const value of fixture.page_statuses.rejected) assert.throws(() => validatePageMetadata({ status: value }, "fixture.md"), /status/, value);
  for (const item of fixture.frontmatter.accepted) {
    const { frontmatter } = parseMarkdown(`---\n${item.yaml}\n---\n`, item.source_path);
    assert.deepEqual(collectPageIds(frontmatter, "", item.source_path), item.ids);
    assert.deepEqual(extractPageRelationships(frontmatter, "", item.source_path).map(({ type, target }) => [type, target]), item.relationships);
  }
  for (const item of fixture.frontmatter.rejected) {
    assert.throws(() => {
      const { frontmatter } = parseMarkdown(`---\n${item.yaml}\n---\n`, "project-management/tasks/TSK-101.md");
      collectPageIds(frontmatter, "", "project-management/tasks/TSK-101.md");
      extractPageRelationships(frontmatter, "", "project-management/tasks/TSK-101.md");
    }, item.error === "duplicate_key" ? /unique|duplicate/i : new RegExp(item.error, "i"));
  }
});

test("explicit record authority fails closed while absent ids may be inferred", () => {
  assert.deepEqual(collectPageIds({}, "", "project-management/tasks/TSK-101.md"), ["TSK-101"]);
  assert.throws(() => collectPageIds({ id: "bad" }, "", "project-management/tasks/TSK-101.md"), /declared id is invalid/);
  assert.throws(() => extractPageRelationships({ depends_on: ["TSK-102", 7] }, "", "project-management/tasks/TSK-101.md"), /relationship is invalid/);
  assert.throws(() => extractPageRelationships({ depends_on: "not-an-id" }, "", "project-management/tasks/TSK-101.md"), /relationship is invalid/);
  assert.deepEqual(extractPageRelationships({ depends_on: "TSK-102", related: ["ADR-0001"] }, "", "project-management/tasks/TSK-101.md").map(({ type, target }) => [type, target]), [["depends_on", "TSK-102"], ["related", "ADR-0001"]]);
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
  const source = "<!-- editorial note -->\n<script>x</script>\n\nInline `<tag>&` remains code.\n\n## ADR-0048 outcome\n\n```html\n<div>example</div>\n```\n\nAfter the fence.";
  const targets = new Map([["ADR-0048", { route: "/system/decision/", title: "Decision", source_path: "docs/decision.md", status: "accepted", stale: false }]]);
  const rendered = rewriteRepositoryMarkdown(source, { sourcePath: "docs/guide.md", sourceRoutes: new Map(), base: "/", strictTargets: targets, mediaReferences: new Map() });
  assert.doesNotMatch(rendered, /editorial note/);
  assert.match(rendered, /&lt;script&gt;/);
  assert.match(rendered, /`<tag>&`/);
  assert.match(rendered, /<div>example<\/div>/);
  assert.match(rendered, /\n\n## <span class="portal-id-preview">/);
  assert.match(rendered, /```\n\nAfter the fence\./);
});

test("cf-stage fences render token-driven figures and fail closed", () => {
  const source = "```cf-stage\npolicy | one source @accent\n->\nhooks | five shims\nguards | in-session\n->\nprotected | human-merged @positive\ncaption: planes read one source\n```";
  const rendered = renderStageFences(source, "docs/x.md");
  assert.match(rendered, /<figure class="portal-stage"><div class="portal-stage-flow">/);
  assert.match(rendered, /<li class="portal-stage-node" data-role="accent"><span class="k">policy<\/span><span class="s">one source<\/span><\/li>/);
  assert.match(rendered, /<li class="portal-stage-node" data-role="neutral"><span class="k">hooks<\/span>/);
  assert.equal(rendered.split("portal-stage-arrow").length - 1, 2);
  assert.match(rendered, /<figcaption>planes read one source<\/figcaption>/);
  const hostile = renderStageFences("```cf-stage\n<b>x</b> | <i onclick=\"y\">z</i>\n->\nB\n```");
  assert.doesNotMatch(hostile, /<b>|<i onclick/);
  assert.match(hostile, /&lt;b&gt;x&lt;\/b&gt;/);
  const ascii = "```text\nA -> B\n```";
  assert.equal(renderStageFences(ascii), ascii);
  assert.throws(() => renderStageFences("```cf-stage\nonly one stage\n```"), /at least two/);
  assert.throws(() => renderStageFences("```cf-stage\nA @nope\n->\nB\n```"), /unknown node role/);
  assert.throws(() => renderStageFences("```cf-stage\nA\n->\n```"), /final stage/);
  assert.throws(() => renderStageFences("```cf-stage\nA\n->\nB\ncaption: x\ncaption: y\n```"), /one caption/);
  assert.throws(() => renderStageFences(`\`\`\`cf-stage\n${"N\n".repeat(6)}->\nB\n\`\`\``), /node count per stage/);
});

test("a leading H1 that repeats the page title renders once", () => {
  assert.equal(stripLeadingTitleHeading("# Guide\n\nBody.", "Guide"), "Body.");
  assert.equal(stripLeadingTitleHeading("# ADR-0053 — Guide\n\nBody.", "Guide"), "Body.");
  assert.equal(stripLeadingTitleHeading("# ADR-0053 — Guide\n\nBody.", "ADR-0053 — Guide"), "Body.");
  assert.equal(stripLeadingTitleHeading("# TSK-002-001: Guide\n\nBody.", "Guide"), "Body.");
  assert.equal(stripLeadingTitleHeading("# ADR-0056 — high primary effort with bounded workers\n\nBody.", "High primary effort with bounded workers"), "Body.");
  assert.equal(stripLeadingTitleHeading("# guide\n\nBody.", "Guide"), "Body.");
  assert.equal(stripLeadingTitleHeading("# Guide\n\nBody.", "guide"), "Body.");
  for (const heading of ["ApI guide", "API Guide", "ADR-0053 — Something else", "NOTE-001 — API guide"]) {
    const source = `# ${heading}\n\nBody.`;
    assert.equal(stripLeadingTitleHeading(source, "API guide"), source);
  }
  assert.equal(stripLeadingTitleHeading("## Guide\n\nBody.", "Guide"), "## Guide\n\nBody.");
  assert.equal(stripLeadingTitleHeading("# Introduction to Guide\n\nBody.", "Guide"), "# Introduction to Guide\n\nBody.");
  assert.equal(stripLeadingTitleHeading("# Other\n\nBody.", "Guide"), "# Other\n\nBody.");
  assert.equal(stripLeadingTitleHeading("Intro first.\n\n# Guide", "Guide"), "Intro first.\n\n# Guide");
});

test("altitude sections become a tablist with one panel per layer", () => {
  const source = "Intro prose.\n\n## Concept\n\nClaim.\n\n## Architecture\n\n```text\nA -> B\n```\n\n## Technical\n\n| Claim | State |\n|---|---|\n| x | pass |\n\n## Appendix\n\nUnwrapped tail.";
  const rendered = decorateAltitude(source);
  assert.match(rendered, /<div class="portal-altitude-tabs" role="tablist" aria-label="Altitude">\n<button type="button" role="tab" id="portal-tab-concept" aria-controls="portal-panel-concept" aria-selected="true" data-anchor="concept">Concept<\/button>\n<button type="button" role="tab" id="portal-tab-architecture" aria-controls="portal-panel-architecture" aria-selected="false" tabindex="-1" data-anchor="architecture">Architecture<\/button>/);
  assert.match(rendered, /<section class="portal-altitude" role="tabpanel" id="portal-panel-concept" aria-labelledby="portal-tab-concept" data-altitude="concept">\n\n## Concept/);
  assert.match(rendered, /<\/section>\n\n<section class="portal-altitude" role="tabpanel" id="portal-panel-architecture" aria-labelledby="portal-tab-architecture" data-altitude="architecture">\n\n## Architecture/);
  assert.match(rendered, /<\/section>\n\n## Appendix\n\nUnwrapped tail\./);
  assert.equal(rendered.split("<section ").length - 1, rendered.split("</section>").length - 1);
  assert.equal(rendered.split('role="tab"').length - 1, 3);
  // A second, differently shaped source page becomes a tablist the same way —
  // the grammar is corpus-wide, never a single hero page.
  const second = decorateAltitude("# Subsystem\n\nLead.\n\n## Concept\n\nOne claim.\n\n```cf-stage\nA | in @accent\n->\nB | out\n```\n\n## Technical\n\n- evidence");
  assert.match(second, /<div class="portal-altitude-tabs" role="tablist"/);
  assert.equal(second.split('role="tabpanel"').length - 1, 2);
  assert.match(second, /<section class="portal-altitude" role="tabpanel" id="portal-panel-technical"/);
});

test("altitude decoration passes non-conforming and ambiguous sources through", () => {
  for (const source of ["## Concept\n\nOnly one layer.", "## Concept\n\nx\n\n## Concept\n\ny\n\n## Technical\n\nz", "Prose without layers.\n\n## Usage\n\nx"]) {
    assert.equal(decorateAltitude(source), source);
  }
  const nested = decorateAltitude("# Concept\n\n### Concept\n\n## Concept\n\nx\n\n## Technical\n\ny");
  assert.match(nested, /data-anchor="concept-2">Concept<\/button>/);
});

test("the AST rewrite permits external links but refuses remote images and ambiguous references", () => {
  const options = { sourcePath: "docs/guide.md", sourceRoutes: new Map(), base: "/", strictTargets: new Map(), mediaReferences: new Map() };
  assert.match(rewriteRepositoryMarkdown("[Site](https://example.com) and [mail](mailto:test@example.com)", options), /https:\/\/example\.com/);
  assert.throws(() => rewriteRepositoryMarkdown("![Remote](https://example.com/image.png)", options), /remote images/);
  assert.throws(() => rewriteRepositoryMarkdown("[Link][same]\n\n![Image][same]\n\n[same]: asset.png", options), /both a link and an image/);
  assert.match(rewriteRepositoryMarkdown("[unused]: asset.png", options), /\[unused\]: asset\.png/);
  assert.throws(() => rewriteRepositoryMarkdown("[Binary](secret.key)", options), /unsupported local media type/);
  const mediaReferences = new Map();
  assert.match(rewriteRepositoryMarkdown("![Local](<media/Mixed Case + café.png>)", {
    ...options, base: "/guide/", mediaReferences,
  }), /\/guide\/media\/[a-f0-9]{16}-Mixed%20Case%20%2B%20caf%C3%A9\.png/);
  assert.equal(mediaReferences.get("docs/media/Mixed Case + café.png")?.endsWith("-Mixed Case + café.png"), true);
});

test("portal-owned Markdown fragments must match rendered heading anchors", () => {
  const sourceAnchors = new Map([
    ["docs/guide.md", headingAnchors("# Guide\n\n## Local outcome\n")],
    ["docs/decision.md", headingAnchors("# Decision\n\n## Café result\n\n## Repeat\n\n## Repeat\n")],
  ]);
  const options = {
    sourcePath: "docs/guide.md",
    sourceRoutes: new Map([["docs/decision.md", "system/decision"]]),
    base: "/",
    strictTargets: new Map(),
    mediaReferences: new Map(),
    sourceAnchors,
  };
  assert.match(rewriteRepositoryMarkdown("[Local](#local-outcome)", options), /#local-outcome/);
  assert.match(rewriteRepositoryMarkdown("[Encoded](decision.md#caf%C3%A9-result)", options), /caf%C3%A9-result/);
  assert.match(rewriteRepositoryMarkdown("[Second](decision.md#repeat-1)", options), /#repeat-1/);
  for (const link of ["#missing", "decision.md#repeat-2", "decision.md#caf%25C3%25A9-result", "decision.md#bad%ZZ"]) {
    assert.throws(() => rewriteRepositoryMarkdown(`[Broken](${link})`, options), /fragment/);
  }
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
  assert.equal(validateBase("/guide-v2/_docs~1/"), "/guide-v2/_docs~1/");
  assert.equal(withBase("/guide/", "system/architecture"), "/guide/system/architecture/");
  for (const invalid of [
    "guide/", "/guide", "/../guide/", "/guide/?x=1", "/guide//", "/café/", "/%2e/", "/%252e/",
    "/\"><script>alert(1)</script><a href=\"/", "/\" autofocus onfocus=\"alert(1)\" x=\"/",
  ]) assert.throws(() => validateBase(invalid), invalid);
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
    await mkdir(path.join(root, "public/markdown"), { recursive: true });
    await mkdir(path.join(root, "public/media"), { recursive: true });
    await writeFile(path.join(root, "public/markdown/deleted.md"), "must be pruned");
    await writeFile(path.join(root, "public/media/deleted.png"), "must be pruned");
    await publishOwnedCorpus(root, corpus("two"));
    assert.equal(await readFile(path.join(root, "public/notes.txt"), "utf8"), "keep me");
    await assert.rejects(readFile(path.join(root, "public/markdown/deleted.md")), /ENOENT/);
    await assert.rejects(readFile(path.join(root, "public/media/deleted.png")), /ENOENT/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("authoritative public publication refuses unknown active files", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-authority-"));
  try {
    await mkdir(path.join(root, "public"));
    await writeFile(path.join(root, "public/rogue.html"), "<script>rogue()</script>");
    await assert.rejects(publishOwnedCorpus(root, [
      { live: "public", preserveUnknown: false, files: new Map([["favicon.svg", "committed"]]) },
    ]), /refusing uncommitted portal file/);
    assert.equal(await readFile(path.join(root, "public/rogue.html"), "utf8"), "<script>rogue()</script>");
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

test("an ownerless workflow directory is preserved rather than reclaimed without identity", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-incomplete-workflow-"));
  try {
    const lock = path.join(root, ".portal/workflow.lock");
    await mkdir(lock, { recursive: true });
    const stale = new Date(Date.now() - 10_000);
    await utimes(lock, stale, stale);
    await assert.rejects(withWorkflowLease(root, async () => "unsafe"), /stable regular|directory/);
    assert.equal((await lstat(lock)).isDirectory(), true);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("a stalled workflow candidate cannot delete the atomic winner", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-workflow-candidate-race-"));
  try {
    let candidateReady;
    const ready = new Promise((resolve) => { candidateReady = resolve; });
    let resumeCreator;
    const resume = new Promise((resolve) => { resumeCreator = resolve; });
    let releaseWinner;
    const winnerHeld = new Promise((resolve) => { releaseWinner = resolve; });
    const creator = withWorkflowLease(root, async () => "creator", {
      afterCandidateReady: async (candidate, lease) => {
        const stale = Date.now() - 10_000;
        await writeFile(path.join(candidate, "owner.json"), JSON.stringify({ ...lease, created_at_ms: stale, heartbeat_at_ms: stale }));
        candidateReady();
        await resume;
      },
    }).catch((error) => error);
    await ready;
    let winnerToken;
    const winner = withWorkflowLease(root, async () => {
      winnerToken = JSON.parse(await readFile(path.join(root, ".portal/workflow.lock"), "utf8")).token;
      await winnerHeld;
      return "winner";
    });
    await waitUntil(() => winnerToken !== undefined);
    resumeCreator();
    const creatorResult = await creator;
    assert.match(String(creatorResult), /already in progress/);
    assert.equal(JSON.parse(await readFile(path.join(root, ".portal/workflow.lock"), "utf8")).token, winnerToken);
    releaseWinner();
    assert.equal(await winner, "winner");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("recovery revalidates a candidate published after its active-claim snapshot", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-workflow-recovery-snapshot-race-"));
  try {
    let candidateReady;
    const candidateIsReady = new Promise((resolve) => { candidateReady = resolve; });
    let publishCandidate;
    const publish = new Promise((resolve) => { publishCandidate = resolve; });
    let snapshotReady;
    const snapshotIsReady = new Promise((resolve) => { snapshotReady = resolve; });
    let resumeRecovery;
    const recover = new Promise((resolve) => { resumeRecovery = resolve; });
    let releaseWinner;
    const winnerHeld = new Promise((resolve) => { releaseWinner = resolve; });
    let winnerToken;
    const winner = withWorkflowLease(root, async () => {
      winnerToken = JSON.parse(await readFile(path.join(root, ".portal/workflow.lock"), "utf8")).token;
      await winnerHeld;
      return "winner";
    }, {
      afterCandidateReady: async (candidate, lease) => {
        const stale = Date.now() - 10_000;
        await writeFile(path.join(candidate, "owner.json"), JSON.stringify({ ...lease, created_at_ms: stale, heartbeat_at_ms: stale }));
        candidateReady();
        await publish;
      },
    });
    await candidateIsReady;
    const reclaimer = withWorkflowLease(root, async () => "unexpected", {
      afterRecoverySnapshot: async ({ activeToken }) => {
        assert.equal(activeToken, null);
        snapshotReady();
        await recover;
      },
    }).catch((error) => error);
    await snapshotIsReady;
    publishCandidate();
    await waitUntil(() => winnerToken !== undefined);
    resumeRecovery();
    assert.match(String(await reclaimer), /already in progress/);
    const candidate = path.join(root, `.portal/.workflow-lock-candidate-${winnerToken}`);
    assert.equal(JSON.parse(await readFile(path.join(candidate, "owner.json"), "utf8")).token, winnerToken);
    assert.equal(JSON.parse(await readFile(path.join(root, ".portal/workflow.lock"), "utf8")).token, winnerToken);
    releaseWinner();
    assert.equal(await winner, "winner");
    assert.equal(await withWorkflowLease(root, async () => "future"), "future");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("failed creator verification repairs only its identity-bound claim and candidate", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-workflow-failed-creator-"));
  try {
    const unrelatedToken = "b".repeat(48);
    const unrelated = path.join(root, `.portal/.workflow-lock-candidate-${unrelatedToken}`);
    const result = await withWorkflowLease(root, async () => "unexpected", {
      afterClaimPublished: async (candidate, lease) => {
        const unrelatedLease = { schema_version: 1, token: unrelatedToken, created_at_ms: Date.now(), heartbeat_at_ms: Date.now() };
        await mkdir(unrelated);
        await writeFile(path.join(unrelated, "owner.json"), JSON.stringify(unrelatedLease));
        await rename(candidate, path.join(root, `.portal/.workflow-lock-recovery-${lease.token}`));
      },
    }).catch((error) => error);
    assert.match(String(result), /could not acquire/);
    await assert.rejects(readFile(path.join(root, ".portal/workflow.lock")), /ENOENT/);
    assert.equal(JSON.parse(await readFile(path.join(unrelated, "owner.json"), "utf8")).token, unrelatedToken);
    assert.equal(await withWorkflowLease(root, async () => "future"), "future");
    assert.equal(JSON.parse(await readFile(path.join(unrelated, "owner.json"), "utf8")).token, unrelatedToken);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("a stalled stale-lease reclaimer cannot remove a later winner", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-workflow-reclaimer-race-"));
  try {
    const stale = Date.now() - 5 * 60 * 60 * 1000;
    const staleLease = {
      schema_version: 1, token: "a".repeat(48), created_at_ms: stale, heartbeat_at_ms: stale,
    };
    const staleCandidate = path.join(root, `.portal/.workflow-lock-candidate-${staleLease.token}`);
    await mkdir(staleCandidate, { recursive: true });
    await writeFile(path.join(staleCandidate, "owner.json"), JSON.stringify(staleLease));
    await writeFile(path.join(root, ".portal/workflow.lock"), JSON.stringify(staleLease));
    let reclaimReady;
    const ready = new Promise((resolve) => { reclaimReady = resolve; });
    let resumeReclaimer;
    const resume = new Promise((resolve) => { resumeReclaimer = resolve; });
    let releaseWinner;
    const winnerHeld = new Promise((resolve) => { releaseWinner = resolve; });
    const reclaimer = withWorkflowLease(root, async () => "reclaimer", {
      afterStaleRename: async () => { reclaimReady(); await resume; },
    }).catch((error) => error);
    await ready;
    let winnerToken;
    const winner = withWorkflowLease(root, async () => {
      winnerToken = JSON.parse(await readFile(path.join(root, ".portal/workflow.lock"), "utf8")).token;
      await winnerHeld;
      return "winner";
    });
    await waitUntil(() => winnerToken !== undefined);
    resumeReclaimer();
    assert.match(String(await reclaimer), /already in progress/);
    assert.equal(JSON.parse(await readFile(path.join(root, ".portal/workflow.lock"), "utf8")).token, winnerToken);
    releaseWinner();
    assert.equal(await winner, "winner");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("claim retirement preserves a replacement published after the expected claim leaves", async () => {
  for (const reason of ["reclaim", "release", "failed"]) {
    const root = await mkdtemp(path.join(os.tmpdir(), `codeflow-portal-workflow-retire-${reason}-`));
    try {
      let retirementReady;
      const retirementIsReady = new Promise((resolve) => { retirementReady = resolve; });
      let resumeRetirement;
      const resume = new Promise((resolve) => { resumeRetirement = resolve; });
      const hooks = {
        beforeClaimRetirement: async ({ reason: actual }) => {
          assert.equal(actual, reason);
          retirementReady();
          await resume;
        },
      };
      let operation;
      if (reason === "reclaim") {
        const stale = Date.now() - 5 * 60 * 60 * 1000;
        const lease = { schema_version: 1, token: "a".repeat(48), created_at_ms: stale, heartbeat_at_ms: stale };
        const candidate = path.join(root, `.portal/.workflow-lock-candidate-${lease.token}`);
        await mkdir(candidate, { recursive: true });
        await writeFile(path.join(candidate, "owner.json"), JSON.stringify(lease));
        await writeFile(path.join(root, ".portal/workflow.lock"), JSON.stringify(lease));
        operation = withWorkflowLease(root, async () => "unexpected", hooks).catch((error) => error);
      } else if (reason === "release") {
        operation = withWorkflowLease(root, async () => "released", hooks);
      } else {
        hooks.afterClaimPublished = async (candidate, lease) => {
          await rename(candidate, path.join(root, `.portal/.workflow-lock-recovery-${lease.token}`));
        };
        operation = withWorkflowLease(root, async () => "unexpected", hooks).catch((error) => error);
      }
      await retirementIsReady;
      await rm(path.join(root, ".portal/workflow.lock"));
      let replacementToken;
      let replacementReady;
      const replacementIsReady = new Promise((resolve) => { replacementReady = resolve; });
      let releaseReplacement;
      const replacementHeld = new Promise((resolve) => { releaseReplacement = resolve; });
      const replacement = withWorkflowLease(root, async () => {
        replacementToken = JSON.parse(await readFile(path.join(root, ".portal/workflow.lock"), "utf8")).token;
        replacementReady();
        await replacementHeld;
        return "replacement";
      });
      await replacementIsReady;
      resumeRetirement();
      const result = await operation;
      if (reason === "release") assert.equal(result, "released");
      else assert.match(String(result), reason === "reclaim" ? /already in progress/ : /could not acquire/);
      assert.equal(JSON.parse(await readFile(path.join(root, ".portal/workflow.lock"), "utf8")).token, replacementToken);
      releaseReplacement();
      assert.equal(await replacement, "replacement");
      assert.equal(await withWorkflowLease(root, async () => "future"), "future");
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("a replacement released while displaced cannot be resurrected", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-workflow-displaced-release-"));
  try {
    const stale = Date.now() - 5 * 60 * 60 * 1000;
    const staleLease = { schema_version: 1, token: "a".repeat(48), created_at_ms: stale, heartbeat_at_ms: stale };
    const staleCandidate = path.join(root, `.portal/.workflow-lock-candidate-${staleLease.token}`);
    await mkdir(staleCandidate, { recursive: true });
    await writeFile(path.join(staleCandidate, "owner.json"), JSON.stringify(staleLease));
    await writeFile(path.join(root, ".portal/workflow.lock"), JSON.stringify(staleLease));
    let retirementReady;
    const retirementIsReady = new Promise((resolve) => { retirementReady = resolve; });
    let moveClaim;
    const move = new Promise((resolve) => { moveClaim = resolve; });
    let claimMoved;
    const claimIsMoved = new Promise((resolve) => { claimMoved = resolve; });
    let inspectMovedClaim;
    const inspect = new Promise((resolve) => { inspectMovedClaim = resolve; });
    const reclaimer = withWorkflowLease(root, async () => "reclaimer", {
      beforeClaimRetirement: async ({ reason }) => {
        if (reason !== "reclaim") return;
        retirementReady();
        await move;
      },
      afterClaimMoved: async ({ reason }) => {
        if (reason !== "reclaim") return;
        claimMoved();
        await inspect;
      },
    });
    await retirementIsReady;
    await rm(path.join(root, ".portal/workflow.lock"));
    let replacementToken;
    let replacementReady;
    const replacementIsReady = new Promise((resolve) => { replacementReady = resolve; });
    let releaseReplacement;
    const replacementHeld = new Promise((resolve) => { releaseReplacement = resolve; });
    const replacement = withWorkflowLease(root, async () => {
      replacementToken = JSON.parse(await readFile(path.join(root, ".portal/workflow.lock"), "utf8")).token;
      replacementReady();
      await replacementHeld;
      return "replacement";
    });
    await replacementIsReady;
    moveClaim();
    await claimIsMoved;
    releaseReplacement();
    assert.equal(await replacement, "replacement");
    await assert.rejects(readFile(path.join(root, `.portal/.workflow-lock-candidate-${replacementToken}/owner.json`)), /ENOENT/);
    inspectMovedClaim();
    assert.equal(await reclaimer, "reclaimer");
    const debris = (await readdir(path.join(root, ".portal"))).filter((name) => name.includes(replacementToken));
    assert.deepEqual(debris, []);
    await assert.rejects(readFile(path.join(root, ".portal/workflow.lock")), /ENOENT/);
    assert.equal(await withWorkflowLease(root, async () => "future"), "future");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("recovery honors displaced-claim liveness at both restoration crash boundaries", async () => {
  for (const boundary of ["before-restore", "after-relink"]) {
    const root = await mkdtemp(path.join(os.tmpdir(), `codeflow-portal-workflow-displaced-crash-${boundary}-`));
    try {
      const portal = path.join(root, ".portal");
      const token = "b".repeat(48);
      const lease = { schema_version: 1, token, created_at_ms: Date.now(), heartbeat_at_ms: Date.now() };
      const candidate = path.join(portal, `.workflow-lock-candidate-${token}`);
      const retired = path.join(portal, `.workflow-lock-reclaim-${"a".repeat(48)}-${"c".repeat(24)}.json`);
      await mkdir(candidate, { recursive: true });
      const owner = path.join(candidate, "owner.json");
      await writeFile(owner, JSON.stringify(lease));
      await link(owner, retired);
      if (boundary === "after-relink") {
        await link(owner, path.join(portal, "workflow.lock"));
        await rm(candidate, { recursive: true });
      } else {
        const blocked = await withWorkflowLease(root, async () => "unexpected").catch((error) => error);
        assert.match(String(blocked), /already in progress/);
        assert.equal(JSON.parse(await readFile(path.join(portal, "workflow.lock"), "utf8")).token, token);
        await rm(candidate, { recursive: true });
      }
      assert.equal(await withWorkflowLease(root, async () => "future"), "future");
      assert.deepEqual((await readdir(portal)).filter((name) => name.includes(token)), []);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("unverified retired claims are quarantined without regaining authority", { skip: process.platform === "win32" }, async () => {
  for (const mutation of ["corrupt", "swap", "symlink"]) {
    const root = await mkdtemp(path.join(os.tmpdir(), `codeflow-portal-workflow-retired-${mutation}-`));
    try {
      const result = await withWorkflowLease(root, async () => "released", {
        afterClaimMoved: async ({ retired, expected }) => {
          if (mutation === "corrupt") await writeFile(retired, "{not-json");
          if (mutation === "swap") {
            await rm(retired);
            await writeFile(retired, JSON.stringify(expected));
          }
          if (mutation === "symlink") {
            const outside = path.join(root, "outside-claim.json");
            await writeFile(outside, JSON.stringify(expected));
            await rm(retired);
            await symlink(outside, retired);
          }
        },
      }).catch((error) => error);
      assert.equal(result?.code, "CODEFLOW_WORKFLOW_CLAIM_QUARANTINED");
      assert.match(String(result), /failed identity verification and was not restored; quarantined/);
      await assert.rejects(readFile(path.join(root, ".portal/workflow.lock")), /ENOENT/);
      const entries = await readdir(path.join(root, ".portal"));
      assert.equal(entries.filter((name) => name.startsWith(".workflow-lock-suspect-")).length, 1);
      assert.equal(entries.some((name) => /^\.workflow-lock-(?:failed|reclaim|release)-/.test(name)), false);
      assert.equal(await withWorkflowLease(root, async () => "future"), "future");
      await assert.rejects(readFile(path.join(root, ".portal/workflow.lock")), /ENOENT/);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("recovery quarantines invalid retired debris once and bounds diagnostics", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-workflow-suspect-recovery-"));
  try {
    const portal = path.join(root, ".portal");
    await mkdir(portal);
    const token = "a".repeat(48);
    await writeFile(path.join(portal, `.workflow-lock-release-${token}-${"b".repeat(24)}.json`), "{invalid");
    const first = await withWorkflowLease(root, async () => "unexpected").catch((error) => error);
    assert.equal(first?.code, "CODEFLOW_WORKFLOW_CLAIM_QUARANTINED");
    assert.equal(await withWorkflowLease(root, async () => "recovered"), "recovered");
    for (let index = 1; index < 64; index += 1) {
      await writeFile(path.join(portal, `.workflow-lock-suspect-${token}-${index.toString(16).padStart(24, "0")}`), "diagnostic");
    }
    assert.equal(await withWorkflowLease(root, async () => "at-budget"), "at-budget");
    await writeFile(path.join(portal, `.workflow-lock-suspect-${token}-${"f".repeat(24)}`), "over-budget");
    await assert.rejects(withWorkflowLease(root, async () => "unexpected"), /too many candidates/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("workflow interruption reaches the child and releases its lease", { skip: process.platform === "win32", timeout: 15_000 }, async () => {
  const root = await selfContainedPortalFixture();
  try {
    const astro = path.join(root, "node_modules/astro/bin/astro.mjs");
    await mkdir(path.dirname(astro), { recursive: true });
    await writeFile(astro, `
      import { writeFileSync } from "node:fs";
      import path from "node:path";
      writeFileSync(path.join(process.cwd(), ".child-ready"), "ready\\n");
      process.on("SIGTERM", () => {
        writeFileSync(path.join(process.cwd(), ".child-signal"), "SIGTERM\\n");
        setTimeout(() => process.exit(0), 25);
      });
      setInterval(() => {}, 1000);
    `);
    const child = spawn(process.execPath, [path.join(root, "scripts/workflow.mjs"), "preview"], { cwd: root, stdio: "ignore" });
    await waitUntil(async () => Promise.all([
      readFile(path.join(root, ".portal/workflow.lock")),
      readFile(path.join(root, ".child-ready")),
    ]).then(() => true, () => false));
    child.kill("SIGTERM");
    const outcome = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code, signal) => resolve({ code, signal }));
    });
    assert.deepEqual(outcome, { code: null, signal: "SIGTERM" });
    assert.equal(await readFile(path.join(root, ".child-signal"), "utf8"), "SIGTERM\n");
    await assert.rejects(readFile(path.join(root, ".portal/workflow.lock")), /ENOENT/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("direct browser lifecycle interruption closes resources and releases its lease", { skip: process.platform === "win32", timeout: 15_000 }, async () => {
  const root = await selfContainedPortalFixture();
  try {
    const runner = path.join(root, "browser-lifecycle-fixture.mjs");
    await writeFile(runner, `
      import { spawn } from "node:child_process";
      import { appendFile, writeFile } from "node:fs/promises";
      import path from "node:path";
      import { withSignalAwareChildLifecycle } from "./scripts/child-lifecycle.mjs";
      import { withWorkflowLease } from "./scripts/publication.mjs";
      const root = process.cwd();
      await withSignalAwareChildLifecycle((lifecycle) => withWorkflowLease(root, async () => {
        const preview = spawn(process.execPath, ["-e", "process.on('SIGTERM',()=>process.exit(0));setInterval(()=>{},1000)"], { stdio: "ignore" });
        lifecycle.trackChild(preview);
        let cleanupRuns = 0;
        lifecycle.addCleanup(async () => {
          cleanupRuns += 1;
          await writeFile(path.join(root, ".browser-cleanup-count"), String(cleanupRuns) + "\\n");
          await appendFile(path.join(root, ".browser-cleanup-order"), "cleanup-start\\n");
          await new Promise((resolve) => setTimeout(resolve, 150));
          await appendFile(path.join(root, ".browser-cleanup-order"), "cleanup-end\\n");
        });
        await writeFile(path.join(root, ".browser-ready"), "ready\\n");
        await lifecycle.wait(new Promise(() => {}));
      }));
    `);
    const child = spawn(process.execPath, [runner], { cwd: root, stdio: "ignore" });
    await waitUntil(async () => Promise.all([
      readFile(path.join(root, ".portal/workflow.lock")),
      readFile(path.join(root, ".browser-ready")),
    ]).then(() => true, () => false));
    child.kill("SIGTERM");
    setTimeout(() => child.kill("SIGINT"), 25);
    const outcome = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code, signal) => resolve({ code, signal }));
    });
    assert.deepEqual(outcome, { code: null, signal: "SIGTERM" });
    assert.equal(await readFile(path.join(root, ".browser-cleanup-count"), "utf8"), "1\n");
    assert.equal(await readFile(path.join(root, ".browser-cleanup-order"), "utf8"), "cleanup-start\ncleanup-end\n");
    await assert.rejects(readFile(path.join(root, ".portal/workflow.lock")), /ENOENT/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("child cleanup forces a stubborn process after its listener closes", { skip: process.platform === "win32", timeout: 5_000 }, async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-stubborn-child-"));
  const child = spawn(process.execPath, ["-e", `
    const { writeFileSync } = require("node:fs");
    const net = require("node:net");
    const path = require("node:path");
    const root = process.cwd();
    const server = net.createServer(() => {});
    server.listen(0, "127.0.0.1", () => writeFileSync(path.join(root, ".stubborn-ready"), "ready\\n"));
    process.on("SIGTERM", () => server.close(() => writeFileSync(path.join(root, ".listener-closed"), "closed\\n")));
    setInterval(() => {}, 1_000);
  `], { cwd: root, stdio: "ignore" });
  try {
    await waitUntil(() => readFile(path.join(root, ".stubborn-ready")).then(() => true, () => false));
    await stopChild(child, "SIGTERM", { graceMs: 250 });
    assert.equal(child.signalCode, "SIGKILL");
    assert.equal(await readFile(path.join(root, ".listener-closed"), "utf8"), "closed\n");
  } finally {
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
    await new Promise((resolve) => child.exitCode !== null || child.signalCode !== null ? resolve() : child.once("exit", resolve));
    await rm(root, { recursive: true, force: true });
  }
});

test("child cleanup rejects kill failure without exit proof", async () => {
  class FailedKillChild extends EventEmitter {
    pid = 42;
    exitCode = null;
    signalCode = null;
    kill() { return false; }
  }
  await assert.rejects(
    stopChild(new FailedKillChild(), "SIGTERM", { graceMs: 10 }),
    /SIGTERM returned false; SIGKILL returned false/,
  );
});

test("child cleanup contains asynchronous kill errors", async () => {
  class AsyncKillErrorChild extends EventEmitter {
    pid = 45;
    exitCode = null;
    signalCode = null;
    kill(signal) {
      queueMicrotask(() => this.emit("error", new Error(`${signal} denied`)));
      return true;
    }
  }
  await assert.rejects(
    stopChild(new AsyncKillErrorChild(), "SIGTERM", { graceMs: 10 }),
    /child emitted error: SIGTERM denied; SIGKILL denied/,
  );
});

test("child cleanup reports synchronous kill exceptions", async () => {
  class ThrowingKillChild extends EventEmitter {
    pid = 46;
    exitCode = null;
    signalCode = null;
    kill(signal) { throw new Error(`${signal} refused`); }
  }
  await assert.rejects(
    stopChild(new ThrowingKillChild(), "SIGTERM", { graceMs: 10 }),
    /SIGTERM threw SIGTERM refused; SIGKILL threw SIGKILL refused/,
  );
});

test("child cleanup rejects an ambiguous exit event", async () => {
  class AmbiguousExitChild extends EventEmitter {
    pid = 43;
    exitCode = null;
    signalCode = null;
    kill() {
      queueMicrotask(() => this.emit("exit", null, null));
      return true;
    }
  }
  await assert.rejects(
    stopChild(new AmbiguousExitChild(), "SIGTERM", { graceMs: 10 }),
    /exit event without a code or signal was not accepted as proof/,
  );
});

test("lifecycle drains a delayed acquisition before re-signalling", { skip: process.platform === "win32", timeout: 15_000 }, async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-delayed-acquire-"));
  try {
    const runner = path.join(root, "delayed-acquire.mjs");
    await writeFile(runner, `
      import { appendFile, writeFile } from "node:fs/promises";
      import path from "node:path";
      import { withSignalAwareChildLifecycle } from ${JSON.stringify(new URL("../scripts/child-lifecycle.mjs", import.meta.url).href)};
      const root = process.cwd();
      let cleanupRuns = 0;
      await withSignalAwareChildLifecycle(async (lifecycle) => {
        const resource = new Promise((resolve) => setTimeout(async () => {
          await appendFile(path.join(root, ".acquire-order"), "acquired\\n");
          resolve({ id: "resource" });
        }, 150));
        const acquisition = lifecycle.acquire(resource, async ({ id }) => {
          cleanupRuns += 1;
          await appendFile(path.join(root, ".acquire-order"), "cleanup-" + id + "\\n");
          await writeFile(path.join(root, ".acquire-cleanup-count"), String(cleanupRuns) + "\\n");
        });
        // Attach the rejection handler before publishing readiness. SIGTERM
        // can arrive while the ready-file write is still completing.
        await Promise.all([acquisition, writeFile(path.join(root, ".acquire-ready"), "ready\\n")]);
      });
    `);
    const child = spawn(process.execPath, [runner], { cwd: root, stdio: "ignore" });
    await waitUntil(() => readFile(path.join(root, ".acquire-ready")).then(() => true, () => false));
    child.kill("SIGTERM");
    const outcome = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code, signal) => resolve({ code, signal }));
    });
    assert.deepEqual(outcome, { code: null, signal: "SIGTERM" });
    assert.equal(await readFile(path.join(root, ".acquire-cleanup-count"), "utf8"), "1\n");
    assert.equal(await readFile(path.join(root, ".acquire-order"), "utf8"), "acquired\ncleanup-resource\n");
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
    await assert.rejects(collectBuiltArtifacts(root, { maximumEntries: 2 }), /entry count exceeds/);
    await assert.rejects(collectBuiltArtifacts(root, { maximumTotalBytes: 1 }), /corpus exceeds/);
    await assert.rejects(collectBuiltArtifacts(root, { maximumDepth: 1 }), /depth exceeds/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("tool output traversal uses one global total-entry budget", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-tool-output-budget-"));
  try {
    await mkdir(path.join(root, "one/nested"), { recursive: true });
    await mkdir(path.join(root, "two"));
    await writeFile(path.join(root, "two/file.txt"), "x");
    await assert.rejects(assertToolOutputRoots(root, ["one", "two"], 1), /total entries/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("streamed browser artifact hashes reject growth and path replacement races", { skip: process.platform === "win32" }, async () => {
  const { symlink } = await import("node:fs/promises");
  for (const race of ["growth", "swap"]) {
    const root = await mkdtemp(path.join(os.tmpdir(), `codeflow-portal-browser-hash-${race}-`));
    try {
      const artifact = path.join(root, "trace.zip");
      const secret = path.join(root, "secret.txt");
      await writeFile(artifact, "stable");
      await writeFile(secret, "secret");
      await assert.rejects(hashBoundedRegularFile(artifact, 64, "browser evidence artifact", {
        afterOpen: async () => {
          if (race === "growth") await writeFile(artifact, "stable-but-changed");
          else {
            await rename(artifact, `${artifact}.opened`);
            await symlink(secret, artifact);
          }
        },
      }), /changed while it was being read/);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
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

test("Git snapshot accepts native SHA-256 object identities", async (context) => {
  const root = await mkdtemp(path.join(os.tmpdir(), "codeflow-portal-sha256-"));
  try {
    const initialized = spawnSync("git", ["-C", root, "init", "-q", "--object-format=sha256"], { encoding: "utf8" });
    if (initialized.status !== 0) return context.skip("installed Git does not support SHA-256 repositories");
    git(root, ["config", "user.email", "portal-tests@codeflow.invalid"]);
    git(root, ["config", "user.name", "CodeFlow portal tests"]);
    await writeFile(path.join(root, "page.md"), "# SHA-256\n");
    commitFixture(root, "sha256 fixture");
    const snapshot = new GitSnapshot(root);
    const commit = snapshot.resolveHead();
    assert.equal(commit.length, 64);
    snapshot.loadInventory(commit);
    const record = snapshot.requireRegular("page.md", ["100644"], "fixture");
    assert.equal(snapshot.readBlobs([record], { perObjectBytes: 1024, totalBytes: 1024, label: "fixture" }).get("page.md").toString(), "# SHA-256\n");
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

test("all portal child classes execute without inherited provider secrets", () => {
  const environment = hardenedChildEnvironment({
    PATH: process.env.PATH,
    HOME: "/safe/home",
    TMPDIR: "/safe/tmp",
    LANG: "C.UTF-8",
    AWS_SECRET_ACCESS_KEY: "aws-canary",
    AZURE_CLIENT_SECRET: "azure-canary",
    GOOGLE_APPLICATION_CREDENTIALS: "/secret/google.json",
    OPENAI_API_KEY: "openai-canary",
    ANTHROPIC_API_KEY: "anthropic-canary",
    NPM_TOKEN: "npm-canary",
    npm_execpath: "/trusted/npm-cli.js",
    NODE_OPTIONS: "--require=/tmp/inject.cjs",
    NODE_PATH: "/tmp/inject-modules",
    LD_PRELOAD: "/tmp/inject.so",
    DYLD_INSERT_LIBRARIES: "/tmp/inject.dylib",
  }, { BROWSER: "none" });
  assert.equal(environment.HOME, "/safe/home");
  assert.equal(environment.BROWSER, "none");
  assert.throws(() => hardenedChildEnvironment({ PATH: "/safe/bin" }, { OPENAI_API_KEY: "override" }), /override is not allowed/);

  const keys = [
    "AWS_SECRET_ACCESS_KEY",
    "AZURE_CLIENT_SECRET",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "NPM_TOKEN",
    "npm_execpath",
    "NODE_OPTIONS",
    "NODE_PATH",
    "LD_PRELOAD",
    "DYLD_INSERT_LIBRARIES",
  ];
  const probe = spawnSync(process.execPath, ["-e", `process.stdout.write(JSON.stringify(${JSON.stringify(keys)}.filter((key) => key in process.env)))`], {
    encoding: "utf8",
    env: environment,
  });
  assert.equal(probe.status, 0, probe.stderr);
  assert.deepEqual(JSON.parse(probe.stdout), []);

  const windowsLike = hardenedChildEnvironment({
    Path: "C:\\Windows\\System32",
    SystemRoot: "C:\\Windows",
    USERPROFILE: "C:\\Users\\safe",
    PATHEXT: ".COM;.EXE;.BAT;.CMD",
    APPDATA: "C:\\Users\\safe\\AppData\\Roaming",
    LOCALAPPDATA: "C:\\Users\\safe\\AppData\\Local",
    npm_execpath: "C:\\Program Files\\nodejs\\node_modules\\npm\\bin\\npm-cli.js",
  });
  assert.equal(windowsLike.PATH, "C:\\Windows\\System32");
  assert.equal(windowsLike.USERPROFILE, "C:\\Users\\safe");
  for (const key of ["APPDATA", "LOCALAPPDATA", "npm_execpath"]) assert.equal(key in windowsLike, false);
});

test("locked installs reject unreviewed dependency lifecycle scripts", async () => {
  const lockfile = JSON.parse(await readFile(new URL("../package-lock.json", import.meta.url), "utf8"));
  assert.doesNotThrow(() => assertReviewedInstallScripts(lockfile));
  const unexpected = structuredClone(lockfile);
  unexpected.packages["node_modules/unreviewed"] = { version: "1.0.0", hasInstallScript: true };
  assert.throws(() => assertReviewedInstallScripts(unexpected), /unreviewed dependency lifecycle scripts/);
  const changed = structuredClone(lockfile);
  const [reviewedPath] = REVIEWED_IGNORED_LIFECYCLE_SCRIPTS.keys();
  changed.packages[reviewedPath].version = "999.0.0";
  assert.throws(() => assertReviewedInstallScripts(changed), /reviewed ignored lifecycle script changed/);
});

test("the adapter emits one bounded non-searchable current-source stub without ancestor content", async () => {
  const root = await portalFixture();
  try {
    const source = path.join(root, "docs/guide.md");
    await writeFile(source, "---\nid: TSK-101\ntitle: Guide\ndepends_on: [TSK-102]\n---\n\n# Guide\n\nGrounded content references TSK-102.\n");
    await writeFile(path.join(root, "docs/target.md"), "---\nid: TSK-102\ntitle: Current target\ndepends_on: [TSK-101]\n---\n\n# Target\n\nThis current page references TSK-101.\n");
    commitFixture(root, "add valid guide");
    runAdapter(root);
    const prior = await readFile(path.join(root, "src/content/docs/reference/guide.md"), "utf8");
    assert.match(prior, /Grounded content/);
    await writeFile(source, "---\nid: TSK-101\ntitle: [broken\n");
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
    assert.deepEqual(stale.unavailable_ids, ["TSK-101"]);
    assert.deepEqual(stale.relationships, []);
    assert.equal(typeof stale.stale_reason, "string");
    assert.equal("last_good_commit" in stale, false);
    assert.equal("last_good_source_sha256" in stale, false);
    assert.deepEqual(target.backlinks, []);
    const targetOutput = await readFile(path.join(root, "src/content/docs/reference/target.md"), "utf8");
    assert.match(targetOutput, /TSK-101 — stale/);
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

test("masked source, token, and media edits block publication", async () => {
  for (const flag of ["--assume-unchanged", "--skip-worktree"]) {
    const root = await portalFixture();
    try {
      const source = path.join(root, "docs/guide.md");
      await writeFile(source, "# Committed guide\n\nCommitted truth.\n");
      commitFixture(root, "add committed guide");
      git(root, ["update-index", flag, "docs/guide.md"]);
      await writeFile(source, "# Hidden worktree edit\n\nMust not be published.\n");
      assert.match(runAdapter(root, false).stderr, /portal source does not match/);
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

  for (const input of ["primitive token import", "referenced media"]) {
    const root = await portalFixture();
    try {
      const configPath = path.join(root, "portal.config.json");
      const config = JSON.parse(await readFile(configPath, "utf8"));
      await writeFile(path.join(root, "docs/guide.md"), input === "referenced media" ? "# Guide\n\n![Diagram](../assets/diagram.png)\n" : "# Guide\n");
      let inputPath;
      if (input === "primitive token import") {
        inputPath = "tokens.json";
        config.primitive_tokens = inputPath;
        await writeFile(path.join(root, inputPath), JSON.stringify({ schema_version: 1, light: { accent: "#005f56" }, dark: { accent: "#72e2cf" } }));
      } else {
        inputPath = "assets/diagram.png";
        await mkdir(path.join(root, "assets"));
        await writeFile(path.join(root, inputPath), pngHeader(1, 1));
      }
      await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
      commitFixture(root, `add ${input}`);
      git(root, ["update-index", "--assume-unchanged", inputPath]);
      if (input === "primitive token import") await writeFile(path.join(root, inputPath), JSON.stringify({ schema_version: 1, light: { accent: "#006f66" }, dark: { accent: "#82f2df" } }));
      else await writeFile(path.join(root, inputPath), pngHeader(2, 1));
      assert.match(runAdapter(root, false).stderr, new RegExp(`${input} does not match`));
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});

test("tracked public runtime bytes are authoritative and untracked active content is refused", async () => {
  for (const flag of ["--assume-unchanged", "--skip-worktree"]) {
    const root = await selfContainedPortalFixture();
    try {
      const favicon = path.join(root, "public/favicon.svg");
      git(root, ["update-index", flag, "public/favicon.svg"]);
      await writeFile(favicon, "<svg><script>masked()</script></svg>");
      assert.match(runLocalAdapter(root, false).stderr, /portal runtime input does not match/);
    } finally { await rm(root, { recursive: true, force: true }); }
  }
  const root = await selfContainedPortalFixture();
  try {
    const nodeVersionPath = path.join(root, ".node-version");
    const nodeVersion = await readFile(nodeVersionPath, "utf8");
    await writeFile(nodeVersionPath, nodeVersion.replace(/\r?\n/g, "\r\n"));
    await writeFile(path.join(root, "public/rogue.html"), "<script>rogue()</script>");
    assert.match(runLocalAdapter(root, false).stderr, /must match HEAD exactly|refusing uncommitted portal file/);
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("Git status pathspecs are split below Windows process limits", () => {
  const pathspecs = Array.from({ length: 1_000 }, (_, index) => `:(top,literal)assets/media/${String(index).padStart(4, "0")}/${"x".repeat(80)}.png`);
  const batches = boundedPathspecBatches(pathspecs);
  assert.ok(batches.length > 1);
  assert.equal(batches.flat().length, pathspecs.length);
  for (const batch of batches) {
    assert.ok(batch.length <= 64);
    assert.ok(batch.reduce((units, item) => units + item.length + 1, 0) <= 8 * 1024);
  }
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
    config.source_roots = ["docs", "apps", "apps/web/docs", "services/api/docs"];
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
      ["apps/web/docs/journey.md", "web/journey"],
      ["docs/product.md", "orient/product"],
      ["services/api/docs/contract.md", "api/contract"],
    ]);

    await mkdir(path.join(root, "packages/web"), { recursive: true });
    await rename(path.join(root, "apps/web/docs"), path.join(root, "packages/web/handbook"));
    config.source_roots = ["docs", "packages/web/handbook", "services/api/docs"];
    config.layers[1].prefixes = ["packages/web/handbook"];
    await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
    commitFixture(root, "move web documentation root");
    runAdapter(root);
    const moved = JSON.parse(await readFile(path.join(root, ".portal/generated/evidence.json"), "utf8"));
    assert.equal(moved.pages.find((page) => page.source_path.endsWith("journey.md")).route, "web/journey");
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("different source roots cannot claim the same semantic route", async () => {
  const root = await portalFixture();
  try {
    await mkdir(path.join(root, "archive"));
    await writeFile(path.join(root, "docs/guide.md"), "# Current guide\n");
    await writeFile(path.join(root, "archive/guide.md"), "# Archived guide\n");
    const configPath = path.join(root, "portal.config.json");
    const config = JSON.parse(await readFile(configPath, "utf8"));
    config.source_roots = ["docs", "archive"];
    await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
    commitFixture(root, "route collision fixture");
    assert.match(runAdapter(root, false).stderr, /route collision/);
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
    const landing = await readFile(path.join(root, "src/content/docs/reference/index.md"), "utf8");
    assert.match(landing, /^title: "Reference"$/m);
    assert.doesNotMatch(landing, /^# Reference$/m);
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
      const output = await readFile(path.join(root, "src/content/docs/reference/guide (one).md"), "utf8");
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
    await writeFile(path.join(root, "docs/decisions/ADR-0001.md"), "---\nid: ADR-0001\ntitle: Decision\n---\n\n# Decision\n\n## Outcome\n");
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
  for (const link of ["missing.md", "../../outside.png", "#missing-heading"]) {
    const root = await portalFixture();
    try {
      await writeFile(path.join(root, "docs/guide.md"), `# Guide\n\n[Broken](${link})\n`);
      commitFixture(root, "add broken link");
      assert.match(runAdapter(root, false).stderr, /does not exist|stay beneath|fragment/);
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

async function selfContainedPortalFixture() {
  const root = await mkdtemp(path.join(starterRoot, ".portal-test-runtime-"));
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
    layers: [
      { id: "orient", label: "Orient", description: "Orientation", paths: ["docs/product.md"] },
      { id: "system", label: "System", description: "System", prefixes: ["docs/decisions"] },
      { id: "reference", label: "Reference", description: "Reference", fallback: true },
    ],
    base: "/",
  });
  await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
  await writeFile(path.join(root, "docs/seed.md"), "# Seed\n");
  git(root, ["init", "-q"]);
  git(root, ["config", "user.email", "portal-tests@codeflow.invalid"]);
  git(root, ["config", "user.name", "CodeFlow portal tests"]);
  commitFixture(root, "initialize self-contained fixture");
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

function runLocalAdapter(root, expectSuccess = true) {
  const result = spawnSync(process.execPath, [path.join(root, "scripts/adapter.mjs")], { cwd: root, encoding: "utf8" });
  if (expectSuccess) assert.equal(result.status, 0, result.stderr);
  else assert.notEqual(result.status, 0, result.stdout);
  return result;
}

async function waitUntil(predicate, timeoutMs = 5_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`condition was not met within ${timeoutMs}ms`);
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
