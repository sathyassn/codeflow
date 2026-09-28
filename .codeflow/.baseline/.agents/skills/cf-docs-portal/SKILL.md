---
name: cf-docs-portal
description: Adopt, configure, build, update, transfer, or verify the optional CodeFlow documentation portal for a consuming repository. Use when a project asks for a layered repository guide, browsable technical documentation, source-linked views, Markdown twins or llms.txt, or maintenance and ownership of an adopted portal. Do not use for product UI design, a transient response surface, or ordinary Markdown-only documentation.
---

# cf-docs-portal: repository guide utility

Create and maintain a guide to the project as it stands over repository-owned
documentation. The portal is a derived utility, never a second source of
truth. Product behavior, architecture, capabilities, decisions, specs, epics,
tasks, and code remain in their established files; records are pointed to as
folders, never listed page by page.

This is a supporting flow inside `cf-model-orchestrator` for non-trivial work.

Docs for **this or any consuming repo** reuse the same utility design
system as `cf-present`: author repository sources; the portal applies
tokens, altitude and the figure grammar. Do not copy present Comment chrome
or lifecycle.

**Before theming, layering, or authoring portal pages, load in order:**

1. [resources/explanation-method.md](resources/explanation-method.md):
   reader, altitude, carrier, draft, check
2. [resources/utility-presentation-system.md](resources/utility-presentation-system.md):
   shared doctrine (ADR-0063)
3. [resources/figure-grammar.md](resources/figure-grammar.md): families,
   rules, altitude (ADR-0068)
4. [resources/design-system/](resources/design-system/README.md): the kit
5. [references/visual-craft.md](references/visual-craft.md): portal profile
6. Other references as the task requires

Pass the page composition gate in `references/visual-craft.md`. A prose-card
wall, a marketing layout, or a page that is the source Markdown re-rendered
fails this skill: the portal composes sources visually.

Apply `cf-design` only when the **consuming product** needs experience
direction, never to utility portal themes; utility tokens and Starlight
components are never product brand authority; product DS stays out of the
portal. Apply `cf-editorial-review` to substantive copy.

## 1. Decide whether to adopt

Use the portal when layered browsing, cross-linking, or machine-readable twins
materially improve understanding. Keep Markdown-only docs when the repository
is tiny, short-lived, or lacks the durable sources to justify a build
dependency. Record an honest refusal, not decorative or empty pages.

Adoption is explicit:

```sh
codeflow portal setup --path <repository-relative-directory>
```

The command works offline from the CodeFlow binary, writes the starter only at
the chosen path, and records adoption in `.codeflow/docs-portal.json`.
Non-adopters receive no Node workspace or lockfile; dependency installation is a
separate operation. Repeated setup and ordinary `codeflow update` replace only
unchanged managed files. Local runtime edits stop the entire portal update;
there is no source merge, conflict sidecar or implicit transfer. Other scaffold
changes may still proceed. Read [references/operations.md](references/operations.md)
before resolving drift, migrating legacy state or changing ownership.

## 2. Configure source-in-place

Edit the portal's user-owned `portal.config.json`. Name the repository root,
source roots, exclusions, title, description, base path, and utility theme.
Do not copy prose into a portal-only content authority. Generated
content under `.portal/generated/`, generated Starlight content, search data,
and build output are disposable.

