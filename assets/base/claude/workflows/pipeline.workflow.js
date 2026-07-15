// pipeline.workflow.js — CodeFlow composable develop pipeline (stages as data).
//
// USER-OWNED: adapt freely — `codeflow update` never touches this file. It is
// seeded once as a reference implementation, never update-managed: sequencing
// frameworks rot, so this copy is yours to reshape per project (ADR-0004).
//
// When to use: unattended or batch runs, parallel fan-out, or when a preset is
// named explicitly. The interactive default is /cf-model-orchestrator when
// both native seats are available, with /cf-develop as the solo fallback.
//
// Args contract (the invoking command passes this object; runtime global `args`):
//   task       string     what to build (required)
//   criteria   string[]   testable acceptance criteria (required)
//   preset     string?    'default' or 'single-vendor-assurance'. The old
//                         'duo' name is rejected: a batch workflow cannot run
//                         the native interactive cross-vendor lanes.
//   stages     string[]?  explicit ordered stages drawn from: plan-audit,
//                         analyze, plan,
//                         build, security, review, consult, qa, verify.
//                         Default: PRESETS.default = ['build', 'review', 'verify']
//                         (solo). PRESETS['single-vendor-assurance'] adds a
//                         plan audit and security pass while stating its reduced
//                         assurance. Genuine duo work is interactive-only
//                         (ADR-0023).
//                         'consult' is an optional independent second read
//                         (single-vendor; ADR-0023); off unless named.
//   models     object?    per-stage model, e.g. { build: 'sonnet' }; every
//                         stage defaults to 'inherit' (the caller's model).
//   maxRework  number?    total build-attempt budget shared by all gate back-edges
//                         (review/security/qa changes_requested -> build). Counts
//                         the initial build, so N permits N-1 reworks. Default 3.
//   dir        string?    working-directory context added to stage prompts.
//
// Runtime contract: plain JS executed as an async-function body — no imports,
// no Date.now/Math.random. `await agent(prompt, opts)` is a fresh-context
// subagent; `opts.schema` yields parsed structured output. Branch only on
// schema enums and exit codes, never on prose. Mechanics (stages-as-data,
// per-stage model, schema verdicts, bounded rework) validated live in run
// wf_74f22cff-d1a; see docs/decisions/ADR-0004-composable-pipeline.md.

export const meta = {
  name: 'pipeline',
  description: 'CodeFlow composable develop pipeline: stages-as-data with bounded rework',
  phases: [{ title: 'Pipeline' }],
}

// Runtime quirk (probe wf_7c980f48-ec3): scriptPath invocations deliver
// `args` as a JSON string; parse defensively before use.
const A = typeof args === 'string' ? JSON.parse(args) : (args ?? {});

if (typeof A.task !== 'string' || A.task.trim() === '' ||
    !Array.isArray(A.criteria) || A.criteria.length === 0 ||
    !A.criteria.every((criterion) => typeof criterion === 'string' && criterion.trim() !== '')) {
  throw new Error('pipeline: A.task (non-empty string) and A.criteria (non-empty string[]) are required');
}
if (A.preset !== undefined && typeof A.preset !== 'string') {
  throw new Error('pipeline: A.preset must be a string');
}
if (A.stages !== undefined &&
    (!Array.isArray(A.stages) || A.stages.length === 0 ||
     !A.stages.every((stage) => typeof stage === 'string' && stage.trim() !== ''))) {
  throw new Error('pipeline: A.stages must be a non-empty string[]');
}
if (A.maxRework !== undefined && (!Number.isInteger(A.maxRework) || A.maxRework < 1)) {
  throw new Error('pipeline: A.maxRework must be a positive integer');
}
if (A.dir !== undefined && typeof A.dir !== 'string') {
  throw new Error('pipeline: A.dir must be a string');
}

