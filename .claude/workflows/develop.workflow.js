// develop.workflow.js — CodeFlow v2 develop pipeline (Day 0 probe draft; charter section 12).
// Topology: build -> review -> (changes_requested -> build again, max 3) -> verify.
//
// Runtime contract: plain JS executed as an async-function body (top-level await/return
// valid); `agent(prompt, opts)` = awaited fresh-context subagent, `opts.schema` = parsed
// structured output, `opts.agent` = repo agent definition. Branches ONLY on schema enums
// and codeflow CLI exit codes — never on prose. See rework-loop.md for the full grounding.
//
// Input (gathered by /cf-develop): input.task (string), input.criteria (string[]).

const MAX_ATTEMPTS = 3; // bounded rework: at most 3 build->review iterations

const task = input.task;
const criteriaList = input.criteria.map((c) => `- ${c}`).join("\n");

const BUILD_SCHEMA = {
  type: "object",
  required: ["summary", "files_changed", "quick_test_exit"],
  properties: {
    summary: { type: "string" },
    files_changed: { type: "array", items: { type: "string" } },
    quick_test_exit: { type: "integer" }, // exit of `codeflow test --mode quick`
  },
};

const FINDING = {
  type: "object",
  required: ["criterion", "file", "line", "issue", "severity"],
  properties: {
    criterion: { type: "string" }, file: { type: "string" }, line: { type: "integer" },
    issue: { type: "string" }, severity: { type: "string", enum: ["blocker", "major", "minor"] },
  },
};

const REVIEW_SCHEMA = {
  type: "object",
  required: ["verdict", "findings"],
  properties: {
    verdict: { type: "string", enum: ["approved", "changes_requested"] },
    findings: { type: "array", items: FINDING },
  },
};

const VERIFY_SCHEMA = {
  type: "object",
  required: ["test_exit", "criteria_unmet"], // exit of `codeflow test --mode full`; unmet criteria verbatim
  properties: {
    test_exit: { type: "integer" },
    criteria_unmet: { type: "array", items: { type: "string" } },
  },
};

// ---- stages 1 + 2: bounded build -> review loop ----------------------------

let attempt = 0;
let build = null;
let review = { verdict: "changes_requested", findings: [] };

while (attempt < MAX_ATTEMPTS && review.verdict !== "approved") {
  attempt += 1;

  // Stage 1 — build. Workflow-prompted builder (no agent file, charter 4.4);
  // rework passes receive the reviewer's findings verbatim.
  build = await agent(
    [
      `Build attempt ${attempt}/${MAX_ATTEMPTS} for: ${task}`,
      `Acceptance criteria:\n${criteriaList}`,
      review.findings.length > 0
        ? `This is rework. Address every finding:\n${JSON.stringify(review.findings, null, 2)}`
        : "Initial build. Implement the criteria; no assumptions — surface ambiguity as a blocker, do not guess.",
      "Before reporting, run `codeflow test --mode quick` and report its exact exit code as quick_test_exit.",
    ].join("\n\n"),
    { schema: BUILD_SCHEMA },
  );

  // Builder's own gate: a failing quick test consumes the attempt without
  // spending an independent review on a build that cannot pass it.
  if (build.quick_test_exit !== 0) {
    review = { verdict: "changes_requested",
      findings: [{ criterion: "test gate", file: "-", line: 0, severity: "blocker",
        issue: `codeflow test --mode quick exited ${build.quick_test_exit}` }] };
    continue;
  }

  // Stage 2 — review. Independent evaluator (fresh context, cf-reviewer definition):
  // verifies the SAME criteria with file:line evidence, re-runs the gates itself.
  review = await agent(
    [
      `Review this build against the acceptance criteria. Task: ${task}`,
      `Criteria:\n${criteriaList}`,
      `Builder summary: ${build.summary}`,
      `Files changed:\n${build.files_changed.join("\n")}`,
      "Run `codeflow validate` and `codeflow test --mode quick`; any nonzero exit is an automatic changes_requested.",
      "Return verdict 'approved' only if every criterion is met, with file:line evidence per finding otherwise.",
    ].join("\n\n"),
    { agent: "cf-reviewer", schema: REVIEW_SCHEMA },
  );
}

if (review.verdict !== "approved") {
  // Bounded halt: cap exhausted — fail loud with the evidence trail (principle 8).
  throw new Error(`develop: changes still requested after ${MAX_ATTEMPTS} attempts; ` +
    `unresolved findings: ${JSON.stringify(review.findings)}`);
}

// ---- stage 3: verify --------------------------------------------------------
// Builder-role pass: full test gate + per-criterion check; only exit codes and
// schema fields are trusted.
const verify = await agent(
  [
    `Verification pass for: ${task}`,
    "Run `codeflow test --mode full` and report its exact exit code as test_exit.",
    `Then check each acceptance criterion and list any unmet ones verbatim in criteria_unmet:\n${criteriaList}`,
  ].join("\n\n"),
  { schema: VERIFY_SCHEMA },
);

if (verify.test_exit !== 0) {
  throw new Error(`develop: verify gate failed — codeflow test exited ${verify.test_exit}`);
}
if (verify.criteria_unmet.length > 0) {
  throw new Error(`develop: criteria unmet after approved review: ${verify.criteria_unmet.join("; ")}`);
}

// Deterministic completion record for the caller (/cf-develop) and the ledger.
return { status: "complete", attempts: attempt, files_changed: build.files_changed,
  review: review.verdict, test_exit: verify.test_exit };
