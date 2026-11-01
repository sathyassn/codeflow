#!/bin/sh
# Codeflow: a process wrapper for editors that start Claude Code themselves
# (TSK-1070, ADR-0093). Installed by `codeflow claude-session --install` as
# ~/.codeflow/shell/claude-wrapper.sh. The VS Code extension runs it as
# `claude-wrapper.sh <claude executable> <arguments>` when its
# `claudeCode.claudeProcessWrapper` setting names it.
#
# Inside a git checkout whose top level holds `.codeflow/project.toml`, it
# adds `--settings ~/.codeflow/claude/session.json` and refuses to start
# when that file is missing or another `--settings` is passed, as the
# `claude` shell function does: Claude Code keeps only the last
# `--settings`, so either start would run without CodeFlow's file. The
# refusal goes to standard error, which the editor reports as a failed
# start. Anywhere else it starts Claude Code unchanged.
_codeflow_exe=$1
shift
# An inherited marker would claim settings this start may not pass.
unset CODEFLOW_SESSION_SETTINGS
_codeflow_session="$HOME/.codeflow/claude/session.json"
_codeflow_top=$(command git rev-parse --show-toplevel 2>/dev/null) || _codeflow_top=
if [ -z "$_codeflow_top" ] || [ ! -f "$_codeflow_top/.codeflow/project.toml" ]; then
  exec "$_codeflow_exe" "$@"
fi
if [ ! -f "$_codeflow_session" ]; then
  echo "codeflow: ~/.codeflow/claude/session.json is missing, so this CodeFlow project would start without its session settings." >&2
  echo "  Restore it: codeflow claude-session --install" >&2
  echo "  Or remove claudeCode.claudeProcessWrapper from your editor settings to start without it." >&2
  exit 1
fi
for _codeflow_arg in "$@"; do
  case "$_codeflow_arg" in
    --settings|--settings=*)
      echo "codeflow: Claude Code uses only one --settings file, so the editor's would replace CodeFlow's session settings in this CodeFlow project." >&2
      echo "  Add those settings to ~/.codeflow/claude/session.json instead, or remove claudeCode.claudeProcessWrapper to start without CodeFlow's file." >&2
      exit 1
      ;;
  esac
done
CODEFLOW_SESSION_SETTINGS=1
export CODEFLOW_SESSION_SETTINGS
exec "$_codeflow_exe" --settings "$_codeflow_session" "$@"
