// pipeline.workflow.js — CodeFlow composable develop pipeline (stages as data).
//
// USER-OWNED: adapt freely — `codeflow update` never touches this file. It is
// seeded once as a reference implementation, never update-managed: sequencing
// frameworks rot, so this copy is yours to reshape per project (ADR-0004).
//
// When to use: unattended or batch runs, parallel fan-out, or when a preset is
// named explicitly. The interactive default is /cf-develop's inline
// build -> review -> verify loop — not this file.
//
// Args contract (the invoking command passes this object; runtime global `args`):
//   task       string     what to build (required)
//   criteria   string[]   testable acceptance criteria (required)
//   stages     string[]?  ordered preset drawn from: plan-align, analyze, plan,
//                         build, security, review, consult, qa, verify.
//                         Default: PRESETS.default = ['build', 'review', 'verify']
//                         (solo). PRESETS.duo = ['plan-align','build','security',
//                         'review','verify'] is the opt-in duo batch path
//                         (ADR-0015); its stages run single-vendor here — genuine
//                         cross-vendor duo work is interactive-only (ADR-0018).
//                         'consult' is an optional independent second read
//                         (single-vendor; ADR-0018); off unless named.
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

if (typeof A.task !== 'string' || !Array.isArray(A.criteria) || A.criteria.length === 0) {
  throw new Error('pipeline: A.task (string) and A.criteria (non-empty string[]) are required');
}

const TASK = A.task;
const CRITERIA = A.criteria.map((c) => `- ${c}`).join('\n');
const WHERE = A.dir ? `Working directory: ${A.dir}.` : '';
const MAX_REWORK = A.maxRework ?? 3;
// Named presets (opt-in via args.stages). The DEFAULT is unchanged — the solo
// build -> review -> verify path. 'duo' adds the plan-align convergence gate
// (ADR-0015) and the cf-security-reviewer red-team (ADR-0016). NOTE (ADR-0018):
// genuine cross-vendor duo work is interactive-only — the /cf-model-orchestrator
// skill drives the second vendor through the plugin (from Claude) or tmux (from
// codex). This unattended pipeline runs fresh-context subagents that reach
// neither interactive lane, and headless execution is prohibited, so 'duo' here
// runs plan-align and security SINGLE-VENDOR (Claude) over the deterministic
// scanner floor, recording the reduced assurance; for a real cross-vendor duo,
// run the work interactively through /cf-model-orchestrator. Pass a preset
// explicitly, e.g. args.stages = PRESETS.duo.
const PRESETS = {
  default: ['build', 'review', 'verify'],
  duo: ['plan-align', 'build', 'security', 'review', 'verify'],
};
const PRESET = A.stages ?? PRESETS.default;

