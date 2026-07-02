---
name: cf-delegate
description: When and how to consult or delegate to another vendor's coding CLI (OpenAI codex primary, Antigravity agy degraded) under its own subscription auth — read-only second opinions versus full task handoffs, the canonical invocations, the edit-access doctrine, and the guardrails. Use when you want an independent second opinion, a specialty pass, or genuinely parallel work handed to a non-Claude harness.
---

# cf-delegate — cross-vendor consult and delegate

One line: **compose harnesses at the process boundary, each under its own
subscription auth; CodeFlow's gates judge the output, never the author.** A
second harness is a colleague you can call, not a dependency you take on — you
still own the result, verify it, and answer for it.

## Consult, delegate, or neither

Match the tool to the work; most work is *neither*.

- **Neither (the default).** You have the context and the skill — do it
  yourself. A second harness costs a round-trip, quota, and a synthesis step;
  spend that only when it buys something you cannot get alone.
- **Consult — read-only second opinion.** A critique, a design review, a
  correctness pass, an "am I missing something" on code *you* wrote. The
  delegate reads and reasons; it never edits. Value is the *independent*
  vantage: a different model, trained differently, catching what you pattern
  past. Use it exactly where self-review is weakest.
- **Delegate — full task handoff with edit access.** A whole unit of work
  handed off, code included. Reserve for genuinely **parallel** work (you are
  busy on the critical path and this is independent) or **specialty** work
  another harness does better. Not for work you could just do — a handoff you
  have to review line-by-line is slower than doing it yourself.

When unsure, consult before you delegate: a read-only opinion is cheap and
reversible; an edit handoff is neither.

## Preflight — is a delegate even available

Delegation is **optional**. Never assume it is wired; check, and degrade
legibly when it is not (`codeflow doctor` reports this as the `delegates`
check):

```sh
codex login status   # exit 0 + "Logged in using ChatGPT" → ready
```

If `codex` is missing, do the work yourself and say so. If it is present but
unauthenticated (or 401s mid-run), stop and tell the user to run `codex login`
— **never automate the auth**. One vendor account, the user's own.

## Canonical codex invocation (primary tier)

OpenAI's `codex` CLI, headless, under the user's ChatGPT subscription. The
verified shape:

```
codex exec --json [--cd DIR] [--skip-git-repo-check] \
  --sandbox read-only|workspace-write [--output-schema FILE] "TASK"
```

`codex exec` takes **no approval flag** — `-a` / `--ask-for-approval` is a
top-level `codex` option, and `codex exec -a never` is rejected outright
(`error: unexpected argument '-a' found`). In headless `exec`, the `--sandbox`
mode is the whole access control: `read-only` for a consult, `workspace-write`
for a delegate that edits. Both verified live on 2026-07-02 (codex-cli 0.142.5);
`--sandbox workspace-write` confirmed writing a file to disk with no approval
prompt.

Stdout is JSONL: `thread.started` (carries `thread_id`), `turn.started`,
`item.completed` (the reply is `.item.text` where `.item.type=="agent_message"`),
`turn.completed` (carries `usage`). Warnings go to stderr — parse stdout only.

**Consult (read-only), capturing the thread id for follow-up:**

```sh
tid=$(codex exec --json --cd "$PWD" --skip-git-repo-check --sandbox read-only \
  "Review src/foo.rs for correctness. Cite line numbers. End with VERDICT: approved|changes_requested." \
  | tee consult.jsonl \
  | jq -r 'select(.type=="thread.started").thread_id')
jq -r 'select(.item.type=="agent_message").item.text' consult.jsonl
```

**Follow-up on the same thread (multi-turn, context retained):**

```sh
codex exec resume "$tid" --json "Which single finding matters most, and why?" \
  | jq -r 'select(.item.type=="agent_message").item.text'
```

### Structured verdicts — `--output-schema`

For a machine-checkable verdict, pass a JSON Schema file; the agent message
comes back as JSON matching it. Branch on the enum, never on prose. Use the
same shape the pipeline gates use, so consult output composes with review:

```json
{
  "type": "object",
  "required": ["verdict", "findings"],
  "properties": {
    "verdict":  { "type": "string", "enum": ["approved", "changes_requested"] },
    "findings": { "type": "array", "items": { "type": "string" } }
  }
}
```

```sh
codex exec --json --cd "$PWD" --skip-git-repo-check --sandbox read-only \
  --output-schema verdict.schema.json "Review the built diff against: <criteria>" \
  | jq -r 'select(.item.type=="agent_message").item.text' | jq .
```

