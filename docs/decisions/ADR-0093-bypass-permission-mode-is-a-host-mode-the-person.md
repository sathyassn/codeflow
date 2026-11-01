---
id: ADR-0093
uid: 7e2eb263-2728-4fa5-a6ca-81d1eba1d024
title: "Bypass permission mode is a host mode the person sets outside the repository"
date: 2026-10-31
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: none # none | one line naming what in architecture.md changes
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0093: Bypass permission mode is a host mode the person sets outside the repository

## Context

Operators running a coordinator and several lane sessions asked for Claude
Code's `bypassPermissions` mode as a sanctioned choice beside auto mode,
because the auto classifier refuses routine task-scoped commands and each
refusal becomes a manual chore (issue 451). ADR-0066 kept
`defaultMode: "bypassPermissions"` in `bypass-sandboxed.json` as "the
documented intent of that preset". Claude Code no longer honours that value:
since v2.1.257 a `bypassPermissions` or `auto` in `.claude/settings.json` or
`.claude/settings.local.json` does not take effect and the session starts in
Manual mode, while every other mode value applies from project scope and
outranks `~/.claude/settings.json` (permission-modes and settings reference,
read 2026-10-31). The shipped key therefore did nothing, and a project value
can keep a person's own user-level bypass from applying. Deny rules hold in
every mode, the guard hooks set `onFailure: "block"`, so a timeout, crash or
unreadable output blocks the call (hooks reference, Claude Code 2.1.295 or
later), and the sandbox is independent of the mode.

## Decision

Auto and bypass are both sanctioned host modes, chosen by the person outside
the repository: in `~/.claude/settings.json`, with `--permission-mode`, or
through managed settings. No preset ships a `defaultMode` that Claude Code
ignores at project scope: `bypass-sandboxed.json` drops
`defaultMode: "bypassPermissions"` and so carries the same project settings
as `default.json`, and `codeflow init` prints the two routes that work when
the person picks that preset. The `acceptEdits` preset keeps its mode, which
applies from project scope and is documented as outranking the user file.
`codeflow update` removes a project `defaultMode` of `bypassPermissions` or
`auto` only when the recorded baseline shows CodeFlow shipped that value,
and keeps and reports one the project set itself.