// Schema verdict for every gate stage — branch on the enum, never on prose.
const VERDICT = {
  type: 'object',
  required: ['verdict', 'findings'],
  properties: {
    verdict: { type: 'string', enum: ['approved', 'changes_requested'] },
    findings: { type: 'array', items: { type: 'string' } },
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
const gate = (name, charge, { role, rule } = {}) => ({
  kind: 'gate',
  model: A.models?.[name] ?? 'inherit',
  schema: VERDICT,
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
  // Plan convergence gate (duo flow, ADR-0015 sibling to the
  // /cf-model-orchestrator skill): a rigorous, self-critiqued plan + acceptance
  // criteria before any build, producing the contract later. Genuine cross-vendor
  // convergence is interactive-only (ADR-0018) — see the preset note above.
  // stages grade against. Bounded rounds to agreement; no agreement stops for the
  // human. kind 'gate' so it shares the rework budget, but it sits BEFORE build,
  // so a changes_requested has no build to rework into and the loop throws loudly
  // with the disagreements attached — that throw IS the "stop for the human"
  // (never launder a disagreement into a default). Degrades to solo when codex is
  // absent. It writes the agreed contract into ctx.spec for the later stages.
  'plan-align': {
    kind: 'gate',
    model: A.models?.['plan-align'] ?? 'inherit',
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
      `Cross-vendor plan-align for: ${TASK}`, WHERE,
      `Acceptance criteria to ratify:\n${CRITERIA}`,
      ctx.analysis && `Analysis findings:\n${ctx.analysis}`,
      ctx.spec && `Draft spec:\n${ctx.spec}`,
      'Converge on a plan and acceptance criteria before any build. Genuine cross-vendor convergence — a second independently-trained model critiquing the plan — is a property of the INTERACTIVE /cf-model-orchestrator duo, which drives the second vendor through the ADR-0018 lane (the codex-plugin-cc plugin, from Claude). This unattended pipeline stage cannot reach that lane (headless execution is prohibited, ADR-0018), so draft the plan and ACs as Claude, then run an adversarial self-critique pass — challenge each assumption and every edge/error case — and record in a finding that cross-vendor convergence was not run here.',
      'Reconcile to agreement, bounded to <=2 rounds. Never launder a disagreement into a default.',
      'On agreement: return verdict approved and put the agreed, pinned, testable plan + acceptance-criteria contract in `contract` — later stages grade against it; findings may note residual assumptions.',
      'On no agreement after the bounded rounds: return verdict changes_requested with one finding per unresolved disagreement, and the pipeline halts for the human.',
    ].filter(Boolean).join('\n\n'),
    apply: (out) => { if (out.contract) ctx.spec = out.contract; },
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
    'Run the adversarial red-team. A genuine dual-vendor red-team — a second-vendor attacker lens independent of the Claude defender — is a property of the INTERACTIVE /cf-model-orchestrator duo (it drives the second vendor through the ADR-0018 lane, the codex-plugin-cc `/codex:adversarial-review`). This unattended pipeline stage cannot reach that lane (headless execution is prohibited, ADR-0018), so run the red-team SINGLE-VENDOR — Claude as both the defender lens (full repo context, triaging the deterministic-scanner floor for reachability) and the assume-breach attacker — and record in a finding that the cross-vendor lens was not run here; the deterministic scanner floor still hard-blocks High+ in CI regardless. Evidence-required: every finding needs a concrete untrusted-source-to-sink trigger, and approved is legal only with an attack_log of the assume-breach attempts actually made. Cover the seven axes — secret/PII exposure, injection (command/SQL/path/template/prompt), authz gaps, vulnerable/malicious deps, general vuln classes, and the agent code\'s own prompt-injection surface — each tagged to OWASP Top 10:2025 / OWASP LLM Top 10:2025 / CWE Top 25 (2025). Emit findings in the SecurityFinding/SecurityVerdict schema (class, severity, CVSS, evidence, confidence); surface each as one string carrying its class+severity+confidence. Layer on the deterministic scanner floor (which hard-blocks High+ in CI) — consume its output as evidence, never re-run the secret regexes. For a genuine cross-vendor red-team, run the work through the interactive /cf-model-orchestrator duo instead of this batch stage.',
    {
      role: 'Role (.claude/agents/cf-security-reviewer.md): load the cf-security-reviewer agent as the reviewer for this stage — it owns the deep seven-axis checklist. Independent evaluator, read-only on code — never fix anything; every claim in the verdict needs evidence.',
      rule: "SET your `verdict` from the SecurityFinding enums, never from prose — the gate branches on that verdict: return 'changes_requested' when any finding has severity in {critical,high} AND confidence in {confirmed,likely}; otherwise 'approved'. One finding string per issue.",
    }),
  review: gate('review',
    'Run `codeflow validate` and `codeflow test --mode quick --strict`; any nonzero exit is an automatic changes_requested. `--strict` makes a NoTargets run (the loud "nothing to run" banner — zero tests executed) exit non-zero: that is not-verified, treat it as changes_requested, never as a pass.'),
  qa: gate('qa',
    'QA lens: exercise each acceptance criterion against actual behavior — run the code and tests, record observed vs expected per criterion; any unmet criterion fails.'),
  // Optional independent-review consult: a fresh-context second read of the built
  // diff against the criteria. Genuine cross-vendor consult is interactive-only
  // (ADR-0018; the cf-consult skill), so this unattended stage runs single-vendor.
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
      'Genuine cross-vendor consult is interactive-only (the cf-consult skill, ADR-0018); this unattended stage cannot reach that lane, so perform a rigorous independent second read yourself in this fresh context.',
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
  const stage = STAGES[name];
  if (!stage) throw new Error(`pipeline: unknown stage '${name}'; known: ${Object.keys(STAGES).join(', ')}`);

  if (name === 'build') attempts += 1;
  const opts = { label: `${name}#${trail.length + 1}`, model: stage.model };
  if (stage.schema) opts.schema = stage.schema;
  const out = await agent(stage.prompt(), opts);
  if (stage.apply) stage.apply(out);
  // The build just consumed ctx.findings in its prompt; clear it so only findings
  // from a gate failure that follows are surfaced (a later build in a multi-build
  // preset must not be told it is rework for a prior loop's findings).
  if (name === 'build') ctx.findings = [];
  trail.push({ stage: name, verdict: out?.verdict ?? null, findings: out?.findings ?? null });

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
