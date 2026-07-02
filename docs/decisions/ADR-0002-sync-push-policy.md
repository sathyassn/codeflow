---
id: ADR-0002
title: sync push policy — push_to_protected warn on this repo
date: 2026-06-12
status: superseded
superseded_by: ADR-0006
architecture_impact: none
---

# ADR-0002 — sync push policy: push_to_protected warn on this repo

## Context

This repository is private on GitHub Free, where remote branch protection is
unavailable (`codeflow remote protect` verified the API returns 403). Work
lands on `main` locally through `codeflow integrate` — but with
`git.push_to_protected = "block"`, the locally-integrated `main` could never
sync to `origin`: there is no PR-merge path that the remote enforces, and the
pre-push hook blocks the direct push.

## Decision

Set `git.push_to_protected = "warn"` in this repo's `.codeflow/policy.json`,
until the repo is public or remote branch protection otherwise exists. Pushing
`main` warns instead of blocking, so the integrate-then-sync flow works. All
other protected-branch rules (commit, force-push, delete, hard-reset) stay at
`block`. The perimeter remains CI plus the integrate gate: nothing reaches
`main` locally except through `codeflow integrate`, and CI re-runs every gate
on push.

## Consequences

- The locally-integrated `main` can sync to `origin` without a bypass,
  keeping discipline intact (no `--no-verify`, no exported gate tokens).
- A direct push of un-integrated work to `main` is no longer hard-blocked at
  pre-push — it warns. The integrate gate and CI still catch it; the residual
  risk is accepted while the remote cannot enforce protection.
- Revisit when the repo goes public or moves to a plan with branch
  protection: flip the value back to `block` and run `codeflow remote
  protect`.

## Architecture impact

None — this is a policy value in config, not code.
