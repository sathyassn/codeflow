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
//   stages     string[]?  ordered preset drawn from: analyze, plan, build,
//                         security, review, qa, verify.
//                         Default: ['build', 'review', 'verify'].
//   models     object?    per-stage model, e.g. { build: 'sonnet' }; every
//                         stage defaults to 'inherit' (the caller's model).
//   maxRework  number?    build-attempt budget shared by all gate back-edges
//                         (review/security/qa changes_requested -> build).
//                         Default 3.
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

if (typeof args.task !== 'string' || !Array.isArray(args.criteria) || args.criteria.length === 0) {
  throw new Error('pipeline: args.task (string) and args.criteria (non-empty string[]) are required');
}

const TASK = args.task;
const CRITERIA = args.criteria.map((c) => `- ${c}`).join('\n');
const WHERE = args.dir ? `Working directory: ${args.dir}.` : '';
const MAX_REWORK = args.maxRework ?? 3;
const PRESET = args.stages ?? ['build', 'review', 'verify'];

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
// validated run's reviewer shape; role text per .claude/agents/cf-reviewer.md.
const gate = (name, charge) => ({
  kind: 'gate',
  model: args.models?.[name] ?? 'inherit',
  schema: VERDICT,
  prompt: () => [
    `Independent ${name} review of the latest build for: ${TASK}`, WHERE,
    `Acceptance criteria:\n${CRITERIA}`,
    `Builder summary:\n${ctx.buildSummary || '(not captured)'}`,
    charge,
    'Role (.claude/agents/cf-reviewer.md): independent evaluator, read-only on code — never fix anything; every claim in the verdict needs evidence.',
    'Judge ONLY on what you observe: run the gates and the code yourself; cite file:line or command output per finding.',
    "Return verdict 'approved' only if every criterion is verified and no blocker or major finding remains; otherwise 'changes_requested' with one finding string per issue.",
  ].filter(Boolean).join('\n\n'),
});

// Stages as data: each row is { model: args.models?.<name> ?? 'inherit',
// prompt builder } plus optional schema/kind/apply. Add or reshape rows freely.
const STAGES = {
  analyze: {
    model: args.models?.analyze ?? 'inherit',
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
    model: args.models?.plan ?? 'inherit',
    schema: { type: 'object', required: ['spec'], properties: { spec: { type: 'string' } } },
    prompt: () => [
      `Draft a short implementation spec for: ${TASK}`, WHERE,
      `Acceptance criteria:\n${CRITERIA}`,
      ctx.analysis && `Analysis findings:\n${ctx.analysis}`,
      'Spec: ordered steps, interfaces and formats pinned down, test plan. Plain text; it is fed verbatim to the builder.',
    ].filter(Boolean).join('\n\n'),
    apply: (out) => { ctx.spec = out.spec; },
  },
  build: {
    model: args.models?.build ?? 'inherit',
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
    'Security lens: hunt secret exposure, injection (command/SQL/path/template), and authorization gaps in the changed code; any concrete instance is a blocker finding.'),
  review: gate('review',
    'Run `codeflow validate` and `codeflow test --mode quick`; any nonzero exit is an automatic changes_requested.'),
  qa: gate('qa',
    'QA lens: exercise each acceptance criterion against actual behavior — run the code and tests, record observed vs expected per criterion; any unmet criterion fails.'),
  verify: {
    kind: 'final',
    model: args.models?.verify ?? 'inherit',
    schema: VERDICT,
    prompt: () => [
      `Final verification gate for: ${TASK}`, WHERE,
      'Run `codeflow test --mode full` and report the exact exit code as a finding; any nonzero exit is changes_requested.',
      `Then check every acceptance criterion one by one, citing evidence per criterion in findings:\n${CRITERIA}`,
      "Verdict 'approved' only when the full test gate is green and every criterion is met.",
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
