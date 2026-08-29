// Shared subject source for the D1 candidates: where this repository keeps each
// kind of knowledge, and how each kind is allowed to change.
//
// Facts, relationships, verbatim quotations and derivation only. No markup, no
// CSS, no visual primitive, no token. All three D1 candidates read this one
// object, so they cannot disagree about the subject and can be compared on
// encoding alone.
//
// tools/verify.mjs binds every fact here to the repository:
//   - the six layers, their homes and their change cadence are re-parsed out of
//     the AGENTS.md organisation table and compared field by field;
//   - every declared home resolves to a real file or directory in the checkout;
//   - every quotation is verbatim in the file it names;
//   - the traceability spine is quoted from the sentence that declares it;
//   - the derived answer is recomputed from the raw arrays below.
//
// Write-authority vocabulary (closed set), and the band each one sits in:
//   human-owned     a person edits it directly                        [yours]
//   project-owned   this project owns the text and extends it freely  [yours]
//   ship-flow-only  editable, but only inside the ship flow's PR      [conditional]
//   tool-maintained a tool rewrites it; an edit here is overwritten   [refuses]
//   cli-allocated   the CLI allocates the record; you fill it in      [refuses]
//   append-only     never edited — superseded by a new entry          [refuses]
//   generated       produced on demand; there is nothing to maintain  [refuses]
window.repoLayers = (() => {
  const contract = "AGENTS.md";
  const q = (text, source = contract) => ({ text, source });

  // ------------------------------------------------------------------ bands
  const bands = [
    { id: "yours", label: "you write it", order: 1 },
    { id: "conditional", label: "you write it, in one flow", order: 2 },
    { id: "refuses", label: "it refuses your edit", order: 3 },
  ];

  const authorities = [
    {
      id: "human-owned", band: "yours", label: "human-owned",
      rule: "a person edits it directly, and rarely",
      quote: q("| WHY — purpose, users, scope, non-goals | `docs/product.md` | rarely; human-owned |"),
    },
    {
      id: "project-owned", band: "yours", label: "project-owned",
      rule: "everything outside the managed markers belongs to this project",
      quote: q("everything outside it is project-owned — extend freely."),
    },
    {
      id: "ship-flow-only", band: "conditional", label: "ship flow only",
      rule: "editable, but only in the same pull request as the code it describes",
      quote: q("**Docs mutate only inside the ship flow, in the same PR as the code:**"),
    },
    {
      id: "tool-maintained", band: "refuses", label: "tool-maintained",
      rule: "a tool owns these bytes; an edit is overwritten on the next update",
      quote: q("The block between the codeflow markers below is maintained by"),
    },
    {
      id: "cli-allocated", band: "refuses", label: "CLI-allocated",
      rule: "the CLI allocates the record and its stable id; you fill the body in",
      quote: q("CodeFlow writes flat stable-ID records (`epics/EPC-NNN.md`,"),
    },
    {
      id: "append-only", band: "refuses", label: "append-only",
      rule: "never edited; a change is a new entry that supersedes the old one",
      quote: q("**Append-only records:** ADRs and the ledger are never edited — supersede with"),
    },
    {
      id: "generated", band: "refuses", label: "generated",
      rule: "produced on demand from the records; there is no file to maintain",
      quote: q("Status views are generated (`codeflow status`) — never hand-maintain a dashboard."),
    },
  ];

  // ----------------------------------------------------------------- layers
  // `changes` and `livesIn` are the AGENTS.md table's own cells, quoted so the
  // verifier can re-parse the table and compare them. `cadence` is the table's
  // own row order: the contract already lists its layers from the one that
  // changes least to the one that changes with nobody touching it, so the
  // volatility axis is read out of the contract rather than invented here. The
  // verifier checks each cadence against that row's index in AGENTS.md.
  const layers = [
    {
      id: "WHY", name: "WHY", asks: "purpose, users, scope, non-goals",
      livesIn: "`docs/product.md`", changes: "rarely; human-owned", cadence: 1,
      homes: [{ path: "docs/product.md", kind: "file", authority: "human-owned" }],
      row: q("| WHY — purpose, users, scope, non-goals | `docs/product.md` | rarely; human-owned |"),
    },
    {
      id: "RULES", name: "RULES", asks: "how we work",
      livesIn: "this file + the agent skills (`.claude/skills/`, `.agents/skills/`)",
      changes: "rarely", cadence: 2,
      homes: [
        { path: "AGENTS.md", kind: "file", authority: "project-owned", split: "tool-maintained",
          splitNote: "the block between the codeflow markers is not yours" },
        { path: ".claude/skills", kind: "dir", authority: "tool-maintained" },
        { path: ".agents/skills", kind: "dir", authority: "tool-maintained" },
      ],
      row: q("| RULES — how we work | this file + the agent skills (`.claude/skills/`, `.agents/skills/`) | rarely |"),
    },
    {
      id: "WHAT", name: "WHAT", asks: "what the system does",
      livesIn: "`docs/capabilities.md` (CAP-### registry)", changes: "every ship", cadence: 3,
      homes: [{ path: "docs/capabilities.md", kind: "file", authority: "ship-flow-only" }],
      row: q("| WHAT — what the system does | `docs/capabilities.md` (CAP-### registry) | every ship |"),
    },
    {
      id: "HOW", name: "HOW", asks: "structure and decisions",
      livesIn: "`docs/architecture.md` + `docs/decisions/` (ADRs)", changes: "per decision", cadence: 4,
      homes: [
        { path: "docs/architecture.md", kind: "file", authority: "ship-flow-only" },
        { path: "docs/decisions", kind: "dir", authority: "append-only" },
      ],
      row: q("| HOW — structure and decisions | `docs/architecture.md` + `docs/decisions/` (ADRs) | per decision |"),
    },
    {
      id: "WORK", name: "WORK", asks: "planned and active work",
      livesIn: "`project-management/` (epics, specs, tasks — allocated by the CLI)",
      changes: "daily", cadence: 5,
      homes: [{ path: "project-management", kind: "dir", authority: "cli-allocated" }],
      row: q("| WORK — planned and active work | `project-management/` (epics, specs, tasks — allocated by the CLI) | daily |"),
    },
    {
      id: "TRACE", name: "TRACE", asks: "what happened and why",
      livesIn: "ledger + `codeflow recall`", changes: "automatic", cadence: 6,
      // Both homes are deliberately not paths in the checkout, and that is the
      // fact — the layer that changes most often is the one with nothing to open.
      homes: [
        { path: "the local ledger", kind: "local-state", authority: "append-only",
          absentNote: "operational evidence, held outside the working tree",
          absentQuote: q("the local ledger supplies operational evidence and `codeflow recall` search,") },
        { path: "codeflow status", kind: "command", authority: "generated",
          absentNote: "there is no dashboard file to maintain",
          absentQuote: q("Status views are generated (`codeflow status`) — never hand-maintain a dashboard.") },
      ],
      row: q("| TRACE — what happened and why | ledger + `codeflow recall` | automatic |"),
    },
  ];

  // ------------------------------------------------------------------ spine
  // The one relationship the contract declares between the layers, quoted from
  // the sentence that declares it. Each node names the layer it belongs to, so
  // a candidate can hang the layers off the spine rather than asserting a link.
  const spine = {
    quote: q("The traceability spine runs downward: capability → epic/task → ADR/spec → PR →"),
    tail: q("ledger. `validate --docs` checks the Git-tracked workgraph and its stable IDs;"),
    nodes: [
      { id: "capability", label: "capability", layer: "WHAT", holds: "CAP-### — what the system does" },
      { id: "epic-task", label: "epic / task", layer: "WORK", holds: "EPC-### and TSK-### — the work that changes it" },
      { id: "adr-spec", label: "ADR / spec", layer: "HOW", holds: "the decision the work rests on, and the interface it pins" },
      { id: "pr", label: "PR", layer: "WHAT", holds: "the change itself, where the docs move with the code" },
      { id: "ledger", label: "ledger", layer: "TRACE", holds: "what actually happened, written for you" },
    ],
    walkUp: q("Answer \"why is X this way\" by"),
  };

  // Two contract sentences a candidate may show as context. Neither is an answer.
  const context = {
    frozen: q("Specs are planning inputs, frozen (`status: implemented`) when their consuming"),
    validated: q("`validate --docs` checks the Git-tracked workgraph and its stable IDs;"),
  };

  // ------------------------------------------------------------- derivation
  const authorityById = new Map(authorities.map((a) => [a.id, a]));
  const bandOf = (authorityId) => authorityById.get(authorityId)?.band ?? null;
  const homesOf = (layer) => layer.homes;
  const authoritiesOf = (layer) => [...new Set(layer.homes.flatMap((h) => [h.authority, h.split].filter(Boolean)))];
  const bandsOf = (layer) => [...new Set(authoritiesOf(layer).map(bandOf))];
  const byCadence = [...layers].sort((a, b) => a.cadence - b.cadence);

  const paths = layers.flatMap((layer) =>
    layer.homes.map((home) => ({
      path: home.path, kind: home.kind, layer: layer.id,
      authority: home.authority, band: bandOf(home.authority),
      split: home.split ?? null, splitBand: home.split ? bandOf(home.split) : null,
      inTree: home.kind === "file" || home.kind === "dir",
      note: home.splitNote ?? home.absentNote ?? null,
    })));

  return {
    contract, bands, authorities, layers, spine, context, paths,
    bandOf, homesOf, authoritiesOf, bandsOf, byCadence,
    // Published to the DOM by every D1 candidate; verify.mjs recomputes it from
    // the raw arrays above and from the AGENTS.md table, never off the page.
    answer: {
      order: byCadence.map((l) => l.id),
      yours: paths.filter((p) => p.band === "yours").map((p) => p.path),
      conditional: paths.filter((p) => p.band === "conditional").map((p) => p.path),
      refuses: paths.filter((p) => p.band === "refuses").map((p) => p.path),
      split: paths.filter((p) => p.split).map((p) => p.path),
      notInTree: paths.filter((p) => !p.inTree).map((p) => p.path),
      rulesLayer: layers.find((l) => l.asks === "how we work").id,
    },
  };
})();
