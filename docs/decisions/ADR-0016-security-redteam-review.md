---
id: ADR-0016
title: "security / red-team review: dual-vendor adversarial stage plus deterministic scanner floor"
date: 2026-07-10
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — adds the security-review CI plane and the per-stack SCA/SAST scanner floor (cargo-audit / osv-scanner / pip-audit / govulncheck / semgrep) as new Tier-3 tool dependencies, and the `security_review` + `dep_audit` policy.json keys beside `secret_scan`; the architecture.md text lands with the implementing slice
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0016: security / red-team review

## Context

The `security` stage was a thin, optional one-liner (`pipeline.workflow.js:123-124`):
one line of prose emitting the generic `{verdict, findings: string[]}` shape — no
class, severity, CVSS, or file:line — and it was not in the default preset, so it
ran only if a caller named it and was trivially skippable. No dependency or
supply-chain scanner was wired anywhere in the repo, a concrete gap now that OWASP
promoted software supply chain to A03:2025 (a new category). Secrets, by contrast,
are already covered in two layers — the pre-commit scan
(`crates/codeflow-core/src/hooks/scan.rs`) and gitleaks in CI, with
`secret_scan: "block"` marked the one gate never relaxed — so any richer review
must **layer** on that floor, not duplicate it. The duo develop flow makes
security / red-team review mandatory and non-bypassable by each model; but the
pipeline file is user-owned (ADR-0004), so a "mandatory stage" written there is
convenience, not a boundary. Genuine non-bypassability has to bind in CI and
policy, exactly as the git rules do.

## Decision

Replace the one-line charge with a two-part mandatory security / red-team stage,
bound at three planes that copy the git-rules model.

**Part 1 — deterministic floor (mechanical, reproducible).** The floor that
ships is `osv-scanner` in CI — stack-agnostic SCA that reads every lockfile
ecosystem (Cargo.lock, package-lock.json, requirements.txt, go.mod, …), so one
job is the universal floor — plus the existing gitleaks + scan.rs for secrets and
entropy. The decided *target* is to grow this into a per-stack scanner table
living in core beside `testing/setup/detect.rs`, so the pipeline stage and the CI
job invoke the same commands: rust → `cargo audit` (optionally `cargo deny check
advisories`); node → `osv-scanner --recursive` (or `pnpm/npm audit
--audit-level=high`); python → `pip-audit`; go → `govulncheck ./...`;
any/multi/unknown → `osv-scanner --recursive` as the universal lockfile fallback,
plus `semgrep --config auto` for cross-language taint/SAST. That per-stack core
module is a future/optional extension — not yet built; only the universal
osv-scanner floor is live today. Each scanner carries a severity floor and an
allowlist (`.osv-scanner.toml`, semgrep ignores) so noise stays out of the dev's
way — the scan.rs placeholder-filter philosophy.

**Part 2 — dual-vendor adversarial review** (the `cf-security-reviewer` agent):
Claude as the defender lens (full repo context — triages every Part-1 scanner hit
for reachability and adds the classes scanners cannot see) and codex as the
attacker lens (read-only, assume-breach mandate, via the ADR-0005 invocation).
The split is deliberate: an attacker and a defender on the same model share blind
spots, so assigning different vendors breaks the correlated blindspot. The agent
carries a seven-axis checklist mapped to OWASP Top 10:2025, OWASP LLM Top 10:2025,
and CWE Top 25 (2025); CVSS-4.0 severity bands; and the `SecurityFinding` /
`SecurityVerdict` schema. The reviewer **sets** its `verdict` from the schema
enums (severity + confidence), never from prose, and the pipeline gate branches
on that verdict — the structured reasoning informs the verdict; the pipeline's
`{verdict, findings: string[]}` schema is a deliberate rework-compat choice.

The three planes:

- **Pipeline (local, warn + rework — not the boundary).** The duo entry point
  forces `security` into the effective preset and runs it before verify;
  block-rule findings → `changes_requested` → bounded rework. The file is
  user-owned, so this is convenience only.
- **CI (server-side, the authoritative block).** A required `security-review`
  job that (a) runs the Part-1 scanners per detected stack with `--exit-code 1`
  on High+, and (b) checks the committed structured-findings artifact is present
  and carries no unresolved High+ model findings. This is what a human must see
  green before merging.
