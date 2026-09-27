# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security vulnerabilities.

Report privately through GitHub's built-in **private vulnerability reporting**:
the repository's **Security** tab, then **Report a vulnerability**. This opens
a private advisory.

If the **Report a vulnerability** button is not shown, open a public issue
titled "Security contact request" that contains no details of the problem. A
maintainer will reply there with a private way to send the report.

Where possible, include: a description of the issue, the affected version(s) or
commit, reproduction steps, and the impact. You will receive an acknowledgement,
and any fix or mitigation is coordinated before public disclosure.

## Supported versions

Only the latest released major line is supported. Older major lines, including
`1.x`, are unmaintained.

## A note on the enforcement model

codeflow enforces repository discipline across four planes — local git hooks, an
in-session guard, CI, and remote branch protection. **By design, the local
planes are fast feedback, not a hard boundary** against a determined actor who
shares the host; the authoritative perimeter is remote branch protection + CI.
Reports of bypasses of that documented boundary are in scope and welcome; a
local guard being avoidable by a host-sharing process is a known, documented
limitation (see `docs/decisions/`), not a vulnerability.
