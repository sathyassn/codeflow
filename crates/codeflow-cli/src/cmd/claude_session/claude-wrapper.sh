#!/bin/sh
# Codeflow: a process wrapper for editors that start Claude Code themselves
# (TSK-1070, ADR-0093). Installed by `codeflow claude-session --install` as
# ~/.codeflow/shell/claude-wrapper.sh. The VS Code extension runs it as
# `claude-wrapper.sh <claude executable> <arguments>` when its
# `claudeCode.claudeProcessWrapper` setting names it.
#
# Inside a git checkout whose top level holds `.codeflow/project.toml`, it
# adds `--settings ~/.codeflow/claude/session.json`, as the `claude` shell
# function does. It never refuses: an editor cannot show this script's
# message, so when the file is missing or the editor passes its own
# `--settings`, it starts Claude Code unchanged and the session-start hook
# warns that the session has no session settings.
_codeflow_exe=$1
shift
_codeflow_session="$HOME/.codeflow/claude/session.json"
_codeflow_top=$(command git rev-parse --show-toplevel 2>/dev/null) || _codeflow_top=
if [ -n "$_codeflow_top" ] && [ -f "$_codeflow_top/.codeflow/project.toml" ] && [ -f "$_codeflow_session" ]; then
  for _codeflow_arg in "$@"; do
    case "$_codeflow_arg" in
      --settings|--settings=*) exec "$_codeflow_exe" "$@" ;;
    esac
  done
  CODEFLOW_SESSION_SETTINGS=1
  export CODEFLOW_SESSION_SETTINGS
  exec "$_codeflow_exe" --settings "$_codeflow_session" "$@"
fi
exec "$_codeflow_exe" "$@"
