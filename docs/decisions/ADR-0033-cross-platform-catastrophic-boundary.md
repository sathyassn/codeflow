---
id: ADR-0033
title: make the catastrophic-action boundary cross-platform
date: 2026-07-18
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — the shell enforcement plane and harness sandbox boundary now distinguish macOS, Linux/WSL2, and native Windows
---

# ADR-0033: cross-platform catastrophic-action boundary

## Context

The destructive-command guard protected Linux roots and devices but omitted
macOS system trees and native Windows PowerShell, disk, recovery, and permission
operations. Its project policy level could also be weakened even though the
documentation described it as a floor. Windows had no release artifact, while
the scaffold gave no precise distinction between native Windows and WSL2
sandbox assurance.

## Decision

Make catastrophic command classification a non-relaxable CodeFlow floor. Keep
ordinary recoverable project edits and deletions autonomous, but block recursive
deletion of OS, drive, user/profile, mount, and network-share roots; destructive
disk/filesystem/recovery operations; and recursive system ownership/permission
changes across Linux/WSL2, macOS, and native Windows. Wire the shared guard to
Bash and PowerShell payloads and prove both blocked and allowed canaries.

Ship an x86-64 Windows MSVC artifact and PowerShell installer, test it on a
native Windows CI runner, and let consuming test targets select `auto`, `sh`,
`powershell`, or `cmd`. `auto` uses `sh` on macOS/Linux/WSL2 and `cmd.exe` on
native Windows, so generated `&&` chains need no extra shell. Codex selects its
elevated native Windows sandbox. Claude's
native Windows shell remains supported for ordinary work, but because Claude
does not provide an OS sandbox there, catastrophic work requires WSL2 or a
container and fails closed when neither is available.

Keep cargo-dist native runners as the release authority and add optional pinned
`cargo-xwin` and `cargo-zigbuild` aliases for host-agnostic Windows and Linux
cross-build checks. Cross compilation proves build/link portability; it does
not replace native execution or installer selection tests. macOS release builds
remain on macOS because the Apple SDK is not redistributed by CodeFlow.

Model consensus, Claude Auto, Codex auto-review, and peer approval never grant
human authorization. Before an otherwise necessary high-blast-radius action,
the host records exact scope, preview/dry-run evidence where available, a
current verified checkpoint or backup and restore path, and authenticated human
approval.
An operation in the non-relaxable deterministic class remains blocked to the
agent after approval: the authenticated human performs it through a separate
controlled operator channel while the models prepare evidence and verify the
reported postcondition. Only other high-blast-radius operations that effective
host policy permits after approval may be executed by the host model.
Those proceed one bounded step at a time with postcondition verification.

## Consequences

- The safety floor and its tests are portable without making project cleanup
  unnecessarily interactive.
- Windows becomes a release platform, while WSL2 remains the higher-assurance
  Claude path and the natural Linux-toolchain path.
- Consumers may tighten policy but cannot configure the catastrophic floor to
  warn, allow, or off.
- Static command classification is defense in depth, not an authorization or
  endpoint-security oracle. Managed policy, least-privilege accounts, backups,
  and native sandboxing remain required at larger blast radii.
- Native Windows release readiness requires a real Windows build/test canary;
  a cross-check from another OS alone is insufficient.

## Architecture impact

`docs/architecture.md` now records the cross-platform shell plane, protected
operation families, native sandbox differences, and WSL2 requirement for
Claude work that needs OS-enforced containment.

## Sources

- Claude Code Windows setup and WSL2 guidance:
  <https://code.claude.com/docs/en/setup>
- Claude Code sandbox platform support:
  <https://code.claude.com/docs/en/sandboxing>
- Claude Code PowerShell tool and native-Windows limitation:
  <https://code.claude.com/docs/en/tools-reference>
- Codex native Windows and WSL sandboxing:
  <https://learn.chatgpt.com/docs/windows/windows-sandbox>,
  <https://learn.chatgpt.com/docs/windows/wsl>
