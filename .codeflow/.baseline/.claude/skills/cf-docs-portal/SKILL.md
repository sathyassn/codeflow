---
name: cf-docs-portal
description: Adopt, configure, build, update, or verify the optional CodeFlow documentation portal for a consuming repository. Use when a project asks for a layered repository guide, browsable technical documentation, source-linked conceptual and implementation views, machine-readable Markdown twins or llms.txt, or maintenance of an already adopted portal. Do not use for product UI design, a transient response surface, or ordinary Markdown-only documentation.
---

# cf-docs-portal — repository guide utility

Create and maintain a navigable view over repository-owned documentation. The
portal is a derived utility, never a second source of truth. Product behavior,
architecture, capabilities, decisions, specs, epics, tasks, and code remain in
their established files.

This is a supporting flow inside `cf-model-orchestrator` for non-trivial work.

Docs for **this or any consuming repo** reuse the same utility design
system as `cf-present`: author repository sources; the portal applies
tokens, altitude, and stage grammar. Do not clone the design-exploration
board or copy present Comment chrome.

**Before theming, layering, or authoring portal pages, load in order:**

1. [resources/utility-presentation-system.md](resources/utility-presentation-system.md)
   — **canonical** utility presentation system (shared with `cf-present`)
2. [references/visual-craft.md](references/visual-craft.md) — portal checklist
3. Other references below as the task requires

Pass the portal composition gate in the canonical resource. A prose-card wall
or a marketing layout fails this skill.

Apply `cf-design` only when the **consuming product** needs experience
direction, never to utility portal themes. Portal themes, Starlight
components, and utility tokens are never product brand authority, and
product DS stays out of the portal. Apply `cf-editorial-review` to
substantive explanatory copy.

## 1. Decide whether to adopt

Use the portal when layered browsing, cross-linking, or machine-readable twins
materially improve understanding. Keep Markdown-only docs when the repository
is tiny, short-lived, or lacks the durable sources to justify a build
dependency. Record an honest refusal instead of decorative or empty pages.

Adoption is explicit:

```sh
codeflow portal setup --path <repository-relative-directory>
```

The command works offline from the CodeFlow binary, writes the starter only at
the chosen path, and records adoption in `.codeflow/docs-portal.json`.
Non-adopters receive no Node workspace or lockfile. Repeated setup and ordinary
`codeflow update` reconcile managed starter files without clobbering project
changes; resolve any reported `<path>.codeflow-<hash>.new` conflict sidecar
deliberately.

## 2. Configure source-in-place

Edit the portal's user-owned `portal.config.json`. Name the repository root,
source roots, exclusions, title, description, base path, and utility theme.
The starter defaults to a `docs/` graph. Add a `project-management` source
root and Records layer only when those files exist.
Do not copy prose into a portal-only content authority. Generated
content under `.portal/generated/`, generated Starlight content, search data,
and build output are disposable.

Read
[references/information-architecture.md](references/information-architecture.md)
before choosing source roots, layers, or more than one portal. It defines the
single-project and monorepo defaults and the narrow reasons to split.
Read [references/content-contract.md](references/content-contract.md) before
changing source interpretation, IDs, relationships, provenance, or stale-page
behavior. Read [references/operations.md](references/operations.md) before
installing dependencies, publishing, upgrading, or collecting acceptance
evidence.

## 3. Build one layered route system

Expose progressive depth where sources support it (altitude grammar):

```text
purpose and mental model                         (concept)
  -> capabilities and journeys
    -> architecture, decisions, and work         (architecture)
      -> technical source references and evidence (technical)
```

Keep navigation predictable and searchable. Prefer plain language, descriptive
titles, concise prose, and bullets when they improve scanning. Match an
established project voice when it exists; otherwise use calm, direct,
third-person documentation language. Avoid cryptic headings, invented
personality, gratuitous emoji, and promotional language.

### Visual craft (utility presentation system—mandatory)

Normative detail:
[resources/utility-presentation-system.md](resources/utility-presentation-system.md)
and [references/visual-craft.md](references/visual-craft.md). Page shape example:
[resources/portal-page-shape.example.md](resources/portal-page-shape.example.md).

- **Visuals** only when they clarify relationship, hierarchy, state, or flow.
  Text inside decorated boxes is not a visual explanation.
- **Architecture-layer sources** each author the altitude trio plus a stage —
  full-width labeled structure, not caption micro-boxes; verification fails a
  trio page that shows more than one panel.
- **Type roles:** display / prose / label / mono-evidence; themes own faces and
  scale. Do not ship ad-hoc font stacks in content.
- **Themes:** `signal`/`folio` map to utility skins instrument/ink; readers
  switch skin, face, scale, and appearance in the Display panel. A project may
  adapt the utility once from its brand; never feed portal palette/type/
  components back into the product design system.
- **Motion:** minimal; meaning at rest; respect reduced motion and no wrong-mode
  flash on first paint.
- **No present Comment lifecycle** in portal chrome.

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
if a future pinned dependency genuinely requires a lifecycle script.

The Node adapter alone derives the content graph and evidence manifest. The
Rust validator executes no project code and writes nothing; it verifies the
manifest's claims against repository and output bytes. Version 1 always escapes
raw source HTML outside code fences; it has no configuration bypass.

Never claim a page, relationship, snippet, search entry, version, or source is
current without the evidence manifest. Never claim accessibility, browser,
design fidelity, or Pagefind behavior from the manifest alone.

## 5. Verify the experience

Map checks to the accepted intent and affected journeys. At minimum for a
material portal change, verify:

- locked build, deterministic adapter tests, and the Rust evidence verifier;
- navigation, search, source links, Markdown twins, `llms.txt`, error handling,
  strict-ID previews (hover, focus, touch, keyboard, Escape, and ordinary link
  navigation), and empty/tiny/monorepo fixtures as applicable;
- keyboard order, focus visibility, semantics, contrast, target size, zoom,
  reduced motion, and responsive behavior against WCAG 2.2 AA;
- both fallback themes in light and dark mode, including persisted preference
  with no incorrect-mode flash;
- Chromium, Firefox, and WebKit journeys when available, with unavailable
  platform evidence reported rather than inferred;
- task-owned browser state, ports, test data, traces, and screenshots, followed
  by verified resource cleanup.

`npm run browser:verify` is the reusable headless default: a task-owned
loopback preview under the workflow lease; journeys, routes, and search terms
derived from validated configuration and generated evidence; Chromium,
Firefox, and WebKit in sequence with separate temporary profiles; WCAG 2.2 AA axe
rules plus the §5 journeys, console failures, and remote requests; bounded
hashed screenshot/trace evidence and verified server and profile teardown
(mechanics and identity binding: `references/operations.md`). Set a unique
`PORTAL_BROWSER_RUN` for concurrent tasks. Use a headed task-owned browser only
for a finding the headless run cannot settle, never the operator's profile or
view.

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