Use the supported configuration/token seams and additional assets at unclaimed
paths; [supported customization](references/operations.md#supported-customization)
specifies the portal's own token schema and path resolution. Bundled files
remain managed even under `public/`. After adoption,
missing project-owned configuration stays absent and is reported, not reseeded.
For bespoke runtime or layout changes, explain the ongoing maintenance cost and
confirm whole-runtime ownership before `codeflow portal transfer --confirm`.
Transfer preserves current edits and intentional deletions; later setup/update
does not upgrade or repair that runtime. Never transfer merely to clear a failed
update. Keep valid evidence, dependency review and rendered-quality checks after
transfer; ownership is not a validation exemption.

Read [references/information-architecture.md](references/information-architecture.md)
before choosing source roots, layers, or more than one portal;
[references/content-contract.md](references/content-contract.md) before changing
source interpretation, IDs, relationships, provenance, or stale-page behavior;
[references/operations.md](references/operations.md) before installing
dependencies, publishing, upgrading, or collecting acceptance evidence.

## 3. Build one layered route system

Expose progressive depth where sources support it (altitude grammar):

```text
purpose and mental model                         (concept)
  -> capabilities and journeys
    -> architecture and boundaries in effect     (architecture)
      -> reference, operations and evidence      (technical)
records: decisions, epics, tasks and specs are pointed to as folders
```

Keep navigation predictable. Match an established project voice;
otherwise write copy by `cf-editorial-review/references/copy-guide.md`.

### Visual craft (utility presentation system, mandatory)

Page shape:
[resources/portal-page-shape.example.md](resources/portal-page-shape.example.md).

- **Figures** lead every altitude panel and how-to section, one family
  each, drawn by the figure block; text in boxes is not a figure.
- **Explanatory sources** author the altitude trio with a figure framed by
  short plain prose in every panel; verification fails a trio page showing
  more than one panel or an explanatory page with no trio.
- **Records** are one generated pointer page of folders, not portal pages;
  the adapter's records switch stays off.
- **Type roles:** display / prose / label / mono-evidence. Display controls
  face and scale independently of skin; content adds no ad-hoc font stacks.
- **Themes:** Graphite, Slate and Sage are independent of font; readers
  switch skin, face, scale, and appearance in the Display panel. A project may
  adapt the utility once from its brand; never feed portal palette/type/
  components back into the product design system.
- **Motion:** minimal; meaning at rest; respect reduced motion; no wrong-mode
  flash on first paint.

For a monorepo, keep global orientation and shared concepts above area or
surface drill-down. Use multiple `source_roots` and layer paths to expose that
graph; do not dump one navigation folder per package or duplicate shared prose.

## 4. Preserve evidence and safety

Run the locked workflow from the adopted portal root:

```sh
npm run deps:install
npm run check
npm run build
npm run browser:verify
codeflow validate --portal <repository-relative-directory>
```

The managed installer verifies the lockfile's lifecycle-script inventory and
runs the locked install with dependency scripts disabled under a non-secret
environment allowlist. Do not replace it with plain `npm ci` or an ad-hoc
`npm rebuild`; follow the reviewed-exception process in `references/operations.md`
if a pinned dependency genuinely requires a lifecycle script.

The Node adapter alone derives the content graph and evidence manifest. The
Rust validator executes no project code and writes nothing; it verifies the
manifest's claims against repository and output bytes. Version 1 always escapes
raw source HTML outside code fences; it has no configuration bypass.

Never claim a page, relationship, snippet, search entry, version, or source is
current without the evidence manifest. Never claim accessibility, browser,
design fidelity, or Pagefind behavior from the manifest alone.

## 5. Verify the experience

Map checks to accepted intent and affected journeys. Before acceptance, read
[operations: experience qualification](references/operations.md#experience-qualification)
for the required browser, accessibility, theme, navigation and cleanup matrix.
`npm run browser:verify` is the reusable isolated headless default, not an
excuse to skip rendered judgment. Missing platform evidence stays unverified.

The directly invoked Claude judgment primary reviews the rendered experience,
content hierarchy, and source fidelity. Codex verifies implementation,
deterministic claims, failure cases, and browser evidence. Both review the exact
integrated result under the normal orchestration contract.

## Completion

Return the adopted path and version, source roots, generated/evidence paths,
commands and exact results, rendered-review evidence, known exclusions or stale
pages, dependency/security findings, and cleanup confirmation. Distinguish
algorithmic verification from agentic judgment; neither substitutes for the
other.
