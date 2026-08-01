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
Apply `cf-design` to settle the portal's own material experience direction,
without turning the utility's themes or stack into product design authority.
Apply `cf-editorial-review` to substantive explanatory copy.

## 1. Decide whether to adopt

Use the portal when layered browsing, cross-linking, or machine-readable twins
materially improve understanding. Keep Markdown-only docs when the repository
is tiny, short-lived, or lacks enough durable source material to justify a
build dependency. Record an honest refusal instead of generating decorative or
empty pages.

Adoption is explicit:

```sh
codeflow portal setup --path <repository-relative-directory>
```

The command works offline from the CodeFlow binary, writes the starter only at
the chosen path, and records adoption in `.codeflow/docs-portal.json`.
Non-adopters receive no Node workspace or lockfile. Repeated setup and ordinary
`codeflow update` reconcile managed starter files without clobbering project
changes; resolve any reported `.new` conflict deliberately.

## 2. Configure source-in-place

Edit the portal's user-owned `portal.config.json`. Name the repository root,
source roots, exclusions, title, description, base path, and utility theme.
Do not copy prose into a portal-only content authority. Generated
content under `.portal/generated/`, generated Starlight content, search data,
and build output are disposable.

Read [references/content-contract.md](references/content-contract.md) before
changing source interpretation, IDs, relationships, provenance, or stale-page
behavior. Read [references/operations.md](references/operations.md) before
installing dependencies, publishing, upgrading, or collecting acceptance
evidence.

## 3. Build one layered route system

Expose progressive depth where sources support it:

```text
purpose and mental model
  -> capabilities and journeys
    -> architecture, decisions, and work
      -> technical source references and evidence
```

Keep navigation predictable and searchable. Use visuals only when they clarify
relationships, hierarchy, state, or flow; text inside decorated boxes is not a
visual explanation. Prefer plain language, descriptive titles, concise prose,
and bullets when they improve scanning. Avoid cryptic headings, invented
personality, gratuitous emoji, and generic promotional language. Match an
established project voice when it exists; otherwise use calm, direct,
third-person documentation language.

The bundled `signal` and `folio` themes are utility fallbacks, each supporting
light and dark preference. A project may adapt the utility one way from its own
brand. The portal must never feed a fallback palette, typography choice,
component, or framework back into the product's design system.

## 4. Preserve evidence and safety

Run the locked workflow from the adopted portal root:

```sh
npm ci
npm run check
npm run build
codeflow validate --portal <repository-relative-directory>
```

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
  and empty/tiny/monorepo fixtures as applicable;
- keyboard order, focus visibility, semantics, contrast, target size, zoom,
  reduced motion, and responsive behavior against WCAG 2.2 AA;
- both fallback themes in light and dark mode, including persisted preference
  with no incorrect-mode flash;
- Chromium, Firefox, and WebKit journeys when available, with unavailable
  platform evidence reported rather than inferred;
- task-owned browser state, ports, test data, traces, and screenshots, followed
  by verified resource cleanup.

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
