// Shared fact registry for the P2 candidates.
//
// Every material claim carries a `quote` taken verbatim from
// docs/verification/tsk-007-presentation/README.md. `tools/verify.mjs`
// whitespace-normalises that record and fails if any quote is absent, so a
// candidate cannot assert something the qualification record does not say.
// Both P2 candidates render from this one registry: the rail candidate uses
// `provenance` and `cells`, the absence candidate uses `cells` and
// `absenceCauses`, and neither may introduce a fact of its own.
//
// W3 addition: `causes` and `absenceCauses` classify every cell that is not an
// executed run and not inapplicable. The classification is the new fact, and it
// is not free — each cause carries a quotation `tools/verify.mjs` checks
// verbatim against the qualification record, and the verifier requires every
// such cell to be classified, so a silently unclassified absence fails the run
// rather than disappearing from the encoding.
window.tsk007Facts = {
  source: "docs/verification/tsk-007-presentation/README.md",
  candidate: "826715510440df53fc999e68d77429d7b49a6d55",
  candidateShort: "82671551",
  candidateQuote: "826715510440df53fc999e68d77429d7b49a6d55",
  lanes: ["macOS arm64", "Linux arm64", "native Windows", "WSL2", "independent review"],
  claims: [
    {
      id: "targets",
      claim: "Nine strict test targets",
      cells: [
        { state: "executed", text: "pass, all nine", note: "presentation target 158.336 s",
          quote: "passed all nine targets at the exact clean candidate" },
        { state: "bounded", text: "pass at 74feaf04", note: "older revision, not promoted",
          quote: "91 platform-applicable presentation tests" },
        { state: "notrun", text: "not run", note: "Parallels licence expired" },
        { state: "notrun", text: "not run", note: "" },
        { state: "na", text: "n/a", note: "" },
      ],
      provenance: {
        produced: "codeflow test --mode full --strict, macOS arm64",
        producedDetail: "presentation target, 158.336 s",
        artifact: "renderer digest",
        artifactDetail: "d9d37a03b53f0006b79fbd77fcf53b9fec1fc0df5a3aef17e5a4d86032e0a900",
        acceptance: "two GPT-5.6 Sol/high reviews, no material findings",
        acceptanceDetail: "candidate 82671551",
        reaches: 4, reason: "",
        quotes: [
          "d9d37a03b53f0006b79fbd77fcf53b9fec1fc0df5a3aef17e5a4d86032e0a900",
          "Two independent GPT-5.6 Sol/high reviews inspected exact candidate",
        ],
      },
    },
    {
      id: "release-binary",
      claim: "Isolated release-binary measurement",
      cells: [
        { state: "executed", text: "pass", note: "two exact builds matched",
          quote: "16,518,448-byte production binary" },
        { state: "na", text: "n/a", note: "" },
        { state: "notrun", text: "not run", note: "" },
        { state: "notrun", text: "not run", note: "" },
        { state: "na", text: "n/a", note: "" },
      ],
      provenance: {
        produced: "isolated macOS arm64 measurement",
        producedDetail: "service delta 1,025,264 B of 1,250,000 B",
        artifact: "16,518,448-byte binary",
        artifactDetail: "6cc9d1679b0a46d1b6a16b8c3c46bafab70e826ff6ce5a415835de6068ea795e",
        acceptance: "two exact production builds matched",
        acceptanceDetail: "candidate 82671551",
        reaches: 4, reason: "",
        quotes: ["6cc9d1679b0a46d1b6a16b8c3c46bafab70e826ff6ce5a415835de6068ea795e"],
      },
    },
    {
      id: "linux",
      claim: "Native Linux support",
      cells: [
        { state: "na", text: "n/a", note: "" },
        { state: "bounded", text: "pass at 74feaf04", note: "explicitly not promoted",
          quote: "is retained as bounded diagnostic evidence and is not promoted to exact-candidate platform approval" },
        { state: "notrun", text: "not run", note: "" },
        { state: "notrun", text: "not run", note: "" },
        { state: "na", text: "n/a", note: "" },
      ],
      provenance: {
        produced: "headless Ubuntu 24.04 arm64 Lima/VZ guest",
        producedDetail: "91 platform-applicable tests, full journey",
        artifact: "secret-scanned source archive",
        artifactDetail: "33fcfd57… at revision 74feaf04ca0eea0062a4554200e7a7e774fc5b6f",
        acceptance: "exact-candidate platform approval",
        acceptanceDetail: "",
        reaches: 3,
        reason: "The artifact exists, but at a revision older than the exact candidate. The record retains it as bounded diagnostic evidence and explicitly does not promote it, so the chain never reaches acceptance for 82671551. It is also arm64 only, not x86_64.",
        quotes: [
          "A plain headless Ubuntu 24.04 arm64 Lima/VZ guest ran source revision",
          "74feaf04ca0eea0062a4554200e7a7e774fc5b6f",
          "33fcfd57a55a709deddddbcf66ae33bb129084932dd1d4501d8313e3745665b4",
        ],
      },
    },
    {
      id: "desktop-chromium",
      claim: "Full desktop Chromium binary",
      cells: [
        { state: "executed", text: "pass", note: "" },
        { state: "notclaimed", text: "SIGTRAP", note: "not claimed as passed",
          quote: "exited with `SIGTRAP` under that VZ guest and is not claimed as passed" },
        { state: "notrun", text: "not run", note: "" },
        { state: "notrun", text: "not run", note: "" },
        { state: "na", text: "n/a", note: "" },
      ],
      provenance: {
        produced: "same Lima/VZ guest run",
        producedDetail: "Playwright arm64 headless_shell ran successfully",
        artifact: "a passing desktop-Chromium artifact",
        artifactDetail: "",
        acceptance: "acceptance",
        acceptanceDetail: "",
        reaches: 2,
        reason: "The run produced no passing artifact: the desktop binary exited with SIGTRAP. The record states it is not claimed as passed, so nothing downstream can be built on it.",
        quotes: ["exited with `SIGTRAP` under that VZ guest and is not claimed as passed"],
      },
    },
    {
      id: "windows",
      claim: "Windows profile and teardown safety",
      cells: [
        { state: "na", text: "n/a", note: "" },
        { state: "na", text: "n/a", note: "" },
        { state: "bounded", text: "start-time guard only", note: "no teardown proof",
          quote: "That guard proves start-time confinement, not account, profile, or VM teardown." },
        { state: "notrun", text: "not run", note: "" },
        { state: "na", text: "n/a", note: "" },
      ],
      provenance: {
        produced: "deterministic confinement guard",
        producedDetail: "rejects unmarked profiles and escaping roots",
        artifact: "start-time confinement only",
        artifactDetail: "",
        acceptance: "native Windows run proving teardown",
        acceptanceDetail: "",
        reaches: 3,
        reason: "The guard proves confinement at start time, not account, profile, or VM teardown. No native Windows or WSL2 run exists to close it — the installed Parallels licence had expired.",
        quotes: [
          "That guard proves start-time confinement, not account, profile, or VM teardown.",
          "No native Windows or WSL2 run was available because the installed Parallels license had expired.",
        ],
      },
    },
    {
      id: "launcher",
      claim: "Native product-launcher behaviour",
      cells: [
        { state: "notrun", text: "not evidence", note: "ran --no-launch with an owned browser",
          quote: "so it is not native product-launcher evidence" },
        { state: "notrun", text: "not evidence", note: "" },
        { state: "notrun", text: "not run", note: "" },
        { state: "notrun", text: "not run", note: "" },
        { state: "na", text: "n/a", note: "" },
      ],
      provenance: null,
    },
    {
      id: "cross-lineage-review",
      claim: "Cross-lineage review of the exact candidate",
      cells: [
        { state: "na", text: "n/a", note: "" },
        { state: "na", text: "n/a", note: "" },
        { state: "na", text: "n/a", note: "" },
        { state: "na", text: "n/a", note: "" },
        { state: "executed", text: "approved ×2", note: "no material findings",
          quote: "returned approval with no material findings" },
      ],
      provenance: null,
    },
    {
      id: "claude-verdict",
      claim: "Bound Claude judgment verdict",
      cells: [
        { state: "na", text: "n/a", note: "" },
        { state: "na", text: "n/a", note: "" },
        { state: "na", text: "n/a", note: "" },
        { state: "na", text: "n/a", note: "" },
        { state: "notrun", text: "blocked", note: "turn ended oauth_org_not_allowed",
          quote: "the accepted turn ended immediately with `oauth_org_not_allowed`" },
      ],
      provenance: {
        produced: "native interactive Claude Code 2.1.220 session",
        producedDetail: "schema-v2 delegate lifecycle",
        artifact: "session bound to the exact review prompt",
        artifactDetail: "523349c9-070a-4f3d-a56f-68b8a3769fb5",
        acceptance: "returned verdict",
        acceptanceDetail: "",
        reaches: 3,
        reason: "The accepted turn ended immediately with oauth_org_not_allowed. This link fails on authorization, not on anything the change did, so the verdict is absent rather than negative.",
        quotes: [
          "523349c9-070a-4f3d-a56f-68b8a3769fb5",
          "the accepted turn ended immediately with `oauth_org_not_allowed`",
        ],
      },
    },
  ],

  // ------------------------------------------------------- W3: why it is missing
  // A closed set. Each entry says what kind of thing stopped the evidence, and
  // carries the record's own sentence for it. "Inapplicable" is deliberately not
  // a cause: a lane a claim does not apply to is not a gap.
  causes: [
    {
      id: "no-environment", label: "no environment to run it in",
      detail: "the machine to run it on was not available at all",
      quote: "No native Windows or WSL2 run was available because the installed Parallels license had expired.",
    },
    {
      id: "wrong-revision", label: "run, but not at this candidate",
      detail: "it exists at an older revision, not promoted here",
      quote: "is retained as bounded diagnostic evidence and is not promoted to exact-candidate platform approval",
    },
    {
      id: "run-failed", label: "ran, produced nothing to stand on",
      detail: "the run happened and yielded no passing artifact",
      quote: "exited with `SIGTRAP` under that VZ guest and is not claimed as passed",
    },
    {
      id: "scope-of-the-check", label: "the check proves less than the claim",
      detail: "something ran, but not the thing the claim needs",
      quote: "That guard proves start-time confinement, not account, profile, or VM teardown.",
      alsoQuote: "so it is not native product-launcher evidence",
    },
    {
      id: "authorization", label: "refused before it began",
      detail: "accepted, then stopped on authorization",
      quote: "the accepted turn ended immediately with `oauth_org_not_allowed`",
    },
  ],

  // Keyed `<claim id>:<lane index>`. `tools/verify.mjs` requires exactly one
  // entry for every cell whose state is neither `executed` nor `na`, and no
  // entry for any cell that is — so an absence cannot be quietly dropped from
  // the classification and an executed run cannot be quietly counted as a gap.
  absenceCauses: {
    "targets:1": "wrong-revision",
    "targets:2": "no-environment",
    "targets:3": "no-environment",
    "release-binary:2": "no-environment",
    "release-binary:3": "no-environment",
    "linux:1": "wrong-revision",
    "linux:2": "no-environment",
    "linux:3": "no-environment",
    "desktop-chromium:1": "run-failed",
    "desktop-chromium:2": "no-environment",
    "desktop-chromium:3": "no-environment",
    "windows:2": "scope-of-the-check",
    "windows:3": "no-environment",
    "launcher:0": "scope-of-the-check",
    "launcher:1": "scope-of-the-check",
    "launcher:2": "no-environment",
    "launcher:3": "no-environment",
    "claude-verdict:4": "authorization",
  },

  // Where a claim's derivation stops short of the exact candidate, the kind of
  // thing that stopped it — same closed set as `causes`. tools/verify.mjs
  // requires one entry for every claim that does not reach acceptance and none
  // for a claim that does.
  stops: {
    linux: "wrong-revision",
    "desktop-chromium": "run-failed",
    windows: "scope-of-the-check",
    launcher: "scope-of-the-check",
    "claude-verdict": "authorization",
  },
};

