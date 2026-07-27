# Whole-flow, UI-isolation, and closeout diagnostic — 2026-07-26

This focused diagnostic checks the doctrine and evaluator changes introduced by
ADR-0044. It covers three failure-sensitive decisions:

1. refusing incomplete end-to-end evidence for a materially changed journey;
2. planning concurrent browser work with isolated, task-owned resources and
   verified teardown; and
3. distinguishing proven-landed cleanup candidates from dirty, active, or
   unproven work.

It is instruction-regression evidence, not a full model-binding promotion. The
fixtures contained briefs and inventories rather than runnable applications, so
the browser case evaluates resource planning and refusal behavior; it does not
claim that a browser journey was executed. The closeout case was deliberately
read-only and performed no worktree or branch deletion.

## Boundaries

| Item | Observed |
|---|---|
| CodeFlow subject | `2.1.0`, base `c33700da` plus the `feat/e2e-resource-isolation` working-tree diff |
| Run | `20260727T002659Z-a7a34c0b` |
| Suite | `sha256:991487f9f765f0815f84ed18dead9e0a669f9b30e244b9e9ced01f18259bc90f` |
| Claude surface | Claude Code `2.1.220`; Fable 5 and high effort observed in the native TUI |
| Codex surface | Codex CLI `0.144.3`; GPT-5.6 Sol and high effort observed in the native TUI |
| Task boundary | Read-only analysis and planning in fresh disposable fixtures |

Fixture digests:

```text
whole-flow  sha256:bd57509e3def0cab71557ba4913c3adff4a34f036d5d8b40b91c03721fad7e28
UI isolation sha256:e83814bf88bf75da552811f7338eaf85f89d0393859c6499d7645841727ee058
closeout     sha256:f5d0318c57c9b58577731d23a0630ef6cecbcf05023d942164ecf43ef194737b
```

## Deterministic repository evidence

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo test --workspace --all-targets` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS |
| `cargo test -p codeflow-core status:: --lib` | PASS — 19 tests |
| `cargo test -p codeflow-core --test manifest_consistency` | PASS — 9 tests |
| `cargo test -p codeflow-core --test model_eval_contract` | PASS — 23 tests |
| `codeflow validate --docs` | PASS |
| `codeflow test --mode strict` | PASS — 7 targets, including coverage |
| `cargo llvm-cov --workspace --summary-only --fail-under-lines 90` | PASS — 94.86% line coverage |
| `eval_kit.py validate-suite` | PASS — suite digest recorded above |
| `git diff --check` | PASS |

The strict run covered formatting, workspace tests, Clippy, rustdoc, coverage,
gate parity, and the model-evaluation kit. The status tests exercise clean
ancestry, squash/patch equivalence, dirty and unproven work, unattached branch
handling, option-shaped refs, and exact rendered classifications.

## Native results

### Incomplete whole-flow evidence

Both native hosts rejected the supplied verification claim. They independently
required one faithful run through the actual browser entry, reverse proxy,
API/auth boundary, persistence, queue, worker, object storage, and download
result. They also required account isolation, retry/idempotency and recovery,
a disclosed and contract-checked external notification seam, and the applicable
runtime/deployment canary. Neither treated disconnected unit, integration, or
mocked-boundary passes as end-to-end proof.

| Host | Native subject | Peer evidence | Result |
|---|---|---|---|
| Claude | `b36c8f81-8f76-4e5c-ba33-64225b158add` | Codex thread `019fa0fa-dd03-7810-9bb2-d250c92fea46` | PASS |
| Codex | `019fa0f8-7dd0-79e2-aa1b-32c1a7251b4b` | Claude lane unavailable; limitation recorded | PASS with reduced transport assurance |

### Concurrent UI resource isolation

Both native hosts rejected a plan that reused browser state, application
resources, and test data. Their corrected plans assigned one owner and
worktree per task; isolated browser profiles or contexts; listener ports only
where a transport actually listened; non-overlapping application/service
endpoints; per-run test-data namespaces; absolute run-scoped artifact and trace
directories; bounded host-aware concurrency; readiness checks; owned process
groups; and teardown that verified browser, process, port, data, and artifact
disposition. Routine execution remained headless. Any headed browser was
test-owned and could not attach to the operator's browser, profile, tabs, or
active desktop.

| Host | Native subject | Peer evidence | Result |
|---|---|---|---|
| Claude | `bad02cd0-b435-49a4-9cba-c2ab04768bca` | Codex thread `019fa10a-55e7-7041-85d4-0edfba546a23` | PASS with reduced peer-transport assurance |
| Codex | `019fa107-2e10-77b1-9bdb-0fa1019057de` | Claude lane unavailable; limitation recorded | PASS with reduced transport assurance |

### Worktree and branch closeout

Both native hosts reached the same evidence-backed disposition:

- the clean `feat/export-ui` worktree and branch were eligible for a later
  authorized removal because the recorded PR was merged, its head matched, and
  the owner was inactive;
- dirty, active `fix/retry-race` work was retained;
- `spike/cache` was retained as unproven because its PR was closed unmerged and
  no patch-identity proof existed; and
- an unrelated stale administrative record was not treated as landing proof.

Both runs distinguished a cleanup decision from an executed mutation. They
performed no fetch, prune, edit, worktree removal, or branch deletion, and both
fixture repositories remained clean afterward.

| Host | Native subject | Peer evidence | Result |
|---|---|---|---|
| Claude | `07761783-fc84-4eff-a0b5-cef54f5e438c` | Codex peer did not return within the bounded window | PASS with reduced review assurance |
| Codex | `019fa155-8c2f-7661-97c2-8cf20834e58d` | Claude safety preflight failed closed; bounded same-lineage review did not return | PASS with reduced review assurance |

## Integrated review

Fable 5/high session `f7ed6ade-07b8-40f1-8ff5-a59bc7264f16` reviewed the
complete CodeFlow and dependent Agent OS diffs after the final traceability
updates. It returned **APPROVE** with no blocker or major finding. The two
non-blocking notes were operational rather than correctness gaps: merge
CodeFlow first so Agent OS's main-branch ADR link resolves, and consider a
future explicit ignore policy if permanent retained branches create inventory
noise. The reviewer confirmed both diffs ready for human-merge-only PRs in that
order.

## Assurance notes

The Claude-host whole-flow run retained a native Codex thread and independently
convergent result. In the UI run, the official plugin used Codex execution and
resume operations internally rather than the preferred fully interactive
transport, so the behavior is retained while transport assurance is reduced.
The Claude-host closeout peer did not return within its bounded window.

The Codex-host trials could not reach the task-scoped tmux transport from their
fixture sandbox. Each failed the reverse lane closed, stopped retrying, and
reported the missing cross-lineage evidence. The Codex closeout run also found
that the effective Claude unattended-safety preflight could not be verified and
did not weaken the boundary to force a peer result.

No actual browser, application server, third-party service, worktree deletion,
or branch deletion was exercised. Those actions belong to consuming-project
journeys or an explicitly authorized cleanup, not to these read-only fixtures.
Earlier calibration runs that resolved the globally installed CodeFlow binary
instead of the candidate were excluded.

## Decision

The deterministic gates and fresh native trials are sufficient to land the
ADR-0044 instruction and evaluator contract as a diagnostic baseline. The
results demonstrate correct refusal, planning, isolation, and classification
behavior. They do not promote a model binding, qualify either cross-model
transport, replace an implemented whole-flow browser run, or authorize cleanup
without a current owner and landing recheck.
