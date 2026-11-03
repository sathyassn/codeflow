# Monitor tool input shape (provisional)

Status: provisional. The shape below is observed in one installed Claude Code
build and is not confirmed on the other supported versions.

## Version scope

- Observed: Claude Code 2.1.317 only. This record claims the matcher and the
  `tool_input.command` read for that build and for any build whose `Monitor`
  hook payload has the same `tool_name` and `tool_input.command` shape.
- Reason for the narrowing: only the installed 2.1.317 build could be
  observed here, the coordinator asked that harness binaries not be read
  again, and a non-interactive capture is refused by policy. No other build
  was available to check.
- What is lost: no confirmation that the hook payload shape holds on other
  supported builds, and no native hook payload for `Monitor` on any build.
- An unverified build does not fail closed. If its `Monitor` payload carries
  the shell text outside `tool_input.command`, the guards find no command and
  allow the call, the same as before this change. This change adds judging
  where the field matches; it does not block unknown shapes. No evidence shows
  such a build exists.
- A matcher entry for a tool name a build does not have matches nothing, so
  the wider matcher is harmless on builds without `Monitor`. A build that
  puts the shell text in a different field is not covered, and the guards
  would pass it, because a call without `tool_input.command` has nothing to
  judge.
- Native hook capture: attempted and not obtained. A non-interactive capture
  run of the Claude CLI with a logging hook in a scratch folder was refused by
  the `security.headless_peer_runs` guard rule, and was not retried or
  bypassed. An interactive capture is still owed (the task's AC-1 and AC-2).

## What was found

- Claude Code 2.1.317 (macOS, installed binary), read on 2026-11-03 by
  listing the strings embedded in the binary, once, before the coordinator
  asked that harness binaries not be read.
- The `Monitor` tool describes its `command` field as "Shell command or
  script. Each stdout line is an event; exit ends the watch." Its other
  fields are `description`, `timeout_ms` and `persistent`.
- The same build allows a socket watch in place of a command: the tool takes
  exactly one of `command` or a socket URL. A socket watch has no
  `tool_input.command`.
- No source in this repository or its docs describes the shape. The only
  repository mention of the tool is a tool-trace line in
  `docs/verification/evidence/tsk-113/ac2-native-run-2/tool-trace.jsonl`, which
  shows the tool name and no input.

## What was not confirmed

- The shape on any other Claude Code version, or in Anthropic's published docs.
- The socket field is not confirmed from a hook payload. The tool schema the
  Claude Code 2.1.317 session itself reported to the model (not a binary read)
  gives it as `ws` with a `url` inside (`{"ws": {"url": ...}}`), and says it
  cannot be combined with `command`. The guard does not read it, and no test
  fixture claims it: the tests cover only a call with no `command`.
- That the hook payload for `Monitor` is `tool_name: "Monitor"` with the input
  under `tool_input`, as for `Bash`. This is assumed from the common hook
  contract; no native hook payload for `Monitor` was captured.

## What the guard does

- Before this change the shell hook matcher was `^(Bash|PowerShell)$`, so a
  `Monitor` call reached no guard, and `HookPayload::shell_command` returned
  nothing for it.
- Now the matcher is `^(Bash|PowerShell|Monitor)$` in the three presets and
  the installed copies, and `shell_command` returns `tool_input.command` for
  `Monitor`. A call with no `command` has nothing to judge and passes.
- Exec-guard treats `Monitor` like `PowerShell`: not shown to be a POSIX
  shell, so the line keeps the raw rules, which are no looser than `Bash`.

## Evidence

- `crates/codeflow-cli/tests/settings_presets.rs`
  (`shell_guard_matcher_covers_the_monitor_tool_everywhere`)
- `crates/codeflow-cli/tests/refusal_journey/monitor.rs`
- `crates/codeflow-core/src/hooks/git_guard.rs`
  (`test_payload_parse_monitor_command_and_missing_command_forms`)
