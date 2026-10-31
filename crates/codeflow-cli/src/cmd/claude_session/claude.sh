# CodeFlow: start Claude Code with CodeFlow's session settings inside a
# CodeFlow project (TSK-1070, ADR-0093). Installed by
# `codeflow claude-session --install`, which writes this file to
# ~/.codeflow/shell/claude.sh and, on your yes, sources it from your shell
# startup file. `command claude` always starts Claude Code without it.
#
# Inside a git checkout whose top level holds `.codeflow/project.toml`, a
# plain `claude` runs `claude --settings ~/.codeflow/claude/session.json`,
# which makes the sandbox binding: a repository file can no longer turn it
# off, rerun a refused command outside it, add exclusions or widen the host
# list. Anywhere else `claude` runs unchanged. The marker is the
# repository's, so a repository can only opt itself into this stricter
# start, never out of anything.
claude() {
  _codeflow_top=$(command git rev-parse --show-toplevel 2>/dev/null) || _codeflow_top=
  if [ -z "$_codeflow_top" ] || [ ! -f "$_codeflow_top/.codeflow/project.toml" ]; then
    unset _codeflow_top
    command claude "$@"
    return
  fi
  unset _codeflow_top
  _codeflow_session="$HOME/.codeflow/claude/session.json"
  if [ ! -f "$_codeflow_session" ]; then
    unset _codeflow_session
    echo "codeflow: ~/.codeflow/claude/session.json is missing, so this CodeFlow project would start without its session settings." >&2
    echo "  Restore it: codeflow claude-session --install" >&2
    echo "  Or start without it: command claude" >&2
    return 1
  fi
  # Claude Code keeps only the last of several `--settings` flags (checked
  # live on 2.1.316), so a second file would replace CodeFlow's: refuse.
  for _codeflow_arg in "$@"; do
    case "$_codeflow_arg" in
      --settings|--settings=*)
        unset _codeflow_arg _codeflow_session
        echo "codeflow: Claude Code uses only one --settings file, so yours would replace CodeFlow's session settings in this CodeFlow project." >&2
        echo "  Add your settings to ~/.codeflow/claude/session.json instead (codeflow claude-session --install keeps them)," >&2
        echo "  or start without CodeFlow's file: command claude $*" >&2
        return 1
        ;;
    esac
  done
  unset _codeflow_arg
  set -- --settings "$_codeflow_session" "$@"
  unset _codeflow_session
  CODEFLOW_SESSION_SETTINGS=1 command claude "$@"
}