## Edit-access doctrine (delegate tier)

A delegate that edits gets `--sandbox workspace-write` **only** inside a
worktree on a feature branch — the same worktree-per-session discipline that
binds Claude here. Point it there with `--cd <worktree>` and let it commit
conventionally:

```sh
codex exec --json --cd /path/to/worktree --sandbox workspace-write \
  "Implement <task>. Commit conventionally (type(scope): description). \
   Scope: only <paths>. Cap: <N> steps. Surface ambiguity as a blocker — never guess." \
  | jq -r 'select(.item.type=="agent_message").item.text'
```

Never grant workspace-write on the root checkout or a protected branch. The
point of the doctrine: a delegate's commits pass through **CodeFlow's existing
gates unchanged** — pre-commit secret scan, commit-msg format and
no-AI-attribution, the test gate, and an independent `cf-reviewer` pass judge
its work exactly as they judge Claude's. Enforcement is author-agnostic, so a
delegate cannot lower the bar. The git-hook plane is harness-agnostic by design:
the protected-branch merge and ref guards (`pre-merge-commit`,
`reference-transaction`) bind a delegate exactly as they bind any agent, so it
cannot ff-merge, `reset --hard`, or delete a protected branch. Review the
handoff before it ships; a delegate's output is a proposal, not a merge — a
human lands it.

## Interactive tier — the official plugin

For human-in-the-loop use (not automation), OpenAI ships and maintains a Claude
Code plugin, `openai/codex-plugin-cc`, under the same subscription auth.
Document it; **do not bundle it**:

- Install per the plugin's README; the user runs `codex login` once.
- Commands: `/codex:review`, and `/codex:rescue` with `--write` / `--resume` /
  `--background`.

## Degraded tier — Antigravity `agy` (opt-in, read-only)

`agy` (Google Antigravity CLI, Google OAuth) works but is **degraded**; use it
only when the user names it, and only for read-only consult. Known hazards:

- **Stdout drops the final response under non-TTY** — never parse `agy` stdout
  in automation. Read the transcript file the run writes instead:
  `~/.gemini/antigravity-cli/brain/<conv-id>/.system_generated/logs/transcript.jsonl`.
- **No `--output-format json`** — no structured-verdict path; treat replies as
  prose you must synthesize, not parse.
- `--conversation <id>` pins a multi-turn thread.
- Permission modes: `request-review` | `proceed-in-sandbox` | `always-proceed`
  | `strict`. Stay at the read-only end for consult.
- **Multi-agent fan-out burns plan quota fast** — no fan-out; one scoped,
  step-capped prompt per consult.

Because output handling is fragile, prefer `codex` for anything structured and
reserve `agy` for a genuinely-wanted second model's read-only take.

**Experimental — binding the CodeFlow guards to `agy`.** Unlike Codex, `agy` is
not shipped as a bound harness (ADR-0008): its hook dialect differs (an
`allow_tool` JSON contract, and hooks that always exit 0), so the exit-2 block
that stops a bad command under Claude/Codex is at best *advisory* here — do not
rely on it as a guard. If you still want the CodeFlow guards to run for feedback,
opt in by hand: create `~/.gemini/config/hooks.json` with
`{"hooks":{"PreToolUse":[{"matcher":"^Bash$","hooks":[{"type":"command","command":"codeflow hook git-guard"},{"type":"command","command":"codeflow hook exec-guard"}]}]}}`.
Treat this as experimental and unverified on macOS; the authoritative protection
for `agy` output stays the same as for any delegate — CodeFlow's git-hook plane,
CI, and an independent review of the handoff before it lands.

## Guardrails

- **Never automate vendor auth.** The user runs `codex login` / `agy` auth by
  hand, on a single account. Degrade legibly on missing or 401 — never paper
  over it.
- **Every delegate prompt carries caps.** Explicit file scope, a step budget,
  and "surface ambiguity as a blocker, never guess." Unbounded handoffs are how
  quota and scope both blow out.
- **Synthesize, never paste.** A delegate's finding is an input, not your
  conclusion. Re-derive it, cite the file:line or command output yourself, and
  say plainly where you agree and disagree before reporting it as fact. An
  unverified delegate claim is an unverifiable claim — a defect by this repo's
  own rule.
- **Stay within ToS.** This process-boundary composition is sanctioned (OpenAI
  ships the plugin itself); the single-account, manual-auth, degrade-on-401
  posture is what keeps it there.
