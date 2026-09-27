---
id: ADR-0037
uid: b5f2f8b6-af5a-4ad6-9355-7c754451b3e6
title: require a canonical delegate prompt boundary
date: 2026-07-24
status: accepted
superseded_by: null
architecture_impact: delegate arm validates non-empty canonical UTF-8 prompt text with internal LF, no terminal line break, and no editor control characters before creating turn state; the host still owns transport and exact delivery
---

# ADR-0037 — canonical delegate prompt boundary

## Context

ADR-0036 commits an armed turn to the SHA-256 of its exact prompt bytes and
requires the target harness's `UserPromptSubmit` event to match that digest.
The PR2 native Claude canary showed that the interactive input editor does not
preserve every possible byte stream: raw CRLF input arrived at the hook as two
LF characters. Treating arbitrary input bytes as transportable would therefore
promise an invariant the target harness cannot satisfy.

The follow-up blind eval also showed that a terminal LF on a single-line prompt
is consumed as editor input rather than preserved in `UserPromptSubmit`; the
same text without that terminal delimiter matched exactly. The canary found a
separate input-editor race as well. Sending Enter immediately
after tmux literal paste could leave the paste attachment unsent. A bounded
settle before the one submission key made the real session reliable; repeated
blind Enter retries would instead risk duplicate or unintended turns.

## Decision

Amend ADR-0036 at the host-to-harness input boundary:

- `delegate arm` accepts only non-empty canonical UTF-8 text with internal LF
  line endings, no terminal line break, and no other control characters. It
  validates this before creating durable turn state, then records the digest
  of those exact canonical bytes.
- The host normalizes once before arming and delivers that same file. For the
  current tmux adapter it uses literal paste, a bounded 300 ms input-settle,
  then one separate Enter.
- An acceptance timeout does not authorize blind retries. The host may inspect
  only the dedicated pane and send Enter once more only when that pane
  explicitly shows the paste attachment still waiting in the input editor.
- Canonicalization is a byte-transport constraint, not semantic prompt
  policing. The binary still does not launch a harness, choose a model, inspect
  a transcript, or determine completion from pane output.

## Consequences

- Canonical UTF-8 prompts with internal LF retain ADR-0036's digest, session,
  and prompt-ID correlation without claiming support for bytes the target TUI
  rewrites or consumes as editor control.
- Empty, CRLF, terminal-line-break, invalid UTF-8, and control-bearing prompt
  files fail before an armed turn exists and can be normalized explicitly by
  the host.
- The current adapter gains a small bounded submission delay, while lifecycle
  waiting and terminal observation remain tmux-free.
- Other future transports may use a different delivery mechanism, but they
  must preserve the same canonical bytes and lifecycle evidence.
