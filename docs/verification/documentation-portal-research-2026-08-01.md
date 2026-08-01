# Documentation portal research — 2026-08-01

This record contains the independent Codex research and prototype lane for
TSK-008. It is an input to the required Claude-led settlement, not a final
design approval. Codex completed this pass without reading Claude's conclusion.
The later cross-model decision is recorded separately in
[`documentation-portal-settlement-2026-08-01.md`](documentation-portal-settlement-2026-08-01.md);
that record and SPC-005 supersede this lane's provisional recommendation.

## What the portal has to explain

CodeFlow serves at least three overlapping audiences:

- a project adopter deciding what CodeFlow changes and how to start;
- a contributor locating a capability, decision, command, or source module;
- a maintainer checking why behavior exists, how sources flow into distributed
  artifacts, and what evidence supports a claim.

A useful portal therefore needs more than a polished README or a generated API
reference. It must connect purpose, workflow, architecture, implementation,
and evidence without turning each layer into a separate source of truth.

```text
why CodeFlow exists
        |
        v
what users can do -----> task-oriented guidance
        |
        v
how the system works --> architecture and decisions
        |
        v
where it is built -----> modules, files, commands, tests
        |
        v
what proves the claim -> source links, versions, verification
```

## External research

The review compared patterns rather than copying one site's appearance. Sources
were accessed on 2026-08-01.

### Documentation and knowledge experiences

