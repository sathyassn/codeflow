# Monitor tool input shape

Status: confirmed by a native capture on Claude Code 2.1.318 for the command
form. Not confirmed on other versions, and the socket form was not captured.

## What the native capture showed

On 2026-11-03 the operator ran an ordinary Claude Code 2.1.318 session in a
scratch folder with one `PreToolUse` hook on `Monitor` that saved each payload
and exited 2 when the payload held a marker path. The files are in
`docs/verification/evidence/tsk-1058/`.

- `capture1-command-allowed.json`: `tool_name` is `Monitor`, and the input is
  under `tool_input` with exactly three keys: `description`, `timeout_ms`
  (300000) and `command`. The command ran normally, so a hook that allows a
  `Monitor` call lets it run.
- `capture3-command-refused.json`: the same shape with a `command` that
  touches the marker path. The hook exited 2, the call was refused and the
  marker file does not exist (checked 22:53 the same night). A hook that
  refuses a `Monitor` call stops the tool before the command starts.
- The harness added `timeout_ms` although the request did not ask for it, so
  an ordinary call carries it.
- `version.txt` records `2.1.318 (Claude Code)` and the date; `notes.txt`
  records what was collected.

## What was found earlier

- Claude Code 2.1.317 (macOS, installed binary), read on 2026-11-03 by
  listing the strings embedded in the binary, once, before the coordinator
  asked that harness binaries not be read.
- The `Monitor` tool describes its `command` field as "Shell command or
  script. Each stdout line is an event; exit ends the watch." Its other
  fields are `description`, `timeout_ms` and `persistent`.
- The 2.1.317 session also reported the tool schema to the model, which
  allows no additional properties and lists `allowed_domains`, `command`,
  `description`, `timeout_ms` and `ws` (a socket watch, `{"ws": {"url": ...}}`,
  not combinable with `command`).
- No source in this repository or its docs described the shape. The only
  earlier repository mention of the tool is a tool-trace line in
  `docs/verification/evidence/tsk-113/ac2-native-run-2/tool-trace.jsonl`,
  which shows the tool name and no input.

## Version scope

- Confirmed natively: Claude Code 2.1.318, command form. Observed from the
  tool schema: 2.1.317.
- Not confirmed: any other Claude Code version, Anthropic's published docs,
  and the socket form from a hook payload. The `ws.url` shape comes from the
  2.1.317 tool schema only.
- A matcher entry for a tool name a build does not have matches nothing, so
  the wider matcher is harmless on builds without `Monitor`.

## What the judge does

- Before this change the shell hook matcher was `^(Bash|PowerShell)$`, so a
  `Monitor` call reached no guard, and `HookPayload::shell_command` returned
  nothing for it.
- Now the matcher is `^(Bash|PowerShell|Monitor)$` in the three presets and
  the installed copies, and `shell_command` returns `tool_input.command` for
  `Monitor`. Exec-guard treats `Monitor` like `PowerShell`: raw rules, which
  are no looser than `Bash`.
- An unverified build fails closed. Both guards accept a `Monitor` input only
  when it has a `command` string (judged like `Bash`) or a `ws` socket watch
  with a `url` string and no `command`, and only these fields: `command`,
  `description`, `timeout_ms`, `persistent`, `allowed_domains`, `ws`. Any
  other shape is refused (exit 2): neither `command` nor `ws`, an unknown
  field, both `command` and `ws`, or a malformed `ws`. The three keys of the
  native payload are all in the list, so ordinary use is not refused. One
  field is outside the list and is still accepted: `dangerouslyDisableSandbox`.
  Both guards read it as the sandbox flag on every shell tool, so it is never
  counted as an unknown field. It carries no shell text, and setting it only
  tightens the judgment (the sandbox-state check treats it as a strict reason).
- The refusal reads "unrecognised Monitor payload, not a bad command", names
  the field, and says the command was not judged. It points to the fix.
- The field list is configurable: `security.monitor_extra_fields` in
  `.codeflow/policy.json` adds names to the built-in list, so a harness update
  that adds a field does not break every adopter until a CodeFlow release.
  The list is read only from the policy published on the remote's default
  branch, as `security.text_guard: relaxed` is. A working copy, a local
  branch, or a repository with no remote cannot widen it, so a session cannot
  grant itself a wider list. The key only adds names: `command` is still
  judged, and a call must carry `command` or `ws`. Widening is a policy
  change the project owner makes. The key is not in the shipped policy file,
  so an older binary never meets it.
- Cost of failing closed: a build whose `Monitor` tool adds a new field has
  its calls refused until the owner lists the field or CodeFlow adds it to
  `MONITOR_FIELDS` in `crates/codeflow-core/src/hooks/git_guard.rs`.

## Evidence

- `crates/codeflow-core/src/hooks/git_guard.rs`
  (`test_monitor_problem_accepts_the_native_payload_shape`,
  `test_monitor_problem_refuses_unrecognised_shapes`,
  `test_monitor_problem_names_the_field_and_honours_listed_extras`,
  `test_payload_parse_monitor_command_and_socket_forms`)
- `crates/codeflow-cli/tests/hooks_cli/monitor_fields.rs`: the refusal names
  the field, the published policy widens the list, a session cannot widen it.
- `crates/codeflow-cli/tests/refusal_journey/monitor.rs`: the installed hooks.
- `crates/codeflow-cli/tests/settings_presets.rs`
  (`shell_guard_matcher_covers_the_monitor_tool_everywhere`)
- `docs/verification/evidence/tsk-1058/`: the native capture.
