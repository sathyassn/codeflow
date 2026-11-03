# Monitor tool input shape (provisional)

Status: provisional. The shape below is observed in one installed Claude Code
build and is not confirmed on the other supported versions.

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
- The exact name of the socket-URL field. The guard does not read it.
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
  (`test_payload_parse_monitor_command_and_url_forms`)