- **Policy (the shared threshold).** New `security_review` and `dep_audit` keys
  in `.codeflow/policy.json` beside `secret_scan`, marked never-relaxed. The CI
  `security-review` job reads its gate level from policy.json rather than a
  hardcoded threshold — the same source-of-truth pattern the git planes use:
  `security_review` is the whole-job umbrella and `dep_audit` the SCA sub-gate,
  and the advisory blocks when *either* is `block`. (These keys are CI-read today;
  no local scanner hook consumes them. They ship coupled with their Rust struct
  fields, in the implementing slice; policy values without fields are not added.)

Split by determinism, because a flaky hard gate creates pressure to bypass —
which doctrine forbids. Deterministic High+ (SCA CVEs, detected secrets, Semgrep
criticals) hard-blocks CI non-overridably, the same status as
`secret_scan: "block"`. Model-reasoned findings are nondeterministic, so they
warn locally, force pipeline rework, and require a clean structured artifact
(present, no unresolved High+) for CI to pass — the human merger is the backstop
for the judgment a machine cannot adjudicate (ADR-0007).

Reconciliation with the existing secret layers: this **layers, it does not
duplicate.** The models never re-run secret regexes; they consume scan.rs +
gitleaks output as evidence and add reachability, PII, IaC, and
runtime-assembled-secret reasoning, deduped on (location, class). Degradation
follows ADR-0005: the deterministic floor is always mandatory; codex missing at
flow start degrades the whole flow to single-vendor silently, but codex missing
for a *requested* duo security stage — or dying mid-duo — blocks loudly, since
losing the second vendor defeats the correlated-blindspot reduction that justifies
the red team.

## Consequences

- The missing supply-chain / SCA gap (A03:2025) is closed, and the security stage
  gains structured, gate-able findings instead of prose.
- Adding cargo-audit / osv-scanner / semgrep / pip-audit / govulncheck are new
  external tool dependencies = Tier-3 decisions (this ADR + capability entries);
  they add scan-time cost, and their advisory DBs can fail a build on a
  newly-published CVE in unchanged code. A triage/allowlist path with a recorded
  justification is required so a transitive, unreachable advisory does not
  hard-block an unrelated PR while still being surfaced.
- Nondeterministic model findings are kept off the hard CI block by design; the
  hard block is deterministic scanner output only, routed through rework + human
  merge for judgment.
- The pipeline plane is convenience, not the perimeter (user-owned file, ADR-0004);
  the design must not be oversold as bound there.
- Remote caveat: on this repo remote branch protection is unavailable (private +
  GitHub Free → 403, ADR-0006), so "non-bypassable" reduces to a required CI
  status check that a human must see green before merging — strong, but not
  machine-enforced-at-push. A consumer repo with `codeflow remote protect` armed
  gets the `security-review` job as a required status check too.

## Architecture impact

`docs/architecture.md` gains the `security-review` CI enforcement plane and the
per-stack SCA/SAST scanner floor as new Tier-3 tool dependencies, and
`.codeflow/policy.json` gains the `security_review` + `dep_audit` keys beside
`secret_scan`. Those edits land in the same PR as the implementing slice (the
scanner module, CI job, and policy struct fields), where the plane becomes real;
this ADR records the decision ahead of that slice.

## Update (2026-07-11) — the implementing slice's shipped posture

The slice that implemented this decision landed with softer semantics than the
decision text in three places, recorded here so the ADR is not read as the
shipped state:

- **The policy keys are gated, not never-relaxed.** `security_review` and
  `dep_audit` shipped defaulting to `warn`, user-settable to `off`, and
  bootstrap grace (`suspend_for_bootstrap`) turns both off — only `secret_scan`
  keeps the never-relaxed status. The CI job reads the keys from policy.json as
  decided, but the hard block arms only when a repo hardens either key to
  `block`.
- **No High+ severity floor.** The shipped `security-review` job runs
  `osv-scanner scan -r .` with no severity filter: any advisory (or scan error)
  triggers the gated outcome. The `--exit-code 1` on High+ floor is future work
  alongside the per-stack scanner table.
- **No structured-findings artifact check.** The CI check that the committed
  structured-findings artifact is present with no unresolved High+ model
  findings was not built; the model layer's verdict is consumed by the pipeline
  gate and the human merger only. Also future work.

The secret layers are unchanged: gitleaks in CI and the pre-commit scan block
unconditionally, exactly as decided.
