// The portal's page classes and what each class must show, declared once.
// `browser-verify.mjs` reads the rules from here and applies them to every
// page the adapter produced, so coverage is per source: a compliant page can
// never stand in for a noncompliant one, and no rule is inferred from a source
// path. The rules restate docs/architecture/utility-presentation.md and
// ADR-0064; changing what a class requires means changing this table.

// The altitude trio (ADR-0053). The adapter renders these depth-2 sections as
// one tablist, and an explanatory page carries all three, not a subset.
export const ALTITUDE_PANELS = Object.freeze(["concept", "architecture", "technical"]);

// The elements a panel can carry, counted inside the panel that owns them so
// that a carrier in a neighbouring panel never answers for a missing one. A
// figure is a grammar figure (`figure.cf-fig`); a `cf-stage` is counted as a
// stage and is the flow interim, never the figure an altitude demands.
export const CARRIER_ELEMENTS = Object.freeze(["figure", "stage", "table", "list", "pre"]);

// What each panel of the trio must carry. Every altitude demands a figure;
// Technical also demands a table, which is an additional carrier and never a
// substitute for the figure (figure-grammar.md section 3).
export const PANEL_CARRIERS = Object.freeze({
  concept: Object.freeze({ demand: "a figure", requires: Object.freeze(["figure"]) }),
  architecture: Object.freeze({ demand: "a figure", requires: Object.freeze(["figure"]) }),
  technical: Object.freeze({ demand: "a figure and a table", requires: Object.freeze(["figure", "table"]) }),
});

// A page whose subject is itself a carrier declares that in the portal
// configuration, per source, and is then held to the carrier it declared: a
// checklist's Technical panel is a disposition list beside its figure, and a
// table would only restate it. The figure is never let off.
export const PANEL_CARRIER_ALTERNATES = Object.freeze({
  technical: Object.freeze({ list: Object.freeze(["figure", "list"]) }),
});

// Pass-through takes one reason from this closed set; no-relationship also
// records the design primary's judgment in a note.
export const PAGE_CLASS_REASONS = Object.freeze(["accepted-record", "governance", "no-relationship"]);

// The derived lookups the adapter can generate with a fidelity check.
export const DERIVED_LOOKUPS = Object.freeze(["capability-registry"]);

// The generated record pointer page (ADR-0064) is one table of folders. The
// records themselves are repository files, never portal pages.
export const RECORD_POINTER_COLUMNS = Object.freeze(["Folder", "Purpose", "Count", "Repository"]);

// The pointer page is generated from configuration, so the failure names the
// configuration rather than a source file that does not exist.
export const RECORD_POINTER_SOURCE = "portal.config.json records pointers";

// Every class carries the utility chrome. Only the classes differ below it.
const UTILITY_CHROME = Object.freeze([
  Object.freeze({
    id: "single-heading",
    demand: "exactly one visible primary heading",
    unmet: (observation) => observation.headings === 1 ? null : `${observation.headings} visible h1 element(s)`,
  }),
  Object.freeze({
    id: "display-control",
    demand: "a visible Display control",
    unmet: (observation) => observation.displayControls > 0 ? null : "no visible Display control",
  }),
  Object.freeze({
    id: "no-present-chrome",
    demand: "no present Comment chrome",
    unmet: (observation) => observation.commentChrome === 0 ? null : `${observation.commentChrome} present Comment element(s)`,
  }),
]);

// One requirement per panel of the trio. A panel the page never rendered is
// left to the trio rule above, which already names it, so a missing panel is
// reported once rather than twice.
function panelCarrierRequirement(panel) {
  return Object.freeze({
    id: `${panel}-carrier`,
    demand: `${PANEL_CARRIERS[panel].demand} inside the ${panel} panel`,
    unmet: (observation, assignment) => {
      const counts = observation.panelCarriers?.[panel];
      if (!counts) return null;
      const declared = assignment?.carriers?.[panel];
      const requires = declared === undefined ? PANEL_CARRIERS[panel].requires : PANEL_CARRIER_ALTERNATES[panel][declared];
      const missing = requires.filter((element) => !(counts[element] > 0));
      if (missing.length === 0) return null;
      const inventory = CARRIER_ELEMENTS.map((element) => `${element} ${counts[element] ?? 0}`).join(", ");
      const source = declared === undefined ? "" : ", which the configuration declares for it";
      return `the ${panel} panel carries no ${missing.join(" and no ")}${source} (${inventory})`;
    },
  });
}