The operator decided on 2026-10-31 (option A, following section 8 of the
sandbox exemptions research) that CodeFlow keeps the binding settings in
its own session file, `~/.codeflow/claude/session.json`, loaded with
`claude --settings`, rather than in the person's user file. Under
`--settings` Claude Code ignores repository settings that loosen the
sandbox and honours `strictAllowlist`, so the file carries the sandbox
locks, the host list with `strictAllowlist`, the exclusions, the file
denies, the hidden token variables and the shipped deny and ask rules, and
bypass mode only on the person's typed yes. `codeflow claude-session
--install`, which `codeflow init` offers after the bypass answer and
`cf-customize` hands the person to run, writes it and a `claude` shell
function that adds `--settings` only when the git top level holds
`.codeflow/project.toml`, and sets `CODEFLOW_SESSION_SETTINGS=1`. The
marker is repository-controlled, which is acceptable because it can only
add strictness. The line that sources the function is added to the shell
startup file only on a second yes and recorded so `--uninstall` removes
exactly it. Bypass is not offered when managed or user settings set
`permissions.disableBypassPermissionsMode` to `"disable"` or cannot be
parsed. The person's `~/.claude/settings.json` gets only optional
per-item additions (sandbox on, credential-file denies, hidden token
variables, the Herdr exclusion when Herdr is installed), each on its own
yes, with a backup and a record in `~/.codeflow/claude-user-settings.json`.
Claude Code keeps only the last of several `--settings` flags, so the
function refuses a person's own `--settings` rather than lose CodeFlow's.
For the VS Code extension the install writes a process wrapper with the
same rule, refusals included, named by the extension's
`claudeCode.claudeProcessWrapper` setting, which the person adds. The
session file also sets `failIfUnavailable`, so a missing sandbox stops
the session; it is CodeFlow's file only, so no other project or platform
inherits it. The shipped presets deny the session file, the function,
the wrapper and the install records to the Edit and Write tools and to
sandboxed writes, as shell startup files, and the install records each
change before making it and puts back what was there before
(TSK-1070 security review, 2026-10-31). A start neither can catch (the desktop
app, `command claude`) is warned by the session-start hook, shown to the
person and to the model; it is not refused, since a refusal would leave
the session unable to act. `cf-customize`
proves the mode with a live canary: which file or flag set the mode,
`git-guard` and `exec-guard` each block a planted refused command, a deny
rule refuses a planted read and the `gh` token path, and the sandbox
refuses a write outside the project without rerunning it outside the
sandbox. This amends ADR-0066's mode line; the rest of ADR-0066 and
ADR-0019 (enforcement is the floor at every tier) stand.

Because bypass drops every prompt, the sandbox carries more of the
boundary, so the same change tightens it in every preset (operator
decision 2026-10-31):

- `sandbox.allowUnsandboxedCommands` is `false`, so Claude Code ignores a
  request to rerun a refused command outside the sandbox. This replaces
  ADR-0029's classified retry. The commands that need host state run
  outside the sandbox by name in `sandbox.excludedCommands`: the `gh`
  subcommands CodeFlow uses (`pr`, `run`, `issue`, `api`, `repo view`,
  `release view`, `release list`, `search`, `auth status`) and `git push *`.
  An entry matches only when every command in the call matches, so the
  agent push path is a plain `git push` from the session's working
  directory, and `gh` output is filtered with `--jq`. Herdr and the
  official Codex plugin are left to the person's own user or `--settings`
  file, since a project entry reaches every adopter and the plugin's
  command path changes with each version (sandbox exemptions research,
  2026-10-31; issue 452).
- A new `forge_exec` deny family refuses the excluded forms that would run
  code outside the sandbox, print the token or reach another host: `gh`
  extensions, aliases, `config set`, `auth status --show-token`,
  codespaces, keys, secrets, variables, `repo clone`, `browse`, and a push
  with `--receive-pack`, `--exec` or an `ext::` remote. The sandbox denies
  writes to `**/.codeflow/git-hooks`, which a push runs outside it, and
  removes `GH_TOKEN`, `GITHUB_TOKEN`, `NPM_TOKEN` and `NODE_AUTH_TOKEN`
  from sandboxed commands.
- A new `forge_write` family asks for every `gh api` write (a POST, PUT or
  PATCH method, `-f`, `-F`, `--field`, `--raw-field`, `--input`, and any
  GraphQL call, since a query and a mutation share one form). This departs
  from ADR-0075 decision 1 (no shipped ask rules) for this family only: an
  ask rule still prompts in bypass, which is the only way a person sees an
  outward write the deny rules cannot name, and an unattended session is
  refused. Codex gets no rule for it, since its `never` approval has no
  prompt. Text rules cannot close the list of spellings (`-X PoSt`,
  `-X=DeLeTe`, `-iXDELETE` passed them), so exec-guard reads each `gh api`
  call by gh's own option grammar and answers Claude Code with the same
  `ask` decision for a write, GraphQL, an absolute URL or `--hostname`; a
  hook `ask` prompts in bypass too (live check, Claude Code 2.1.316). It
  refuses a DELETE in any form and a form it cannot read for certain
  (TSK-1070 security review round 2). The shipped ask and deny rules stay
  as a second layer for the common spellings.
- The Read tool and the sandbox deny the token stores of `gh`, git,
  Docker, gcloud, Azure, npm and the macOS keychains.
- The `secret` and `credentials` name rules match exact secret names and
  data-file extensions only, so a source file such as `secret_scan.rs` is
  readable in the sandbox, where `!` exceptions cannot apply.
- `sandbox.network.allowedDomains` lists the development hosts the
  shipped stacks use instead of `*`, which ADR-0025 set. The private-range
  denies and local binding stay; the denies match host names, not IP
  literals, and local binding also reaches other local listeners.
- `codeflow update` moves a sandbox value still equal to the one CodeFlow
  last shipped, removes a retired shipped array entry such as `*`, and
  keeps the project's own values and entries.

## Consequences

- A preset now does what its name says, and a bypass default in the user
  file is no longer cancelled by a shipped project value. An existing
  adopter on the old bypass preset loses the inert key on the next update.
- Bypass removes the classifier, the allow-rule layer, protected-path
  prompts and the prompts for excluded commands and unlisted hosts. What
  remains is the guard hooks, deny and ask rules, the fail-closed sandbox
  with no rerun outside it, git hooks, CI and remote protection; the docs
  state both lists.
- In bypass, Claude Code lets a sandboxed command reach an unlisted host
  without a prompt unless `sandbox.network.strictAllowlist` is on, and a
  project file cannot set it. Under it Claude Code ignores the project's
  list, so CodeFlow's session file sets it together with the hosts, and a
  session started without that file is warned at start.
- Breaking for adopters: a tool that needs host state and is not in the
  shipped list, such as `docker`, `herdr` or the Codex plugin, a `gh`
  subcommand outside the list, a `git push` inside a compound call or a
  script, and a host that is not listed now fail in the sandbox. A `gh api`
  write now prompts in every mode.
- The documented App token form `GH_TOKEN=$(...) gh pr create` never
  matches an exclusion because of the `$(...)`, so it stays sandboxed and
  fails; a helper that mints the token and runs `gh` itself is the
  follow-up issue 452 asks for. The person adds them in
  `.claude/settings.local.json` (`sandbox.excludedCommands`,
  `sandbox.network.allowedDomains`). Removing a merged worktree, which the
  sandbox's `.git` and `.claude` write denies stop, goes to the operator.
- Excluded commands do not inherit the host list. Exec-guard's prompt
  covers `gh api --hostname` and an absolute URL endpoint; `gh` with `GH_HOST` or a `-R` naming another host,
  and a push to a remote URL on another host, have no rule of their own.
- Whether PreToolUse hooks run under bypass is not stated in Claude Code's
  docs, so the canary proves it in each live session rather than this record
  asserting it.
- Claude Code's own guidance remains that bypass belongs in an isolated
  environment; the choice and its tradeoff are the person's.

## Architecture impact

None.
