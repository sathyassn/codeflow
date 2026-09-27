## Claude worker effort preflight

Absence of an effort parameter on the Agent tool is not proof that Claude
cannot run stronger workers. Check the installed version's supported
[subagent definitions](https://code.claude.com/docs/en/sub-agents): `effort`
frontmatter or a session-scoped `--agents` JSON definition can set a child's
effort independently of the primary. At launch, define only the bounded worker
needed, with `description`, `prompt`, permitted `model`, and `effort`; the
owning Claude primary invokes that named `subagent_type`. In an existing
session, verify a supported definition is loaded before invoking it. Never
invent a missing Agent argument or install a permanent fleet of worker roles.

On a Codex host, before launching a Claude worker through the delegated
lifecycle, **read and follow**
`.claude/skills/cf-delegate/resources/claude-turn-completion.md`. Its
"Sequential turns" section governs that lifecycle (`wait --until terminal`,
continuation records): there, collect the worker result before the primary
returns. It does not govern an in-session Agent launch. A Claude host does
not load it. Claude Code runs Agent-tool subagents in the background; for a
same-family worker launched that way, the verified return is the task notification from
this session's own launch of that worker. Do not report the unit complete
before it arrives, then check the result in it against the brief.
Notifications from any other launch and background Bash watchers are not
completion. If the harness gives no such notification, the route is
unavailable for this dispatch; use another bounded native route and
preserve the existing Stop-hook and lifecycle safety policy unchanged.

Keep the primary at its default effort. Inspect effective
[effort precedence](https://code.claude.com/docs/en/model-config):
`CLAUDE_CODE_EFFORT_LEVEL` can override the child's definition, and supported
levels depend on the selected model and organization limits. Do not change
global settings or blanket defaults to make one worker stronger. Resolve an
override only within an authorized task process, retaining the primary's
explicit default; otherwise record the limitation. Use an approved binding's
route and a bounded native canary. Definition values remain requested until
native metadata verifies the model/effort; self-report is insufficient.