const TASK = A.task;
const CRITERIA = A.criteria.map((c) => `- ${c}`).join('\n');
const WHERE = A.dir ? `Working directory: ${A.dir}.` : '';
const MAX_REWORK = A.maxRework ?? 3;
// Named presets. This Claude workflow can run fresh-context Claude agents but
// cannot reach either native interactive peer lane. The assurance preset is
// therefore honestly single-vendor; the real duo runs interactively through
// /cf-model-orchestrator (ADR-0023).
const PRESETS = {
  default: ['build', 'review', 'verify'],
  'single-vendor-assurance': ['plan-audit', 'build', 'security', 'review', 'verify'],
};
if (A.preset === 'duo' || A.stages?.includes('plan-align')) {
  throw new Error('pipeline: genuine duo work requires the interactive /cf-model-orchestrator flow; batch duo is unsupported (ADR-0023)');
}
if (A.preset && !Object.prototype.hasOwnProperty.call(PRESETS, A.preset)) {
  throw new Error(`pipeline: unknown preset '${A.preset}'; known: ${Object.keys(PRESETS).join(', ')}`);
}
if (A.preset && A.stages) {
  throw new Error('pipeline: choose either A.preset or A.stages, not both');
}
const PRESET = A.stages ?? PRESETS[A.preset ?? 'default'];

// Schema verdict for every gate stage — branch on the enum, never on prose.
const VERDICT = {
  type: 'object',
  required: ['verdict', 'findings'],
  properties: {
    verdict: { type: 'string', enum: ['approved', 'changes_requested'] },
    findings: { type: 'array', items: { type: 'string' } },
  },
};
const SECURITY_VERDICT = {
  ...VERDICT,
  required: [...VERDICT.required, 'attack_log'],
  properties: {
    ...VERDICT.properties,
    attack_log: { type: 'array', minItems: 1, items: { type: 'string' } },
  },
};

// Context fed forward between stages; gates write findings, build consumes them.
let attempts = 0; // build executions — the shared rework budget counts these
const ctx = { analysis: '', spec: '', findings: [], buildSummary: '' };

// Independent gate stage: evidence-required reviewer prompt mirroring the
// validated run's reviewer shape. Defaults to the cf-reviewer role and its
// blocker/major approval rule; a stage owned by a different agent (security)
// passes its own `role` and `rule` so the prompt carries exactly one role and
// one verdict rule — never two contradicting ones.
const gate = (name, charge, { role, rule, schema } = {}) => ({
  kind: 'gate',
  model: A.models?.[name] ?? 'inherit',
  schema: schema ?? VERDICT,
  prompt: () => [
    `Independent ${name} review of the latest build for: ${TASK}`, WHERE,
    `Acceptance criteria:\n${CRITERIA}`,
    `Builder summary:\n${ctx.buildSummary || '(not captured)'}`,
    charge,
    role ?? 'Role (.claude/agents/cf-reviewer.md): independent evaluator, read-only on code — never fix anything; every claim in the verdict needs evidence.',
    'Judge ONLY on what you observe: run the gates and the code yourself; cite file:line or command output per finding.',
    rule ?? "Return verdict 'approved' only if every criterion is verified and no blocker or major finding remains; otherwise 'changes_requested' with one finding string per issue.",
  ].filter(Boolean).join('\n\n'),
});

