// Page anatomy A1, type scale S2 and figure tones on a real portal build
// (TSK-444). One fixture repository goes through the normal adapter and Astro
// build: a page with an authored description, a section with its lead, a
// subsection and a toned companion figure, and a page with no description.
// The built site is read in Chromium, so what is checked is what the cascade
// gives the reader, not what the CSS text says.
import assert from "node:assert/strict";
import { readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { chromium } from "@playwright/test";
import { hardenedChildEnvironment } from "../scripts/process-environment.mjs";
import { serveBuiltSite } from "../scripts/site-server.mjs";
import { specimen } from "./page-shapes.mjs";
import { buildFixture, commitFixture, configureFixture, runLocalAdapter, selfContainedPortalFixture } from "./portal-fixture.mjs";

const DESCRIPTION = "How a change lands, and which plane stops it.";
const GUIDE = [
  "---",
  `description: "${DESCRIPTION}"`,
  "---",
  "",
  "# Landing guide",
  "",
  "## Where a change lands",
  "",
  "Every change lands through a pull request that four planes check.",
  "",
  "The body after the lead stays at body size.",
  "",
  "### The remote plane",
  "",
  "Remote branch protection is the boundary.",
  "",
].join("\n");
const PLAIN = "# Plain page\n\n## A section\n\nA page with no authored description.\n";
const LAYERS = [
  { id: "orient", label: "Orient", description: "Orientation", paths: ["docs/none.md"] },
  { id: "system", label: "System", description: "System", prefixes: ["docs/decisions"] },
  { id: "reference", label: "Reference", description: "Reference", fallback: true },
];

// The S2 ladder a reader must see: title > section > subsection > every block
// title, by computed size. Returns the violations, so a control can prove it
// sees one.
function orderViolations(sizes) {
  const violations = [];
  for (const name of ["h2", "h3", "blockTitles"]) if (!sizes[name]?.length) violations.push(`no ${name} measured`);
  if (typeof sizes.title !== "number") violations.push("no title measured");
  if (violations.length) return violations;
  const max = (values) => Math.max(...values);
  const min = (values) => Math.min(...values);
  if (!(sizes.title > max(sizes.h2))) violations.push(`title ${sizes.title} is not above section ${max(sizes.h2)}`);
  if (!(min(sizes.h2) > max(sizes.h3))) violations.push(`section ${min(sizes.h2)} is not above subsection ${max(sizes.h3)}`);
  if (!(min(sizes.h3) > max(sizes.blockTitles))) violations.push(`subsection ${min(sizes.h3)} is not above block title ${max(sizes.blockTitles)}`);
  return violations;
}

const MEASURE = () => {
  const sizes = (selector) => [...document.querySelectorAll(selector)].filter((node) => node.getClientRects().length).map((node) => parseFloat(getComputedStyle(node).fontSize));
  const lede = document.querySelector(".cf-title + .cf-lede");
  return {
    title: sizes(".cf-title")[0],
    lede: lede ? { text: lede.textContent.trim(), size: parseFloat(getComputedStyle(lede).fontSize) } : null,
    h2: sizes(".cf-main .sl-markdown-content h2"),
    lead: sizes(".cf-main .sl-markdown-content :is(h2, .sl-heading-wrapper:has(> h2)) + p"),
    body: [...document.querySelectorAll(".cf-main .sl-markdown-content p")].filter((node) => /^(The body after the lead|Remote branch protection is the boundary)/.test(node.textContent.trim())).map((node) => parseFloat(getComputedStyle(node).fontSize)),
    h3: sizes(".cf-main .sl-markdown-content h3"),
    blockTitles: sizes(".cf-fig-title"),
    tones: [...document.querySelectorAll("figure.cf-fig .cf-fig-svg--wide [data-cf-tone]")].map((group) => {
      const mark = group.querySelector("rect, path, circle");
      return { tone: group.getAttribute("data-cf-tone"), stroke: getComputedStyle(mark).stroke };
    }),
    cat: ["--cf-fig-cat1", "--cf-fig-cat2", "--cf-fig-accent"].map((name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim()),
  };
};

const hex = (value) => {
  const [r, g, b] = value.match(/\d+/g).map(Number);
  return `#${[r, g, b].map((channel) => channel.toString(16).padStart(2, "0")).join("")}`;
};

test("a built page opens with its title and authored summary, keeps the S2 order in the cascade and draws toned figures", { skip: process.platform === "win32", timeout: 300_000 }, async () => {
  const root = await selfContainedPortalFixture();
  try {
    await rm(path.join(root, "docs/seed.md"));
    await writeFile(path.join(root, "docs/guide.md"), GUIDE);
    await writeFile(path.join(root, "docs/plain.md"), PLAIN);
    const toned = await specimen("controls/layering-toned.json");
    toned.figure.facts = [{ claim: "The guide names the remote plane as the boundary", source: "docs/guide.md", derive: "the source text", check: { kind: "contains", text: "Remote branch protection is the boundary" }, value: true }];
    await writeFile(path.join(root, "figures-toned.json"), `${JSON.stringify(toned, null, 2)}\n`);
    await configureFixture(root, {
      layers: LAYERS,
      page_carriers: [],
      page_classes: [{ source: "docs/guide.md", class: "illustrated" }],
      figures: [{ declaration: "figures-toned.json", route: "reference/guide" }],
    });
    commitFixture(root, "page anatomy fixture");
    runLocalAdapter(root);
    buildFixture(root);

    // The adapter keeps its fallback description for the meta tags; the page
    // never shows it as a summary.
    const plainHtml = await readFile(path.join(root, "dist/reference/plain/index.html"), "utf8");
    assert.match(plainHtml, /<meta name="description" content="Repository source: docs\/plain\.md"/);
    assert.doesNotMatch(plainHtml, /class="cf-lede"/);
    const guideHtml = await readFile(path.join(root, "dist/reference/guide/index.html"), "utf8");
    assert.match(guideHtml, new RegExp(`<h1 class="cf-title" id="_top">Landing guide</h1>\\s*<p class="cf-lede">${DESCRIPTION}</p>`));
    assert.equal((guideHtml.match(/data-cf-tone="(?:accent|cat1|cat2)"/g) ?? []).length >= 8, true, "the toned figure keeps its tones through the build");

    const site = await serveBuiltSite({ directory: path.join(root, "dist") });
    const browser = await chromium.launch({ headless: true, env: hardenedChildEnvironment() });
    try {
      const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
      const expected = {
        light: { cat1: "#3860ac", cat2: "#722756" },
        dark: { cat1: "#9bbeff", cat2: "#cf82ae" },
      };
      for (const mode of ["light", "dark"]) {
        for (const scale of ["compact", "default", "large"]) {
          await page.goto(`${site.origin}/reference/guide/`, { waitUntil: "networkidle" });
          await page.evaluate(([theme, step]) => {
            document.documentElement.setAttribute("data-theme", theme);
            document.documentElement.setAttribute("data-cfp-scale", step);
          }, [mode, scale]);
          const sizes = await page.evaluate(MEASURE);
          const where = `${mode} ${scale}`;
          assert.equal(sizes.lede?.text, DESCRIPTION, `${where}: the authored summary sits under the title`);
          assert.deepEqual(orderViolations(sizes), [], `${where}: ${JSON.stringify(sizes)}`);
          assert.ok(sizes.lead.length >= 1 && sizes.lead.every((size) => size === sizes.lede.size), `${where}: a section lead takes the lede size: ${JSON.stringify(sizes)}`);
          assert.ok(sizes.body.length === 2 && sizes.body.every((size) => size < sizes.lede.size), `${where}: body text after the lead stays below it: ${JSON.stringify(sizes)}`);
          if (scale === "default") {
            assert.deepEqual([sizes.title, sizes.lede.size, sizes.h2[0], sizes.h3[0], sizes.blockTitles[0]], [34, 19, 24, 18, 15], where);
          }
          const strokes = Object.fromEntries(sizes.tones.map(({ tone, stroke }) => [tone, hex(stroke)]));
          assert.equal(strokes.cat1, expected[mode].cat1, `${where}: cat1 ink`);
          assert.equal(strokes.cat2, expected[mode].cat2, `${where}: cat2 ink`);
          assert.equal(strokes.accent, sizes.cat[2], `${where}: the governing region keeps the accent`);
        }
      }

      // Negative control: a later rule that lifts a subsection to display
      // size wins the cascade, and the order check must see it.
      await page.addStyleTag({ content: ".cf-main .sl-markdown-content h3 { font-size: var(--cf-fs-display); }" });
      const broken = await page.evaluate(MEASURE);
      assert.ok(orderViolations(broken).some((violation) => violation.startsWith("section")), `the cascade control was not caught: ${JSON.stringify(broken)}`);

      await page.goto(`${site.origin}/reference/plain/`, { waitUntil: "networkidle" });
      const plain = await page.evaluate(MEASURE);
      assert.equal(plain.lede, null, "a page with no authored description shows no summary");
      assert.equal(plain.title, 34);
    } finally {
      await browser.close();
      await site.close();
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
