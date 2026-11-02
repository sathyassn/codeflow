# Seat account profiles and limit reads: qualification record

Status: Codex read qualified and the AC-4 journey run on 2026-11-02 (see the
two sections below). No token, sign-in file or credential helper was read,
copied or queried.

## What each family exposes

| Family | Release read | Method | What was read | Result |
|---|---|---|---|---|
| Codex | `codex-cli 0.166.0` | `codex app-server generate-json-schema --out <dir>`, an offline schema export of the installed release | Request `account/rateLimits/read` (response `GetAccountRateLimitsResponse`) and notification `account/rateLimits/updated`. Both carry `rateLimits`, a snapshot whose `primary` and `secondary` windows have `usedPercent` (integer, required), `windowDurationMins` (integer or null) and `resetsAt` (integer or null). The response also has `rateLimitsByLimitId` | Field names and types match the reader. The reader takes the default bucket, `rateLimits` |
| Claude Code | documented, `2.1.314` installed | The statusline page of the Claude Code documentation | `rate_limits.five_hour` and `seven_day`, each with `used_percentage` (0 to 100) and `resets_at` (Unix seconds). The block appears only for claude.ai Pro, Max and Ultra plans (or behind a gateway with a spend limit), only after the first response, and each window can be absent | Not exercised live: no statusline script piped a real payload |
| Grok Build | `grok 1.0.70` | None | No machine-readable usage signal was found | `codeflow models record-limits --family grok --limit-hit` stores the error's time; the wording of Grok's limit error was not captured |

Window mapping used by the reader: a Codex window of up to 360 minutes is
the 5-hour window and one of 8,640 minutes or more is the weekly window; a
window with no duration is placed by position, `primary` first. A window
that is absent shows `not exposed`.

The Codex method names come from the installed release, not from vendor
documentation. A different release can rename them. The field names were
schema checked on `0.166.0`; the reader refuses a payload it cannot read
with a message and never guesses.

## AC-3: Codex app-server read on the supported release

Read on 2026-11-02 with `codex-cli 0.166.0` by a short script that starts
`codex app-server`, sends `initialize`, `initialized` and then
`account/rateLimits/read` (script and raw reply:
`/tmp/cf-tsk431-ac4/read_limits.py`, `/tmp/cf-tsk431-ac4/ratelimits-read.json`,
kept on the operator's host). The reply carries `rateLimits.primary` with
`usedPercent` 71, `windowDurationMins` 10080 and `resetsAt` 1794181768, and a
null `secondary`. `codeflow models record-limits --family codex` accepted it
and `codeflow models limits` showed `weekly: 71% used, resets
2026-11-08T23:49:28Z` and `5-hour: not exposed`. The method name is from the
release's own schema, not vendor documentation.

Limits of this read: the process was a stand-alone app-server on the
operator's host, not a process inside the seat, so it shows the method and
the payload shape on this release, not the seat's own channel. The
`account/rateLimits/updated` event was not received, so only the read is
qualified. The reply holds an account identifier, so the raw file is not
copied into the repository.

## AC-4: journey with the build under test

The installed 3.1.0 has no `codeflow models launch-env`, so the journey runs
with the build under test: put that build's `target/debug` folder first on
`PATH`, and check `which codeflow` and `codeflow --version` name it before
starting. In a project whose `.codeflow/local/accounts.toml` maps one family to
a second config home:

1. `codeflow models launch-env --family <family>` prints one line,
   `export <VARIABLE>='<second home>'`; for a family the profile does not map
   it prints `unset <VARIABLE>`.
2. A Herdr seat of that family starts only after the published sequence
   passes in its pane (`cf-herdr`, "Create or resume"): the pane runs that
   line, then `launch-env --family <family> --check`, and `herdr pane
   wait-output` sees the acknowledgement made for this launch (a fresh
   random value, printed only when the check passes). With `CODEX_HOME`,
   `CLAUDE_CONFIG_DIR` or `GROK_HOME` already set in the caller's
   environment to some other folder, the seat's own variable still holds the
   mapped folder (or is unset when unmapped). The other two families'
   variables are inherited unchanged and are not part of this check.
3. `codeflow models record-limits --family <family> --config-home <second
   home>` stores a payload from the seat (pass the producing seat's home
   whenever the host's environment differs from the seat's), and `codeflow models limits` shows the account line
   `config home <second home> (via <VARIABLE>)` and the family's windows.
4. With `token = "x"` added to the profile, `launch-env` exits 2 and prints
   nothing to `eval`; the sequence stops at its first `|| exit 1`, so no
   `herdr agent start` runs and no seat starts.

Result (2026-11-02, evidence in `/tmp/cf-tsk431-ac4/`, not copied because the
raw files name accounts and host paths):

- Step 1, `step1-output.txt`, run by the operator with the build under test
  (`codeflow 3.2.0 source=4dfcef551 dirty=false`): `export
  CODEX_HOME='~/.codex-second'`, then `CODEX_HOME is the mapped
  config home`, `rc=0`.
- Step 2, `step5-shell-before-start.txt`: in the Herdr pane the mapped line and
  `--check` passed (`check_rc=0`) before `herdr agent start`. The seat
  pane then showed `OpenAI Codex (v0.166.0)` running in this worktree
  (`step5-pane-read.txt`). The evidence does not record the agent name or a
  ready flag; the claim rests on the pane read.
- Step 3, `step5-output.txt`: after `record-limits` stored the reading,
  `codeflow models limits` printed `account: config home
  ~/.codex-second (via CODEX_HOME)` and `weekly: 71% used, resets
  2026-11-08T23:49:28Z`.
- Step 4 (refusal when the profile names a token) is covered by the
  `models_cli` launch tests, not by a seat run.

The 71% in step 3 is the stand-alone app-server read of the second config
home (`initialize.json` names `~/.codex-second`), taken at 21:26Z and stored
about an hour later. The seat pane read at 17:35 local showed 16% of the
weekly limit left, so the limits view shows a stored reading, not the seat's
current state. The AC-3 read used the same second home.

Not exercised: the seat's own environment could not be read with `ps`, so
`CODEX_HOME` inside the seat process is shown by the pane that launched it and
not by a process listing; the limits view shows the stored reading, not a live
one; the update notification; a live Claude statusline payload.