const PROVENANCE = Object.freeze({
  id: "provenance-line",
  demand: "the visible source provenance line",
  unmet: (observation) => observation.provenance ? null : "no visible provenance line",
});

// An illustrated or pass-through page renders its source unchanged inside one
// source region; the validator proves those bytes against the committed source.
const SOURCE_REGION = Object.freeze({
  id: "source-region",
  demand: "the unchanged source region",
  unmet: (observation) => observation.sourceRegions === 1 ? null : `${observation.sourceRegions ?? 0} source region(s)`,
});

const BOUND_FIGURES = Object.freeze({
  id: "bound-figures",
  demand: "every figure bound to the route in the configuration",
  unmet: (observation, assignment) => {
    const expected = assignment?.figures?.length ?? 0;
    const drawn = observation.companions ?? 0;
    return drawn === expected ? null : `the configuration binds ${expected} figure(s) and the page draws ${drawn}`;
  },
});

export const PAGE_CLASSES = Object.freeze({
  explanatory: Object.freeze({
    id: "explanatory",
    label: "explanatory page",
    requirements: Object.freeze([
      ...UTILITY_CHROME,
      PROVENANCE,
      Object.freeze({
        id: "altitude-trio",
        demand: `the altitude trio ${ALTITUDE_PANELS.join(", ")}`,
        unmet: (observation) => {
          const missing = ALTITUDE_PANELS.filter((panel) => !observation.altitudePanels.includes(panel));
          if (missing.length === 0) return null;
          const present = observation.altitudePanels.length ? observation.altitudePanels.join(", ") : "none";
          return `missing ${missing.join(", ")}; present ${present}`;
        },
      }),
      ...ALTITUDE_PANELS.map(panelCarrierRequirement),
      BOUND_FIGURES,
    ]),
  }),
  // The source rendered as it is, with companion figures bound to it in the
  // configuration: at least one at the page head, and no altitude trio demanded.
  illustrated: Object.freeze({
    id: "illustrated",
    label: "illustrated source",
    requirements: Object.freeze([
      ...UTILITY_CHROME,
      PROVENANCE,
      SOURCE_REGION,
      Object.freeze({
        id: "page-head-figure",
        demand: "a companion figure at the page head",
        unmet: (observation) => (observation.headFigures ?? 0) > 0 ? null : "no figure drawn above the source",
      }),
      BOUND_FIGURES,
    ]),
  }),
  passThrough: Object.freeze({
    id: "pass-through",
    label: "pass-through source",
    requirements: Object.freeze([...UTILITY_CHROME, PROVENANCE, SOURCE_REGION, BOUND_FIGURES]),
  }),
  derivedLookup: Object.freeze({
    id: "derived-lookup",
    label: "derived lookup",
    requirements: Object.freeze([
      ...UTILITY_CHROME,
      PROVENANCE,
      Object.freeze({
        id: "derived-table",
        demand: "the generated lookup table",
        unmet: (observation) => (observation.tables ?? 0) > 0 ? null : "no table outside a figure",
      }),
    ]),
  }),
  recordPointer: Object.freeze({
    id: "record-pointer",
    label: "record pointer page",
    requirements: Object.freeze([
      ...UTILITY_CHROME,
      Object.freeze({
        id: "folder-table",
        demand: `one folder table with the columns ${RECORD_POINTER_COLUMNS.join(", ")}`,
        unmet: (observation) => {
          const columns = observation.pointerColumns.join(", ");
          if (columns !== RECORD_POINTER_COLUMNS.join(", ")) return `table columns are ${columns.length ? columns : "absent"}`;
          return observation.pointerRows > 0 ? null : "the folder table has no rows";
        },
      }),
    ]),
  }),
});

const CLASS_BY_ID = new Map(Object.values(PAGE_CLASSES).map((pageClass) => [pageClass.id, pageClass]));

// The generated pointer route, or null when the guide publishes the records
// themselves or names no pointer folder.
export function recordPointerRoute(config) {
  const records = config?.records;
  if (!records || records.enabled !== false || !Array.isArray(records.pointers) || records.pointers.length === 0) return null;
  return `${records.layer}/records`;
}

