# cf-present Plan v5 review — 2026-08-01

## Scope

Plan v5 corrects the storage, process, privacy, replay, and cleanup boundary of
TSK-011 before implementation continues. It also materializes the missing
`update` and `resolve` commands required by SPC-004's already accepted immutable
revision and addressed/dismissed feedback behavior. It does not change the
product outcome, EPC-005's task graph, producer/reviewer ownership, or the
integration target.

## Settlement

Codex exact review of candidate `d1ab4e5e` found material gaps in browser-state
quota ownership, Windows leader-loss recovery and launch rollback, Windows
privacy at file creation, feedback retry convergence, interrupted temporary and
trash cleanup, retention accounting, cleanup isolation, and relative export
handling. Codex rejected the candidate for integration and approved the bounded
correction recorded in ADR-0052.

The directly invoked native Claude judgment primary ran as Fable 5/high in
Claude Code session `24253382-0aa6-432d-a330-098e7c6ccc4c`, prompt digest
`6a122f45b5bb1b6b9f68a755cf013a7e1664949ce4e37ac4ba7159338613473d`, with
no subagent delegation. It read SPC-004, TSK-011, ADR-0049, ADR-0050, and
sampled the exact candidate. Its verdict was `approved`, subject to binding
implementation refinements: Windows
absence proof is marker-keyed rather than parent-PID-only and treats unreadable
or failed enumeration as ambiguity; a profile/resource probe corroborates
absence; durable eviction and clear reap derived runtime after the same proof;
the project → session → runtime-control lock order is fixed and tested;
create-time DACL wording supersedes the earlier post-create mechanism; and
pre-release legacy in-session runtime directories are handled deliberately.

The task-local prompt called the amendment “Plan v3” because it ran from the
pre-integration TSK-011 branch. The integration target already contains the
independent EPC-005 Plan v3 and Plan v4 amendments. Materialization therefore
renumbers this identical safety-boundary amendment to EPC-005 Plan v5; no node,
edge, guard, owner, outcome, scope, or safety decision changed.

A second native Fable 5/high record review ran in Claude Code session
`418aa704-06c1-40f5-907c-677828266dbb`, turn `plan-v5-record`, prompt
`bd611996-eebc-4ac3-aa87-d832a141be20`. It approved the initial stable record
and surfaced one task-branch planning drift as non-blocking: `resolve` existed
only in the implementation branch's edited SPC-004. Codex classified that drift
as material rather than allowing it to ride into closeout, and also found that
the implemented immutable-revision lifecycle required an equally unrecorded
`update` command. Because those commands are the public means of exercising
accepted behavior, Plan v5 now adds both to SPC-004 and TSK-011 before work
resumes.

The corrected exact diff then received a fresh native interactive Claude Code
review in the same project session,
`418aa704-06c1-40f5-907c-677828266dbb`, using Fable 5/high with tool access and
no subagent delegation. The reviewer inspected the planning diff, both new
records, relevant SPC-004 context, and the committed CLI implementation that
made the interface drift observable. It found no material or blocking issue
and returned `PLAN_V5_CORRECTED_VERDICT: approved`. This verdict approves the
planning amendment only; it is not implementation qualification.

## Verification required after implementation

- Deterministic regression, concurrency, and crash tests cover every ADR-0052
  invariant, including runtime reaping, lock order, multi-eviction retention,
  isolated partial cleanup, exact retry semantics, and relative/Unicode export.
- Browser E2E includes live profile writes concurrent with durable feedback,
  update, retention, close, and cleanup without quota or permission races.
- macOS and Linux/WSL native evidence, Windows cross-build evidence, and native
  Windows process/DACL/runtime evidence remain separately labeled.
- Full warning-free local gates and an exact post-rebase Fable 5/high review
  bind the integration-ready SHA. Missing native evidence remains a release
  limitation, never an inferred pass.

This planning review verifies the approved correction only. It does not verify
the implementation or qualify a supported operating system.
