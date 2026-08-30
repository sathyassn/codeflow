# Herdr-primary consult canary — 2026-08-30

Live dual-lineage consult from a Grok host inside Herdr (`HERDR_ENV=1`,
workspace `w2`). Doctrine pins remain in `cf-herdr` and
`herdr_host_contract.rs`. This note records what the run actually exercised.

## Exercised

| Step | Observed |
|---|---|
| Discover without hijack | `herdr workspace/tab/pane/agent list`; caller `w2:p1` untouched; w4/w5 idle Codex seats untouched |
| Named tab create `--no-focus` | `cf/codeflow/skill-cat/claude/01` (`w2:t8`/`w2:p8`); `cf/codeflow/skill-cat/codex/01` (`w2:t9`/`w2:p9`) |
| Agent start | `cf-codeflow-skill-cat-cl01` (`claude --model fable --effort high`); `cf-codeflow-skill-cat-cx01` (`codex --model gpt-5.6-sol`, observed high) |
| Prompt and wait | `herdr agent prompt` then `herdr agent wait` until idle/done; native ids `9be3ff37-1c16-48de-8d80-967023abd3bd` (Claude) and `01a0536b-1b18-76a0-9018-ea728126f541` (Codex) |
| Same-tab follow-up | Second and third turns on the same names after harvest; cwd remained the craft worktree |
| Anti-hijack | Did not send keys to `$HERDR_PANE_ID`; did not close foreign `cf-` agents |

## Not exercised

- Schema-v2 `delegate arm` + `herdr pane send-text` of the armed file +
  wait-accepted/terminal (Grok-hosted consults used `herdr agent prompt`)
- Multi-line armed-prompt digest identity vs send-text
- `codeflow doctor --check delegate-roundtrip` on the Herdr lane
- Closing self-created tabs (left open for operator inspection)

Those gaps stay on the degraded-tmux lifecycle canary and a later Herdr
lifecycle round-trip. Marker tests are not a substitute.
