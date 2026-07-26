# Model-role and quality diagnostic — 2026-07-26

This targeted native-interactive diagnostic checks the new positive
primary-resolution, test-integrity, and performance/concurrency cases. It is
not a full binding qualification or promotion result.

## Boundaries

| Item | Observed |
|---|---|
| Claude subject | Claude Code 2.1.220; Fable 5 high shown by the native TUI |
| Codex subject | Codex CLI 0.144.3; GPT-5.6 Sol high shown by the native TUI |
| CodeFlow base | `7817b91c` plus working-tree diff `ff35fd332895c10f18d102fd39881fa168b620781ab054d07f9df950c8c55eca` |
| Agent OS base | `e32b87f` plus working-tree diff `85210128679a080679020abac2e19c539263c5df7ab1091ffcd36a278ab373a0` |
| Subject boundary | Fresh native interactive sessions; read-only prompt; no peer invocation or repository mutation |

## Results

| Case | Fable | Codex | Observed judgment |
|---|---|---|---|
| Managed default resolves both primaries | Semantic pass | Pass | Both selected the managed Claude and Codex primaries, high default effort, independent pre-exchange planning, direct primary invocation, owning-primary internal routing, and no worker substitution. Fable labeled preflight `proceed`; Codex labeled the requested planning artifact `plan`. |
| Coverage-gaming tests | Pass | Pass | Both rejected copied production logic as an oracle, mock call order as behavioral proof, and test-only production branches despite 94% coverage; both required independently anchored observable tests. |
| Performance/concurrency operating risk | Pass | Pass | Both found N+1 access, unbounded fan-out/memory, missing backpressure/cancellation, lost-update races, and retry/idempotency risk, then selected bounded load/stress and concurrency evidence. |

Response SHA-256:

- Fable: `924cf10bf671621229b6704d8cd7adccf42019ed4b7890e48936aec6d34d16ca`
- Codex: `c7e07e86bf9c0b80c052364c85102aae704121a489af7412b1db52cd518a4d4f`

The retained temporary responses remain outside the repository and contain no
secret or credential material.

## Interpretation

The run shows that both current primary model classes understand the managed
seat split and detect the two quality traps when presented directly. It does
not prove live cross-harness dispatch, full independent-plan reconciliation,
all evaluation-envelope fields, repeated reliability, applied model/effort
telemetry beyond the TUI, or promotion fitness. Fable's `proceed` label is a
semantic pass for CodeFlow's case but would require an exact-envelope rerun
before counting as an Agent OS `action: plan` pass. Those claims remain the
work of the full qualification protocol.