| Source | Useful pattern | Boundary for CodeFlow |
|---|---|---|
| [Linear Docs](https://linear.app/docs) | Restrained navigation, clear popular paths, low visual noise | Its category-card landing is too generic to become the portal's main explanatory structure |
| [Stripe Docs](https://docs.stripe.com/get-started) | Strong task/product routes, contextual navigation, search, source-oriented affordances, explanatory product visuals | The scale and product taxonomy exceed CodeFlow's needs; copy structure, not surface density |
| [Railway Docs](https://docs.railway.com/overview/the-basics) | Guide-oriented discovery, source links, recency signals | A guide catalog alone does not expose CodeFlow's source and distribution architecture |
| [GitHub Docs](https://docs.github.com/en/get-started/using-github-docs/about-versions-of-github-docs) | Explicit version context and one-source conditional documentation | CodeFlow should not create copied version trees unless a real compatibility requirement emerges |
| [Diátaxis](https://diataxis.fr/) | Separates tutorials, how-to guides, reference, and explanation by user need | It is an information-architecture lens, not a mandatory folder taxonomy or page template |
| [C4 model](https://c4model.com/diagrams) | Context-to-code zoom and diagrams that stand on their own | Use only levels that clarify a real relationship; generated diagram volume is not a goal |

The recurring useful idea is layered orientation with task paths, not a single
hierarchical document tree. Search is necessary but cannot compensate for weak
information architecture. Source and version context should stay visible near
claims rather than living on a separate provenance page.

### Maintained framework candidates

The framework review used official documentation, current package metadata, a
clean install, an audit, and a minimal production build where noted.

| Candidate | Strength | Cost or risk | Codex assessment |
|---|---|---|---|
| [Astro Starlight](https://starlight.astro.build/) | Static-first docs, Markdown/MDX/Markdoc, built-in local Pagefind search, i18n, component and CSS overrides | Current starter installed 363 packages and 224 MB; custom composition must not fight theme internals | Strongest provisional base if its default theme is treated as infrastructure, not the chosen design |
| [VitePress](https://vitepress.dev/) | Small conceptual surface, local MiniSearch, dead-link checking, optional MPA mode | Current dependency chain reported unresolved Vite/esbuild audit findings in this trial | Do not select while the current supported chain retains those findings |
| [Docusaurus](https://docusaurus.io/docs/advanced/routing) | Mature versioning and i18n | Versioning copies documentation and adds build/editorial cost; heavier than current needs | Use only if true concurrent-version requirements are established |
| [Fumadocs](https://fumadocs.dev/docs/headless/source-api) | Clear content/core/UI separation and flexible source/search adapters | Next/React application surface raises build and maintenance cost | Reconsider if the portal needs unusually rich custom sources or application-like interaction |
| [Nextra](https://nextra.site/docs/docs-theme/start) | Convenient Next/React documentation shell | Theme and application coupling is more than the current portal needs | No present advantage over Starlight or Fumadocs for this contract |
| [Pagefind](https://pagefind.app/) | Static, local search generated from built output | Search quality still depends on semantic page structure and metadata | Appropriate search primitive, whether directly or through Starlight |

Observed clean-build measurements are environmental evidence, not general
benchmarks:

| Trial | Install | `node_modules` | Production build | Audit observation |
|---|---:|---:|---:|---|
| Starlight 0.41.6 / Astro 7.0.2 | about 30 s | 224 MB | 4.45 s cold; 1.61 s warm; four pages and Pagefind | Starter's Sharp range selected a vulnerable release; pinning current Sharp 0.35.3 cleared the audit and preserved the build |
| VitePress 1.6.4 | about 6 s | 98 MB | 2.45 s wall time | Three findings in the supported Vite/esbuild chain, including one high, with no available fix in that chain |

These results must be rerun from a locked implementation branch. They do not
authorize carrying a temporary dependency override indefinitely.

## Prototype comparison

Rendered evidence and runnable sources are under
[`tsk-008-portal-prototypes/`](tsk-008-portal-prototypes/).

| Criterion | Guided path | System atlas | Evidence notebook |
|---|---|---|---|
| Hierarchy | Strongest first-use orientation and progressive depth | Strongest system overview for an experienced contributor | Strongest sustained reading and evidence adjacency |
| Navigation and search | Reader path, section navigation, search | Architecture levels, component inspector, search | Chapter navigation, in-page structure, search |
| Source traceability | Persistent context rail and traceability spine | Source/evidence chain tied to selected system view | Sources sit beside explanatory chapters and reference material |
| Responsive behavior | Journey collapses cleanly; mobile menu was exercised | Dense diagrams require intentional horizontal access on narrow screens | Long-form content and native tables adapt predictably |
| Accessibility basis | Semantic regions, visible focus, native controls, reduced-motion rule | Native level/inspector controls; diagram labels remain textual | Semantic headings, list/tree structure, native command table |
| Language | Direct onboarding and task language | Compact technical labels and explanations | Calm editorial explanation without marketing voice |
| Visual/prose balance | Diagrams carry responsibility, sequence, and traceability | Architecture maps are the main explanatory object | Task graph, repository anatomy, and verification comparison break up prose |
| Maintainability | Moderate custom shell; mostly standard content patterns | Highest bespoke interaction and content-model cost | Lowest interaction cost; long-form editorial upkeep remains |
| Build cost | Compatible with a customized static docs framework | May require custom interactive islands and richer fixtures | Compatible with largely static Markdown-driven rendering |
| Product isolation | Utility identity is independent of consuming-product UI | Same | Same |

No direction is sufficient alone:

- Guided Path can become too linear for repeat contributors.
- System Atlas can privilege architecture over tasks and would be expensive if
  every page became an interactive map.
- Evidence Notebook can become a long-form archive unless task routes and
  search remain prominent.

## Provisional Codex recommendation

Use a hybrid information architecture, not a visual collage:

1. Make Guided Path the default orientation and task journey.
2. Embed the System Atlas pattern only where an architecture relationship or
   source flow benefits from direct exploration.
3. Apply the Evidence Notebook's source adjacency and editorial rhythm to
   explanation, decisions, and verification material.

Astro Starlight is the provisional technical baseline. Use it for static build,
content loading, local search, accessibility foundations, and extension points;
replace its generic surface composition with the settled CodeFlow utility
design. Keep interactive architecture views as isolated components so most
pages remain plain, portable content.

Claude subsequently critiqued this recommendation independently and led the
final settlement. The lanes converged on this hybrid structure and expanded
its exact records, traceability, theme, version, and agent-output contracts in
the durable settlement record.

## Source and version contract proposed for settlement

The portal is a derived view. It must not become another documentation store.

```text
approved repository sources
  |  Markdown, front matter, source manifest, code metadata
  v
source adapter + validation
  |  rejects missing, conflicting, stale, or orphaned mappings
  v
generated build model (disposable)
  |  source file + anchor + repository ref retained per page/claim
  v
static portal output
```

The implementation should:

- map every generated page to one or more authoritative repository sources;
- carry repository version/ref and source anchors into the rendered experience;
- fail on broken internal links, duplicate routes, orphaned generated pages,
  and missing required source/version metadata;
- report source changes that make a generated page stale;
- keep generated caches and deployed output disposable unless the project
  explicitly chooses to track them; and
- never infer history, rationale, ownership, or behavior from filenames alone.

## Verification performed

- All prototype HTML passed `html-validate` after correcting button types,
  landmark semantics, document type, and a simulated table.
- Local source targets linked by the prototypes returned HTTP 200 from the
  repository-root server.
- Desktop and 390 x 844 mobile renders were captured in light and dark modes.
- Browser interaction exercised visible theme controls, mobile navigation,
  search filtering, architecture-level switching, and component inspection.
- Browser accessibility snapshots exposed named landmarks, headings, links,
  tables, and controls; the tested search interaction produced no console
  warning or error.

Not verified: WCAG 2.2 AA conformance, measured color contrast, complete
keyboard traversal, screen-reader behavior, automated axe results, production
framework integration, content generation, cross-browser behavior, or runtime
performance. The implemented keyboard shortcut was not counted as observed
because the browser harness could not establish the required focused target.

## Risks and decisions still open

- Starlight is pre-1.0; custom overrides and dependency posture need a locked,
  maintained boundary and repeatable upgrade test.
- A source adapter is necessary, but an elaborate content graph before real
  pages exercise it would be speculative.
- Architecture views can go stale quickly; only sourced, useful views should
  be built, with stale-source checks.
- Monorepo scale, multiple release lines, and hosted search are cases to test,
  not assumptions to optimize for now.
- Theme tokens belong only to the portal utility. They must not influence or
  share runtime components with a consuming product or `cf-present`.
