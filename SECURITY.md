# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security vulnerabilities.

Report privately through GitHub's built-in **private vulnerability reporting**:
the repository's **Security** tab → **Report a vulnerability**. This opens an
advisory visible only to the maintainers.

Where possible, include: a description of the issue, the affected version(s) or
commit, reproduction steps, and the impact. You will receive an acknowledgement,
and any fix or mitigation is coordinated before public disclosure.

## Supported versions

Only the latest released major line is supported. Older major lines, including
the archived `1.x` implementation at tag `v1-final`, are unmaintained.

## A note on the enforcement model

codeflow enforces repository discipline across four planes — local git hooks, an
in-session guard, CI, and remote branch protection. **By design, the local
planes are fast feedback, not a hard boundary** against a determined actor who
shares the host; the authoritative perimeter is remote branch protection + CI.
Reports of bypasses of that documented boundary are in scope and welcome; a
local guard being avoidable by a host-sharing process is a known, documented
limitation (see `docs/decisions/`), not a vulnerability.
