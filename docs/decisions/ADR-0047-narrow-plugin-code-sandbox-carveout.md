---
id: ADR-0047
uid: f4e4c2db-b446-445d-9fcf-af4814471d94
title: narrow plugin-code sandbox carveout
date: 2026-08-01
status: accepted
superseded_by: null
architecture_impact: none
---

# ADR-0047 — narrow plugin-code sandbox carveout

## Context

Claude's subprocess sandbox denies reads across `~/.claude` so repository work
cannot inspect credentials, sessions, histories, memory, or other private host
state. Installed commands execute code from Claude's plugin cache, so denying
that cache also prevents the official Codex companion from starting. The wider
plugin directory is not an acceptable exception because it also contains
mutable, cross-project job data and transcripts.

## Decision

Keep the broad `~/.claude` read denial and re-allow only
`~/.claude/plugins/cache` for sandboxed subprocesses. Mutable plugin data and
all other Claude state remain outside the exception. Permission-layer secret
denies and the classified trusted-tool retry remain independent controls.

## Consequences

Installed plugin code can start without granting arbitrary subprocesses access
to plugin job logs or other private Claude state. A plugin operation that needs
authenticated tool state must use the existing classified trusted-tool path;
the exception must not be widened for convenience. If Claude changes the
installed-code location, the binding must be requalified and this decision
superseded rather than silently broadening the path.

## Architecture impact

None. This narrows a harness sandbox binding without changing CodeFlow's
repository architecture.
