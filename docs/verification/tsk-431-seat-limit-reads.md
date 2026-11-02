# Seat account profiles and limit reads: qualification record

Status: partial. It records what was read from each CLI's own schema and
documentation, and the steps that still have to run in native seats for
TSK-431 AC-3 and AC-4. No token, sign-in file or credential helper was read,
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

## AC-3: Codex app-server read in a native seat (to run)

The schema export above shows the shape. Still to do: in an interactive
Codex seat on the supported release, read the live result of
`account/rateLimits/read` and the next `account/rateLimits/updated` event,
store each with `codeflow models record-limits --family codex --config-home
<the seat's CODEX_HOME>` (the host's own environment is not the seat's, so
the producing home is passed explicitly), and record
the release (`codex --version`), the method each value came from and the
`codeflow models limits` output here. CodeFlow does not speak the app-server
protocol itself; the seat's host supplies the payload.

## AC-4: journey with the build under test (to run)

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

Not exercised so far: any of the steps above in a seat, a second account that
is signed in (no credential is copied or created), and a live Claude
statusline payload.
