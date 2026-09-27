# UI and design verification

`DESIGN_INTENT` is proportional. A cosmetic correction records `N/A` with the
unchanged accepted direction; a bounded change inside an established design
system records `conform` and names that authority. A new or materially reshaped
user-facing surface applies `cf-design` and records the creator intent,
audience/job/context evidence, experience target, language and voice,
applicable appearance modes, systems and operator direction, proportionate
research/options, subject and governing idea, user action and primary
composition, settled direction, earned system scope, accessibility target,
pre-registered review rubric, and fidelity plan. Collapse irrelevant dimensions
instead of filling a template. While the operator owns an open material
direction or composition, an operator-visible rendered board settles it before
implementation; agreement between model seats is not that decision. Do not
create a separate design document unless the project needs a durable product or
design-system decision at its normal spec/ADR altitude.

Design exploration remains bounded after direction selection: vary only a
named unresolved choice that can materially change the outcome, retain
proportionate source/rights/consent/transformation and product-use evidence for
material references or assets, never send private material to an external
service without explicit authority, and bind material feedback to the exact
reviewed version plus accepted/rejected rationale in Plan vN+1.

A product, UX, UI, interaction, or visual-design finding anchored in the brief,
settled `DESIGN_INTENT`, applicable accessibility target, or observed user
behavior is graded by this same materiality rule; it is not demoted merely
because it concerns design. An unanchored aesthetic preference remains a minor,
non-blocking observation.

For web UI, exercise real rendered behavior through the active native harness
with repository tests plus a supported Playwright MCP or official CLI/skill.
Browser headless mode is valid for routine deterministic E2E and CI: it still
provides DOM/accessibility state, assertions, screenshots, traces, console
output, and network evidence, and is unrelated to the prohibited headless
peer-model transport. Use headed/UI mode when live observation, browser chrome,
interaction debugging, or environment-specific rendering is material. For
native, mobile, desktop, browser-chrome, or other surfaces outside Playwright's
controlled page/context, prefer a surface-specific driver and use Computer Use
only when no narrower driver reaches the surface.

On an interactive user-facing change, split verification by seat. Default UI
assignment is Claude as responsible primary and executor and Codex as reviewer.
The executor performs the **implementer check** against `DESIGN_INTENT`
(design-system fit, states, Playwright or the platform driver). The named
independent cross-lineage reviewer
independently **QAs** the changed surface and affected journeys through
Computer Use. When that reviewer is Codex, use official app-server Computer
Use through a qualified official native client (`cf-delegate`); verify that
the chosen route actually exposes it. When Claude reviews a Codex-authored UI unit,
Claude performs Computer Use QA in Claude Code; Codex does not QA its own
unit. The sentence above about Computer Use as a *driver of last resort*
still holds for deterministic E2E. Playwright remains the deterministic web
driver; Computer Use is the QA exploration layer, not a default web driver.
Scope is every interactive
control those journeys expose (buttons, links, tabs, menus, disclosures,
fields, drag handles, scroll containers) with pointer (click, drag, scroll),
keyboard (tab order, activation, shortcuts), and applicable touch/gesture.
Cover applicable viewports including sizes where composition changes, not
only the narrowest and widest. Do not exhaust the entire product unless the
work is a full-surface redesign. Unavailable Computer Use is a declared
limitation, not a pass of interactive QA.

Check at least:

- before any concurrent browser work, allocate a task/run owner and isolate
  every mutable resource it uses: a fresh browser context/profile (prefer
  Playwright MCP `--isolated`, or a task-specific user-data directory only when
  persistence is required); a unique MCP/service endpoint when the transport
  listens on a port; non-overlapping loopback application/service endpoints;
  a per-run and, when parallel, per-worker test-data namespace/account/schema;
  and an absolute task-scoped output/report/screenshot/trace directory;
- define allocation, readiness, retention, and teardown before launch. Use
  fixture/finalizer cleanup for data and services, stop only owned process
  groups/containers/endpoints, close contexts and browsers, then verify ports
  and task-owned resources were released. Preserve failure evidence under the
  project's retention policy; delete secrets and successful disposable state;
- never attach to the operator's existing browser, default user profile, tabs,
  or active desktop. A materially necessary headed run launches a test-owned
  browser/profile and must not take over the user's current view. Computer Use
  runs only in a dedicated test desktop/session or with explicit operator
  control; if that isolated surface is unavailable, record the limitation
  rather than hijacking the user's session;
- the consuming project owns its safe port allocator/range, namespace format,
  artifact root, retention, and teardown commands. CodeFlow supplies this
  resource contract, not universal port numbers or stack-specific scripts;

- the upstream `DESIGN_INTENT`, including its valid `N/A` or `conform` path,
  before treating a design as approved;
- the project's existing design system and component library before adding a
  new pattern;
- recurring foundations or tokens, accessible primitives, reusable
  application-specific components, and their composition into views or pages;
- minimal explicit interaction state with one clear owner and framework-native
  data flow;
- no repeated one-off styling, state logic, or components when current reuse is
  evidenced; no new design system or higher-order abstraction for a one-off
  surface without such evidence;

- the approved design, its fidelity to the settled intent, and the primary user
  journeys;
- navigation, actions, guidance, validation, loading, empty, error, disabled,
  success, destructive, and recovery copy states where applicable;
- visual/verbal coherence and terminology against the project voice; localized
  variants, writing direction (LTR/RTL), and text expansion before claiming
  localization quality;
- applicable light, dark, high-contrast, system-following, manual-override,
  persistence, reduced-motion, imagery, and data-visualization behavior without
  an incorrect-mode flash;
- responsive/adaptive behavior at relevant sizes, including the intermediate
  viewports where composition actually changes;
- keyboard navigation, focus, labels, contrast, and other applicable
  accessibility requirements against the project's target; for web surfaces,
  default to WCAG 2.2 AA unless a stronger target or a different
  surface-appropriate target with recorded rationale governs;
- validation, destructive-action safeguards, and recovery;
- console/runtime errors and network failures.

Match evidence to the claim: locators, accessibility snapshots, and web-first
assertions for structure and behavior; screenshots or same-environment visual
comparisons for appearance; console/network evidence for runtime behavior; and
traces on failure or first retry, or a bounded trace for an ambiguous
exploratory flow. A screenshot alone does not prove interaction or
accessibility, automated accessibility evidence is partial, and tracing every
green run wastes resources. A code-only review is not UI verification.
Compare the rendered result with the settled intent using only applicable
dimensions: governing idea and composition, hierarchy, interaction, content,
visual/verbal coherence, type and colour roles, layout, spacing, imagery,
density, motion, states, appearance modes, and platform fit. Distinguish an
approved improvement or evidenced implementation constraint from unjustified
drift. A changed design contract or material direction creates Plan vN+1; a
reviewer's unsupported taste does not.
