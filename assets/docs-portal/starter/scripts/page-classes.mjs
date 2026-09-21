// The portal's page classes and what each class must show, declared once.
// `browser-verify.mjs` reads the rules from here and applies them to every
// page the adapter produced, so coverage is per source: a compliant page can
// never stand in for a noncompliant one, and no rule is inferred from a source
// path. The rules restate docs/architecture/utility-presentation.md and
// ADR-0064; changing what a class requires means changing this table.

// The altitude trio (ADR-0053). The adapter renders these depth-2 sections as
// one tablist, and an explanatory page carries all three, not a subset.
export const ALTITUDE_PANELS = Object.freeze(["concept", "architecture", "technical"]);

// The carriers the adapter can generate inside a panel: a cf-stage or text
// figure, a Markdown table, or a fenced block. One of them must open the
// Concept panel, because a panel of prose alone is the source re-rendered.
export const CONCEPT_CARRIERS = Object.freeze(["figure", "table", "pre"]);

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

export const PAGE_CLASSES = Object.freeze({
  explanatory: Object.freeze({
    id: "explanatory",
    label: "explanatory page",
    requirements: Object.freeze([
      ...UTILITY_CHROME,
      Object.freeze({
        id: "provenance-line",
        demand: "the visible source provenance line",
        unmet: (observation) => observation.provenance ? null : "no visible provenance line",
      }),
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
      Object.freeze({
        id: "concept-figure",
        demand: `a ${CONCEPT_CARRIERS.join(", ")} carrier in the Concept panel`,
        unmet: (observation) => observation.conceptCarriers > 0 ? null : "the Concept panel carries prose only",
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
export function classifyPortalPages(config, pages) {
  const assignments = [];
  for (const page of pages) {
    if (!page || page.stale !== false || typeof page.route !== "string" || typeof page.source_path !== "string") continue;
    assignments.push({ route: page.route, source: page.source_path, pageClass: PAGE_CLASSES.explanatory.id });
  }
  const pointer = recordPointerRoute(config);
  if (pointer !== null) assignments.push({ route: pointer, source: RECORD_POINTER_SOURCE, pageClass: PAGE_CLASSES.recordPointer.id });
  return assignments.sort((left, right) => (left.route < right.route ? -1 : left.route > right.route ? 1 : 0));
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
      const reason = requirement.unmet(observation);
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
