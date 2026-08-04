// Shared subject source for the D2 candidates: which decision governs SPC-004
// now, and what evidence backs each of its claims.
//
// Facts, relationships, verbatim quotations and derivation only. No markup, no
// CSS, no visual primitive, no token. Both D2 candidates read this one object.
//
// tools/verify.mjs binds every fact here to the repository:
//   - every record's frontmatter field against the real record file;
//   - every quotation against the file it declares, whitespace-normalised;
//   - the derived answer against an independent recomputation.
//
// Relation vocabulary (closed set), chosen so nothing is over-claimed:
//   governs               the decision's scope covers this claim
//   supersedes-mechanism  the decision reaches ONE named mechanism inside an
//                         earlier decision, and nothing else. The earlier
//                         decision stays in force and its `superseded_by` stays
//                         null — which is exactly why frontmatter alone cannot
//                         answer the question
//   process               the decision governs the process the claim depends on,
//                         not the claim's content
//
// Evidence vocabulary (closed set):
//   executed  a run exists at the exact implementation candidate
//   bounded   evidence exists but is explicitly not promoted to that candidate
//   absent    no evidence exists, with the reason recorded
window.recordAuthority = (() => {
  const SPC004 = "project-management/specs/SPC-004.md";
  const ADR0049 = "docs/decisions/ADR-0049-cf-present-runtime-boundary.md";
  const ADR0050 = "docs/decisions/ADR-0050-cf-present-document-session-contract.md";
  const ADR0051 = "docs/decisions/ADR-0051-bounded-design-sourcing-and-revision.md";
  const ADR0052 = "docs/decisions/ADR-0052-separate-present-runtime-and-durable-state.md";
  const TSK011 = "project-management/tasks/TSK-011.md";
  const TSK014 = "project-management/tasks/TSK-014.md";
  const EPC005 = "project-management/epics/EPC-005.md";
  const TSK007 = "docs/verification/tsk-007-presentation/README.md";
  const OBSERVER = "observer-state.md";

  // The record under the lens.
  const subject = {
    id: "SPC-004", path: SPC004, kind: "spec",
    title: "interactive presentation utility contract",
    frontmatter: { status: "approved", created: "2026-07-31" },
    whyNotImplemented: "its consuming work has not shipped",
  };

  // Every other record is here because a real frontmatter field or a quoted
  // sentence connects it to the subject.
  const records = [
    {
      id: "ADR-0049", path: ADR0049, kind: "decision",
      title: "bounded cf-present runtime and renderer boundary",
      frontmatter: { date: "2026-08-01", status: "accepted", superseded_by: "null" },
    },
    {
      id: "ADR-0050", path: ADR0050, kind: "decision",
      title: "versioned cf-present document and session contract",
      frontmatter: { date: "2026-08-01", status: "accepted", superseded_by: "null" },
    },
    {
      id: "ADR-0051", path: ADR0051, kind: "decision",
      title: "extend product design with bounded variation, trustworthy sourcing, and revision provenance",
      frontmatter: { date: "2026-08-01", status: "accepted", superseded_by: "null" },
    },
    {
      id: "ADR-0052", path: ADR0052, kind: "decision",
      title: "separate cf-present ephemeral runtime from durable state",
      frontmatter: { date: "2026-08-01", status: "accepted", superseded_by: "null" },
    },
    {
      id: "TSK-011", path: TSK011, kind: "task",
      title: "build the interactive presentation utility",
      frontmatter: { status: "blocked", specs: "[SPC-004]", depends_on: "[TSK-006, TSK-014]" },
    },
    {
      id: "TSK-014", path: TSK014, kind: "task",
      title: "recover the design method and utility directions",
      frontmatter: { status: "in_progress", specs: "[SPC-004, SPC-005, SPC-006]" },
    },
    {
      id: "EPC-005", path: EPC005, kind: "epic",
      title: "presentation, design, and documentation system",
      frontmatter: { status: "in_progress", adrs: "[ADR-0047, ADR-0048, ADR-0049, ADR-0050, ADR-0051, ADR-0052]" },
    },
  ];

  // The sentence the whole case turns on. Quoted, not paraphrased.
  const partialScope = {
    text: "ADR-0049 and ADR-0050 remain historical decisions; this ADR supersedes only their same-tree runtime assumption and post-create Windows DACL mechanism where those details conflict.",
    source: ADR0052,
  };
  const noOutcomeChange = {
    text: "This decision changes no product outcome, task owner, graph, or integration target.",
    source: ADR0052,
  };

  const claims = [
    {
      id: "CLM-1",
      where: "§ Behavior 1",
      claim: "A versioned envelope of typed blocks; the schema rejects malformed documents, and an older renderer opening a newer schema falls back to an explicit read-only view rather than a silent partial render.",
      quote: {
        text: "An older renderer opening a newer schema falls back to an explicit read-only/raw view, never a silent partial render.",
        source: SPC004,
      },
      authority: [
        {
          record: "ADR-0050", relation: "governs",
          quote: {
            text: "An older renderer receiving a higher document version displays the complete raw envelope as escaped, read-only text with an explicit unsupported-version warning. It performs no partial typed rendering and exposes no mutation controls.",
            source: ADR0050,
          },
        },
      ],
      evidence: {
        state: "executed",
        what: "codeflow test --mode full --strict passed all nine targets at the exact candidate",
        at: "826715510440df53fc999e68d77429d7b49a6d55",
        bound: "macOS arm64 only",
        source: TSK007,
        quote: { text: "passed all nine targets at the exact clean candidate", source: TSK007 },
      },
    },
    {
      id: "CLM-2",
      where: "§ Behavior 10",
      claim: "The local service binds loopback, allocates random ports and per-session tokens, validates Host and Origin, applies strict CSP, and never attaches to the operator's browser profile or active view.",
      quote: {
        text: "never attaches to the operator's browser profile or active view",
        source: SPC004,
      },
      authority: [
        {
          record: "ADR-0049", relation: "governs",
          quote: {
            text: "It creates a new app window with a unique CodeFlow-owned profile directory and never attaches to an existing profile or active view.",
            source: ADR0049,
          },
        },
      ],
      evidence: {
        state: "executed",
        what: "the real headless browser journey and the injected-failure cleanup journey both ran at the exact candidate",
        at: "826715510440df53fc999e68d77429d7b49a6d55",
        bound: "the journey used --no-launch with a directly owned Playwright browser, so it is not native product-launcher evidence",
        source: TSK007,
        quote: { text: "so it is not native product-launcher evidence", source: TSK007 },
      },
    },
    {
      id: "CLM-3",
      where: "§ Behavior 8",
      claim: "Owner-private state uses the platform-appropriate OS state directory keyed by canonical project identity, so worktrees share state without writing runtime data into `.git` or the worktree.",
      quote: {
        text: "Owner-private state uses the platform-appropriate OS state directory keyed by canonical project identity, so worktrees share state without writing runtime data into `.git` or the worktree.",
        source: SPC004,
      },
      authority: [
        {
          record: "ADR-0049", relation: "governs",
          quote: {
            text: "platform-state authority",
            source: ADR0049,
          },
        },
        {
          record: "ADR-0052", relation: "supersedes-mechanism",
          scope: partialScope,
          quote: {
            text: "Create Windows private files with a protected owner-only security descriptor passed to `CreateFileW`, then verify the returned handle; no permissive inherited ACL interval is accepted.",
            source: ADR0052,
          },
        },
      ],
      evidence: {
        state: "bounded",
        what: "a deterministic confinement guard runs, and the record states exactly what it does not prove",
        at: null,
        bound: "start-time confinement only, not account, profile, or VM teardown; no native Windows or WSL2 run exists",
        source: TSK007,
        quote: {
          text: "That guard proves start-time confinement, not account, profile, or VM teardown.",
          source: TSK007,
        },
      },
    },
    {
      id: "CLM-4",
      where: "§ Interfaces and formats",
      claim: "V1 supports macOS, Linux, native Windows and WSL2 using native conventions; cross-compilation proves only buildability, and an unqualified platform fails closed rather than selecting an insecure fallback.",
      quote: {
        text: "each supported runtime needs native or explicitly scoped runtime evidence, and an unqualified platform fails closed rather than selecting an insecure fallback.",
        source: SPC004,
      },
      authority: [
        {
          record: "ADR-0052", relation: "governs",
          quote: {
            text: "Native Windows remains a release qualification boundary; cross-compilation proves buildability only.",
            source: ADR0052,
          },
        },
      ],
      evidence: {
        state: "bounded",
        what: "Linux arm64 evidence exists at revision 74feaf04, older than the exact candidate",
        at: "74feaf04ca0eea0062a4554200e7a7e774fc5b6f",
        bound: "retained as bounded diagnostic evidence and explicitly not promoted; no native Windows or WSL2 run at all",
        source: TSK007,
        quote: {
          text: "is retained as bounded diagnostic evidence and is not promoted to exact-candidate platform approval",
          source: TSK007,
        },
      },
    },
    {
      id: "CLM-5",
      where: "§ Behavior 3",
      claim: "The selected default must be modern, elegant, aesthetically coherent, pleasing, functional, responsive, and suited to focused explanation/review rather than resembling a generic model artifact or dashboard template.",
      quote: {
        text: "The selected default must be modern, elegant, aesthetically coherent, pleasing, functional, responsive, and suited to focused explanation/review rather than resembling a generic model artifact or dashboard template.",
        source: SPC004,
      },
      authority: [
        {
          record: "ADR-0051", relation: "process",
          quote: {
            text: "cf-design gains a delta-only progressive-disclosure sourcing contract and the existing versioned plan retains reviewed-design provenance",
            source: ADR0051,
          },
        },
      ],
      evidence: {
        state: "absent",
        what: null,
        at: null,
        bound: "the operator's real trial invalidated the prior design acceptance, and the replacement rendered board's blind gates are not run",
        source: TSK014,
        quote: {
          text: "The operator-visible `cf-present` trial and the built portal invalidated the prior design acceptance.",
          source: TSK014,
        },
        alsoQuote: {
          text: "A clean deterministic pass says the study is internally consistent and accessible. It says nothing about whether a composition communicates.",
          source: OBSERVER,
        },
      },
    },
  ];

  // The record's review state, which is not a claim of the record itself.
  const verdicts = [
    {
      id: "V1", lane: "cross-lineage review", state: "returned",
      what: "two independent GPT-5.6 Sol/high reviews approved the exact candidate with no material findings",
      quote: { text: "returned approval with no material findings", source: TSK007 },
    },
    {
      id: "V2", lane: "Claude judgment verdict", state: "blocked",
      what: "the session bound correctly; the accepted turn ended on authorization, not on anything the change did",
      quote: { text: "the accepted turn ended immediately with `oauth_org_not_allowed`", source: TSK007 },
    },
  ];

  // ------------------------------------------------------------ derivation
  const recordById = new Map([subject, ...records].map((r) => [r.id, r]));
  const authorityOf = (claimId) => claims.find((c) => c.id === claimId)?.authority ?? [];
  const relationsTo = (recordId) => claims
    .filter((c) => c.authority.some((a) => a.record === recordId))
    .map((c) => ({ claim: c.id, relation: c.authority.find((a) => a.record === recordId).relation }));

  // The decision that governs the record NOW is the most recent accepted
  // decision that reaches it. Its reach is partial exactly when its only link to
  // some claim is `supersedes-mechanism`.
  const linked = [...new Set(claims.flatMap((c) => c.authority.map((a) => a.record)))];
  const byDate = linked
    .map((id) => recordById.get(id))
    .filter((r) => r && r.frontmatter.status === "accepted")
    .sort((a, b) => (a.frontmatter.date < b.frontmatter.date ? 1 : -1));
  const partialRecords = [...new Set(claims.flatMap((c) =>
    c.authority.filter((a) => a.relation === "supersedes-mechanism").map((a) => a.record)))];
  const governing = partialRecords.length ? partialRecords[0] : byDate[0]?.id ?? null;
  const governingReach = partialRecords.includes(governing) ? "partial" : "whole";
  const unbacked = claims.filter((c) => c.evidence.state === "absent").map((c) => c.id);
  const boundedClaims = claims.filter((c) => c.evidence.state === "bounded").map((c) => c.id);
  const backed = claims.filter((c) => c.evidence.state === "executed").map((c) => c.id);
  const central = claims.find((c) => c.evidence.state === "absent") ?? claims[0];

  return {
    subject, records, claims, verdicts, partialScope, noOutcomeChange,
    recordById, authorityOf, relationsTo, partialRecords, unbacked, boundedClaims, backed, central,
    // Published to the DOM by both D2 candidates.
    answer: {
      governing,
      reach: governingReach,
      stillInForce: ["ADR-0049", "ADR-0050"],
      centralClaim: central.id,
      centralEvidence: central.evidence.state,
      unbacked,
      bounded: boundedClaims,
      backed,
    },
  };
})();