// Stages as data: each row is { model: args.models?.<name> ?? 'inherit',
// prompt builder } plus optional schema/kind/apply. Add or reshape rows freely.
const STAGES = {
  analyze: {
    model: A.models?.analyze ?? 'inherit',
    schema: { type: 'object', required: ['findings'],
      properties: { findings: { type: 'array', items: { type: 'string' } } } },
    prompt: () => [
      `Analyze the codebase ahead of building: ${TASK}`, WHERE,
      `Acceptance criteria:\n${CRITERIA}`,
      'Read docs/capabilities.md, docs/architecture.md, and the code paths this touches. Return findings: existing behavior, constraints, risks, and what the build must not break — one string each.',
    ].filter(Boolean).join('\n\n'),
    apply: (out) => { ctx.analysis = out.findings.map((f) => `- ${f}`).join('\n'); },
  },
  plan: {
    model: A.models?.plan ?? 'inherit',
    schema: { type: 'object', required: ['spec'], properties: { spec: { type: 'string' } } },
    prompt: () => [
      `Draft a short implementation spec for: ${TASK}`, WHERE,
      `Acceptance criteria:\n${CRITERIA}`,
      ctx.analysis && `Analysis findings:\n${ctx.analysis}`,
      'Spec: ordered steps, interfaces and formats pinned down, test plan. Plain text; it is fed verbatim to the builder.',
    ].filter(Boolean).join('\n\n'),
    apply: (out) => { ctx.spec = out.spec; },
  },
  // Single-vendor plan audit. This is deliberately not called duo or
  // cross-vendor convergence; the interactive orchestrator owns that contract.
  // kind 'gate' makes a rejected pre-build plan stop loudly because there is no
  // earlier build stage to rework into. An approved contract feeds later stages.
  'plan-audit': {
    kind: 'gate',
    model: A.models?.['plan-audit'] ?? 'inherit',
    schema: {
      type: 'object',
      required: ['verdict', 'findings'],
      properties: {
        verdict: { type: 'string', enum: ['approved', 'changes_requested'] },
        findings: { type: 'array', items: { type: 'string' } },
        contract: { type: 'string' },
      },
    },
    prompt: () => [
      `Single-vendor plan audit for: ${TASK}`, WHERE,
      `Acceptance criteria to ratify:\n${CRITERIA}`,
      ctx.analysis && `Analysis findings:\n${ctx.analysis}`,
      ctx.spec && `Draft spec:\n${ctx.spec}`,
      'Draft the plan and acceptance criteria, then run an adversarial fresh-context self-critique over every assumption and edge/error case. Record clearly that cross-vendor convergence was not run; the interactive /cf-model-orchestrator flow is required for dual approval (ADR-0023).',
      'Return approved only with a pinned, testable plan + acceptance-criteria contract in `contract`; otherwise return changes_requested with one finding per unresolved defect.',
    ].filter(Boolean).join('\n\n'),
    apply: (out) => {
      if (out.verdict === 'approved' &&
          (typeof out.contract !== 'string' || out.contract.trim() === '')) {
        throw new Error('pipeline: an approved plan-audit must return a non-empty contract');
      }
      if (out.contract) ctx.spec = out.contract;
    },
  },
  build: {
    model: A.models?.build ?? 'inherit',
    prompt: () => [
      `Build attempt ${attempts}/${MAX_REWORK} for: ${TASK}`, WHERE,
      `Acceptance criteria:\n${CRITERIA}`,
      ctx.analysis && `Analysis findings:\n${ctx.analysis}`,
      ctx.spec && `Implementation spec:\n${ctx.spec}`,
      ctx.findings.length > 0
        ? `This is rework. Address every finding:\n${ctx.findings.map((f) => `- ${f}`).join('\n')}`
        : 'Initial build. Implement the criteria with tests; surface ambiguity as a blocker, never guess.',
      'Commit conventionally (type(scope): description). Return a summary of what changed, with file paths.',
    ].filter(Boolean).join('\n\n'),
    apply: (out) => { ctx.buildSummary = typeof out === 'string' ? out : JSON.stringify(out); },
  },
  security: gate('security',
    'Run the adversarial red-team. A genuine dual-vendor attacker/defender pass belongs to the INTERACTIVE /cf-model-orchestrator flow through its host-appropriate native lane (ADR-0023). This unattended workflow cannot reach either lane, so run the red-team SINGLE-VENDOR — use separate defender and assume-breach attacker contexts — and record that the cross-vendor lens was not run. Deterministic scanners retain their configured authority: secret findings always block, while dependency/SCA findings follow the project\'s security_review and dep_audit policy. Evidence-required: every finding needs a concrete untrusted-source-to-sink trigger, and approved is legal only with an attack_log of the assume-breach attempts actually made. Cover secret/PII exposure, injection (command/SQL/path/template/prompt), authz gaps, vulnerable/malicious deps, general vuln classes, and the agent code\'s own prompt-injection surface, tagged to the project\'s current OWASP/CWE baselines. Emit findings in the SecurityFinding/SecurityVerdict schema (class, severity, CVSS, evidence, confidence). Consume deterministic scanner output as evidence; never let a model verdict override a red scanner. For a genuine cross-vendor red-team, run the interactive duo.',
    {
      schema: SECURITY_VERDICT,
      role: 'Role (.claude/agents/cf-security-reviewer.md): load the cf-security-reviewer agent as the reviewer for this stage — it owns the deep seven-axis checklist. Independent evaluator, read-only on code — never fix anything; every claim in the verdict needs evidence.',
      rule: "SET your `verdict` from the SecurityFinding enums, never from prose — the gate branches on that verdict: return 'changes_requested' when any finding has severity in {critical,high} AND confidence in {confirmed,likely}; otherwise 'approved'. One finding string per issue.",
    }),
  review: gate('review',
    'Run `codeflow validate` and `codeflow test --mode quick --strict`; any nonzero exit is an automatic changes_requested. `--strict` makes a NoTargets run (the loud "nothing to run" banner — zero tests executed) exit non-zero: that is not-verified, treat it as changes_requested, never as a pass.'),
  qa: gate('qa',
    'QA lens: exercise each acceptance criterion against actual behavior — run the code and tests, record observed vs expected per criterion; any unmet criterion fails.'),
  // Optional independent-review consult: a fresh-context second read of the built
  // diff against the criteria. Genuine cross-vendor consult is interactive-only
  // (ADR-0023; the cf-consult skill), so this unattended stage runs single-vendor.
  // kind 'gate' so its verdict shares the one rework budget with review; off
  // unless 'consult' is in the preset.
  consult: {
    kind: 'gate',
    model: A.models?.consult ?? 'inherit',
    schema: VERDICT,
    prompt: () => [
      `Independent fresh-context consult on the latest build for: ${TASK}`, WHERE,
      `Acceptance criteria:\n${CRITERIA}`,
      `Builder summary:\n${ctx.buildSummary || '(not captured)'}`,
      'Genuine cross-vendor consult is interactive-only (the cf-consult skill, ADR-0023); this unattended stage cannot reach that lane, so perform a rigorous independent second read yourself in this fresh context.',
      'Review the built diff against the criteria, cite file:line, and challenge the builder summary rather than trusting it — verify each point against the actual diff.',
      "Return verdict 'changes_requested' ONLY for substantive defects you can confirm; otherwise 'approved'. One finding string per issue.",
    ].filter(Boolean).join('\n\n'),
  },
  verify: {
    kind: 'final',
    model: A.models?.verify ?? 'inherit',
    schema: VERDICT,
    prompt: () => [
      `Final verification gate for: ${TASK}`, WHERE,
      'Run `codeflow test --mode full --strict` and report the exact exit code as a finding; any nonzero exit is changes_requested. `--strict` makes a NoTargets run (the loud "nothing to run" banner — zero tests executed) exit non-zero: a no-op is not-verified, report it as changes_requested, never approved.',
      `Then check every acceptance criterion one by one, citing evidence per criterion in findings:\n${CRITERIA}`,
      "Verdict 'approved' only when the full test gate actually ran and is green (never on a NoTargets/no-op run) and every criterion is met.",
    ].filter(Boolean).join('\n\n'),
  },
};