// ------------------------------------------------------------------ derivation
// Both P2 candidates publish this into the non-visible machine channel;
// tools/verify.mjs recomputes it from the raw arrays above rather than reading
// it off the page. It is never rendered into visible text.
(() => {
  const facts = window.tsk007Facts;
  const executedLanes = facts.lanes.filter((_, lane) =>
    facts.claims.some((claim) => claim.cells[lane]?.state === "executed"));
  const causesByLane = Object.fromEntries(facts.lanes.map((name, lane) => [
    name,
    [...new Set(facts.claims
      .map((claim) => facts.absenceCauses[`${claim.id}:${lane}`])
      .filter(Boolean))].sort(),
  ]));
  const share = Object.fromEntries(facts.causes.map((cause) => [
    cause.id,
    Object.values(facts.absenceCauses).filter((id) => id === cause.id).length,
  ]));
  // How far a claim's derivation travels, on a four-stage scale: produced,
  // artifact, acceptance, exact candidate. A claim carrying a recorded chain
  // states its own reach; the two that carry none are read off their cells —
  // an executed cell means the chain arrived, no executed cell means none was
  // ever produced. Nothing else is inferred.
  facts.stages = ["produced", "artifact", "acceptance", "exact candidate"];
  facts.reachOf = (claim) => (claim.provenance
    ? claim.provenance.reaches
    : (claim.cells.some((cell) => cell.state === "executed") ? 4 : 0));
  facts.answer = {
    executedLanes,
    causesByLane,
    absences: Object.keys(facts.absenceCauses).length,
    share,
  };
})();