// Eligibility comes from the adapter's own output: every page it built from a
// source is explanatory, and the pointer page it generates is the pointer
// class. A stale stub is excluded because its source never built, which the
// build already reports as its own failure.
// The class a source takes: the page_classes entry naming it exactly, else
// the longest prefix that holds it, else explanatory. The adapter and the gate
// both read this one function.
export function pageClassFor(config, sourcePath) {
  const entries = config?.page_classes ?? [];
  const exact = entries.find((entry) => entry.source === sourcePath);
  const prefix = entries.filter((entry) => entry.prefix !== undefined && sourcePath.startsWith(`${entry.prefix.replace(/\/$/, "")}/`))
    .sort((left, right) => right.prefix.length - left.prefix.length)[0];
  const entry = exact ?? prefix;
  if (entry === undefined) return { pageClass: PAGE_CLASSES.explanatory.id, reason: null, note: null, derive: null, declaredBy: null };
  return { pageClass: entry.class, reason: entry.reason ?? null, note: entry.note ?? null, derive: entry.derive ?? null, declaredBy: entry.source ?? entry.prefix };
}

export function classifyPortalPages(config, pages) {
  const declared = new Map((config?.page_carriers ?? []).map((entry) => [entry.source, entry]));
  const assignments = [];
  for (const page of pages) {
    // A stale page is not eligible here because it never built. The gate
    // refuses the run for it separately, through assertNoStaleSources, so a
    // source can never leave the eligible set quietly.
    if (!page || page.stale !== false || typeof page.route !== "string" || typeof page.source_path !== "string") continue;
    const carriers = declared.get(page.source_path);
    const resolved = pageClassFor(config, page.source_path);
    const assignment = { route: page.route, source: page.source_path, pageClass: resolved.pageClass, reason: resolved.reason, figures: (config?.figures ?? []).filter((binding) => binding.route === page.route) };
    if (carriers !== undefined) {
      assignment.carriers = Object.fromEntries(Object.keys(PANEL_CARRIER_ALTERNATES).filter((panel) => carriers[panel] !== undefined).map((panel) => [panel, carriers[panel]]));
    }
    assignments.push(assignment);
  }
  const pointer = recordPointerRoute(config);
  if (pointer !== null) assignments.push({ route: pointer, source: RECORD_POINTER_SOURCE, pageClass: PAGE_CLASSES.recordPointer.id });
  return assignments.sort((left, right) => (left.route < right.route ? -1 : left.route > right.route ? 1 : 0));
}

// A source the adapter could not build leaves a stale stub behind and drops
// out of the eligible set. That is exactly the silence this gate exists to
// break, so the run refuses it and names the source and the reason the adapter
// recorded.
// A carrier declared for a source that the build never produced is a silent
// exemption waiting to happen, so the declaration is held to the eligible set.
// So is a page class declaration that names no published source, and a
// figure bound to a route this build did not publish.
export function declaredCarrierFailures(config, assignments) {
  const sources = new Set(assignments.map((assignment) => assignment.source));
  const routes = new Set(assignments.map((assignment) => assignment.route));
  return [
    ...(config?.page_carriers ?? [])
      .filter((entry) => !sources.has(entry.source))
      .map((entry) => `portal.config.json page_carriers declares a carrier for ${entry.source}, which this build did not publish`),
    ...(config?.page_classes ?? [])
      .filter((entry) => ![...sources].some((source) => entry.source === source || (entry.prefix !== undefined && source.startsWith(`${entry.prefix.replace(/\/$/, "")}/`))))
      .map((entry) => `portal.config.json page_classes declares ${entry.class} for ${entry.source ?? entry.prefix}, which matches no published source`),
    ...(config?.figures ?? [])
      .filter((binding) => !routes.has(binding.route))
      .map((binding) => `portal.config.json binds ${binding.declaration} to ${binding.route}, a route this build did not publish`),
  ];
}

export function assertDeclaredCarriers(config, assignments) {
  const failures = declaredCarrierFailures(config, assignments);
  if (failures.length === 0) return `declared-carriers:${(config?.page_carriers ?? []).length}`;
  throw new Error(`portal composition gate: ${failures.length} declared carrier(s) match no published page\n  ${failures.join("\n  ")}`);
}