// Driver: statement-order sequencing over the preset, with bounded back-edges.
// review/security/qa changes_requested -> most recent build stage, all sharing
// one budget (MAX_REWORK build executions — the validated-run semantics).
// verify failure and budget exhaustion throw loudly with the findings attached.
const trail = [];
let i = 0;
while (i < PRESET.length) {
  const name = PRESET[i];
  if (!Object.prototype.hasOwnProperty.call(STAGES, name)) {
    throw new Error(`pipeline: unknown stage '${name}'; known: ${Object.keys(STAGES).join(', ')}`);
  }
  const stage = STAGES[name];

  if (name === 'build') attempts += 1;
  const opts = { label: `${name}#${trail.length + 1}`, model: stage.model };
  if (stage.schema) opts.schema = stage.schema;
  const out = await agent(stage.prompt(), opts);
  if (stage.apply) stage.apply(out);
  // The build just consumed ctx.findings in its prompt; clear it so only findings
  // from a gate failure that follows are surfaced (a later build in a multi-build
  // preset must not be told it is rework for a prior loop's findings).
  if (name === 'build') ctx.findings = [];
  trail.push({
    stage: name,
    verdict: out?.verdict ?? null,
    findings: out?.findings ?? null,
    attack_log: out?.attack_log ?? null,
  });

  if ((stage.kind === 'gate' || stage.kind === 'final') && out.verdict !== 'approved') {
    ctx.findings = out.findings;
    if (stage.kind === 'final') {
      throw new Error(`pipeline: verify gate failed — ${out.findings.join('; ')}`);
    }
    const back = PRESET.lastIndexOf('build', i);
    if (back === -1) {
      throw new Error(`pipeline: '${name}' requested changes but the preset has no build stage to rework into — ${out.findings.join('; ')}`);
    }
    if (attempts >= MAX_REWORK) {
      throw new Error(`pipeline: rework budget exhausted after ${attempts} build attempts; unresolved ${name} findings: ${JSON.stringify(out.findings)}`);
    }
    log(`rework (attempt ${attempts}, ${name}): ${out.findings.join('; ')}`);
    i = back;
    continue;
  }
  i += 1;
}

return { status: 'complete', attempts, trail };
