# Shadow Testing Architecture

Reference document for INF-TSK-021-030 (Phase C shadow test harness).

## Harness Flow

```text
┌─────────────────────────────────────────────────────────────────┐
│ registry.go — AllShadowTests(projectDir) — 25 test cases        │
│                                                                  │
│  Hooks (13):              Pathflow (8):     Validation (4):      │
│  • hook/security/bash     • pathflow/       • validate/task/     │
│  • hook/security/edit       phase-trans       real-task-file     │
│  • hook/pathflow-gate     • pathflow/       • validate/task/     │
│  • hook/sentinel-write      stage-trans       missing-file       │
│  • hook/checkpoint-reg    • pathflow/       • validate/epic/     │
│  • hook/checkpoint-done     session-reg       real-epic-file     │
│  • hook/session-start     • pathflow/       • validate/epic/     │
│  • hook/session-end         task-update       missing-file       │
│  • hook/edit-write-guard  • pathflow/                            │
│  • hook/gh-pr-guard         session-meta                         │
│  • hook/protection-guard  • pathflow/                            │
│  • hook/webfetch-guard      session-meta                         │
│  • hook/team-guard          (iteration=0)                        │
│                           • pathflow/                            │
│                             stage-trans                          │
│                             (iteration=0)                        │
└───────────────────────────────┬─────────────────────────────────┘
                                │ ShadowTest{Name, GoCommand,
                                │   ShellCommand, Stdin, Env}
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│ harness.go — ShadowTest.Run(ctx, projectDir, goBinary)          │
│                                                                  │
│              Same stdin / env / args                             │
│                    │           │                                 │
│          ┌─────────┘           └──────────┐                     │
│          ▼                                ▼                      │
│  ┌───────────────┐              ┌───────────────────┐           │
│  │  Shell script │              │   Go subcommand   │           │
│  │  (ShellCmd)   │              │   (GoCmd via      │           │
│  │               │              │    os.Executable) │           │
│  └───────┬───────┘              └────────┬──────────┘           │
│          │ ExecResult                    │ ExecResult            │
│          │ {stdout,stderr,exit}          │ {stdout,stderr,exit}  │
│          └──────────────┬───────────────┘                       │
│                         ▼                                        │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ NormalizeJSONLOutput(output, rules)                       │   │
│  │                                                           │   │
│  │  Per JSONL line — NormalizeJSONLEvent():                  │   │
│  │  • rename "type"      → "event"                          │   │
│  │  • rename "ts"        → "timestamp"                      │   │
│  │  • strip "id" field                                       │   │
│  │  • strip "event_id" field                                 │   │
│  │  • map "session_metadata" → "session_register"           │   │
│  │  • flatten key/value envelope → flat fields               │   │
│  │  • normalize timestamp values → "__TIMESTAMP__"          │   │
│  │  • rename "status" → "task_status" (task-update only)    │   │
│  └──────────────────────────┬───────────────────────────────┘   │
│                             ▼                                    │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ CompareOutputs(goNorm, shellNorm)                         │   │
│  │                                                           │   │
│  │  → ShadowResult{                                          │   │
│  │      GoResult    ExecResult                               │   │
│  │      ShellResult ExecResult                               │   │
│  │      Divergences      []Divergence  // unexpected diffs   │   │
│  │      KnownDivergences []Divergence  // expected diffs     │   │
│  │    }                                                      │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## Entry Points

```text
┌─────────────────────────────────────────────────────────────────┐
│ CLI (Go)                                                         │
│                                                                  │
│  codeflow shadow-test [--category hooks|pathflow|validate]       │
│                       [--verbose] [--project-dir .]             │
│                                                                  │
│  Exit 0 → all tests pass (zero unexpected divergences)          │
│  Exit 1 → unexpected divergences found (report printed)         │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│ Shell runner (test suite integration)                            │
│                                                                  │
│  .codeflow/testing/shadow/run-shadow-tests.sh                    │
│                                                                  │
│  Registered at MEDIUM priority in .codeflow/testing/test-config  │
│  Exit 0 → all pass  │  Exit 1 → unexpected divergences          │
└─────────────────────────────────────────────────────────────────┘
```

Both entry points exercise the same `AllShadowTests()` registry and `ShadowTest.Run()` harness.

## Migration Timeline

```text
Phase B (DONE)                Phase C (THIS TASK)           Phase E (NEXT)
──────────────────            ────────────────────          ──────────────
Build Go CLI                  Shadow harness proves         Cutover with
equivalents                   behavioral parity             confidence

TSK-001 → TSK-014    ──▶     TSK-030                ──▶   TSK-022
TSK-037                       Run Go + shell                Replace shell
                              side-by-side,                 invocations in
Go subcommands exist          compare normalized            settings.json
for all hooks,                outputs, log all              with Go binary
pathflow scripts,             divergences                   calls
and validation cmds

                              ┌────────────────┐
                              │ Go code exists │
                              │       +        │
                              │ Shadow harness │
                              │ proves same    │
                              │ behavior       │
                              └───────┬────────┘
                                      │
                                      ▼
                              ┌────────────────┐
                              │ Cutover with   │
                              │ confidence     │
                              └────────────────┘
```

## Key Insight

Shadow testing establishes **functional equivalence**, not byte-identical output.

Go and shell produce semantically identical data in structurally different forms
due to incremental schema evolution. Requiring byte-identical output would flood
results with false positives for every known schema difference (field renames,
absent fields, event type remappings), drowning real behavioral divergences in noise.

The normalization layer neutralizes all known structural differences before
comparison. What remains after normalization is real — any divergence that
survives normalization represents an actual behavioral difference that requires
investigation before cutover proceeds.
