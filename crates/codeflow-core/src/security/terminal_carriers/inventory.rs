//! The text-carrying routes the terminal/app carrier rule models.

pub(crate) const PROGRAMS: &[&str] = &["orca", "tmux", "screen", "osascript", "expect"];

/// A route names the command whose text is read, not a data-role exemption.
pub(super) const ROUTES: &[(&str, &str)] = &[
    ("orca", "terminal create"),
    ("orca", "terminal split"),
    ("orca", "terminal send"),
    ("orca", "computer type-text"),
    ("orca", "computer paste-text"),
    ("orca", "emulator type"),
    ("orca", "agent launch"),
    ("orca", "worktree create"),
    ("orca", "automations create"),
    ("orca", "orchestration worker-start"),
    ("orca", "orchestration send"),
    ("orca", "orchestration ask"),
    ("orca", "orchestration reply"),
    ("orca", "orchestration dispatch"),
    ("orca", "plugin dev"),
    ("orca", "plugin test"),
    ("orca", "plugin check"),
    ("tmux", "send-keys"),
    ("tmux", "send"),
    ("tmux", "paste-buffer"),
    ("tmux", "run-shell"),
    ("tmux", "new-session"),
    ("tmux", "new-window"),
    ("tmux", "split-window"),
    ("screen", "stuff"),
    ("screen", "command"),
    ("osascript", "keystroke"),
    ("osascript", "do script"),
    ("expect", "send"),
];

pub(super) fn contains(program: &str, route: &str) -> bool {
    ROUTES.contains(&(program, route))
}
