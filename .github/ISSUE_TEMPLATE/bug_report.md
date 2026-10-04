---
name: Bug report
about: Something in codeflow isn't working as documented
title: ''
labels: bug
assignees: ''
---

**What happened**
A clear description of the bug.

**Expected**
What you expected to happen.

**Severity**
`critical` or `normal`. Critical means the bug is in a published release or
blocks current work, and it does one of these (say which):
- blocks adopters: a managed file, gate, hook or guard fails or refuses
  ordinary work
- weakens a security boundary (do not file it here; see `docs/SECURITY.md`)
- loses or rewrites data, records or history
- deadlocks or hangs a gate, or blocks CodeFlow's own fix path

**Reproduction**
Steps to reproduce — ideally the exact `codeflow` command(s) and, where a gate
fired, the guard/hook message it printed.

**Environment**
- `codeflow --version`:
- OS / arch:
- Install method (shell installer / `cargo install`):

**Additional context**
Logs, `codeflow doctor` output, or anything else relevant.
