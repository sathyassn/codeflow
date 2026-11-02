# Seat account profiles and limit reads: qualification record

Status: evidence for TSK-431 AC-3 and AC-4, taken on 2026-10-30. It says
what was read from each CLI, by which method, and what was not exercised.
No token, sign-in file or credential helper was read, copied or queried.

## What each family exposes

| Family | Release read | Method | What was read | Result |
|---|---|---|---|---|
| Codex | `codex-cli 0.166.0` | `codex app-server generate-json-schema`, an offline schema export of the installed release | Request `account/rateLimits/read` and event `account/rateLimits/updated` exist. Both carry `rateLimits` with `primary` and `secondary` windows of `usedPercent` (integer), `windowDurationMins` (integer or null) and `resetsAt` (Unix seconds or null). The update is "sparse": a client merges it into the last read | Field names and types are the release's own |
| Codex | `codex-cli 0.166.0` | `/status` typed into an interactive Codex seat in a Herdr tab, signed in on the default home | A `Weekly limit` line: `76% left (resets 9:32 AM on 6 Nov)`. No 5-hour line was shown for this plan | One live value, read as `usedPercent` 24 with `resetsAt` 2026-11-06T14:32:00Z. The seat shows the app-server's data; CodeFlow did not speak JSON-RPC to the server |
| Claude Code | documented, `2.1.314` installed | The statusline page of the Claude Code documentation | `rate_limits.five_hour` and `seven_day`, each with `used_percentage` (0 to 100) and `resets_at` (Unix seconds). The block appears only for claude.ai Pro, Max and Ultra plans (or behind a gateway with a spend limit), only after the first response, and each window can be absent | Not exercised live: no statusline script piped a real payload |
| Grok Build | `grok 1.0.70` | None | No machine-readable usage signal was found | `codeflow models record-limits --family grok --limit-hit` stores the error's time; the wording of Grok's limit error was not captured |

Window mapping used by the reader: a Codex window of up to 360 minutes is
the 5-hour window and one of 8,640 minutes or more is the weekly window; a
window with no duration is placed by position, `primary` first. The
live Codex read gave only a weekly window, so a missing window shows
`not exposed`.

The Codex method names come from the installed release, not from vendor
documentation. A different release can rename them, so the reader is
qualified on `0.166.0` and a payload it cannot read is refused with a
message, never guessed.

## Journey (AC-4)

Run on 2026-10-30 with this branch's built binary standing in for the
installed build (the installed 3.1.0 predates the command), in a disposable
project under the session scratch folder.

1. Wrote `.codeflow/local/accounts.toml` mapping `codex` to a second config
   home folder (empty, never signed in) and nothing else.
2. `codeflow models launch-env --family codex` printed one line,
   `export CODEX_HOME='<second home>'`.
3. In a new Herdr tab, ran that line in the pane, then
   `herdr agent start ... --kind codex -- --ask-for-approval never`. The
   seat started on Codex's first-run sign-in screen, which a home with no
   login shows.
4. Observed: the process environment held `CODEX_HOME=<second home>` and
   neither `CLAUDE_CONFIG_DIR` nor `GROK_HOME`; Codex filled the second home
   with its own state files (`config.toml`, `installation_id`, `logs_2.sqlite`).
5. Piped the weekly reading from the `/status` run above to
   `codeflow models record-limits --family codex`. `codeflow models limits`
   then showed: `account: config home <second home> (via CODEX_HOME)`,
   `5-hour: not exposed`, `weekly: 24% used, resets 2026-11-06T14:32:00Z`.

Not exercised: a second account that is signed in (no credential was copied
or created), a reading taken from that second account, a Claude or Grok seat
under a second home, and a live Claude statusline payload. The weekly figure
in step 5 came from the default signed-in home, so it shows the reader's
output, not the second account's usage.