export function staleSourceFailures(pages) {
  const failures = [];
  for (const page of pages ?? []) {
    if (!page || page.stale !== true) continue;
    const source = typeof page.source_path === "string" ? page.source_path : "an unnamed source";
    const reason = typeof page.stale_reason === "string" && page.stale_reason.trim() ? page.stale_reason.trim() : "the adapter recorded no reason";
    failures.push(`${source} did not build and is a stale stub at ${page.route ?? "no route"}: ${reason}`);
  }
  return failures;
}

export function assertNoStaleSources(pages) {
  const failures = staleSourceFailures(pages);
  if (failures.length === 0) return `no-stale-sources:${(pages ?? []).length}`;
  throw new Error(`portal composition gate: ${failures.length} source(s) did not build, so the eligible set is incomplete\n  ${failures.join("\n  ")}`);
}

// One line per unmet requirement, each naming the offending source. Every
// assignment is evaluated, so one page's failure never hides another's and a
// route that was never observed fails as loudly as a route that fell short.
export function pageClassFailures(assignments, observations) {
  const observed = new Map(observations.map((observation) => [observation.route, observation]));
  const failures = [];
  for (const assignment of assignments) {
    const pageClass = CLASS_BY_ID.get(assignment.pageClass);
    if (!pageClass) throw new Error(`unknown page class ${assignment.pageClass} for ${assignment.source}`);
    const where = `${assignment.source} (${pageClass.label} at ${assignment.route})`;
    const observation = observed.get(assignment.route);
    if (!observation) {
      failures.push(`${where} was not verified: the run collected no observation for this route`);
      continue;
    }
    for (const requirement of pageClass.requirements) {
      const reason = requirement.unmet(observation, assignment);
      if (reason !== null) failures.push(`${where} lacks ${requirement.demand}: ${reason}`);
    }
  }
  return failures;
}

// The one throw site for the gate. Failures a caller found while exercising a
// page, such as altitude tabs that do not hide their inactive layers, join the
// class failures so one run reports everything it saw.
export function assertPageClassCoverage(assignments, observations, additionalFailures = []) {
  const failures = [...pageClassFailures(assignments, observations), ...additionalFailures];
  if (failures.length === 0) return `page-class-coverage:${assignments.length}`;
  throw new Error(`portal composition gate: ${failures.length} failure(s) across ${assignments.length} eligible source(s)\n  ${failures.join("\n  ")}`);
}

// ADR-0064: while the records switch is off, a pointer folder reaches no route
// at all. Both halves are checked, because the adapter's exclusion and the
// built site can fail apart: a source under a pointer folder must own no page,
// and the built site must carry the pointer page without anything below it.
export function recordRouteFailures(config, pages, builtArtifactPaths) {
  const records = config?.records;
  if (!records || records.enabled !== false) return [];
  const folders = records.pointers.map((pointer) => pointer.folder);
  const failures = [];
  for (const page of pages) {
    if (!page || typeof page.source_path !== "string") continue;
    const folder = folders.find((item) => page.source_path === item || page.source_path.startsWith(`${item}/`));
    if (folder !== undefined) failures.push(`${page.source_path} owns the route ${page.route} although ${folder} is a pointer folder and the records switch is off`);
  }
  const pointer = recordPointerRoute(config);
  if (pointer !== null) {
    for (const artifact of builtArtifactPaths) {
      const route = builtRoute(artifact);
      if (route !== null && route.startsWith(`${pointer}/`)) failures.push(`${artifact} is a per-record route below ${pointer} although the records switch is off`);
    }
  }
  return failures;
}

export function assertNoRecordRoutes(config, pages, builtArtifactPaths) {
  const failures = recordRouteFailures(config, pages, builtArtifactPaths);
  if (failures.length === 0) return;
  throw new Error(`portal composition gate: the records switch is off but the build publishes record routes\n  ${failures.join("\n  ")}`);
}

function builtRoute(artifactPath) {
  if (typeof artifactPath !== "string" || !artifactPath.startsWith("dist/") || !artifactPath.endsWith("/index.html")) return null;
  return artifactPath.slice("dist/".length, -"/index.html".length);
}
