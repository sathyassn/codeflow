//! The guard forms TSK-141 pins, in one place: the exec-guard unit tests
//! (`security::deletion`, `security::headless`, `hooks::exec_guard`) and
//! the CLI journeys (`tier_floor_e2e`, `codex_hooks`) include this file.

/// Each AC-1 form beside the `rm -rf` it is equivalent to; both are refused
/// with the same target (TSK-141 AC-1). The last rows are forms refused
/// before this task, kept as they are.
pub const COMPOSED_PAIRS: &[(&str, &str)] = &[
    ("find / -delete", "rm -rf /"),
    ("find ~ -delete", "rm -rf ~"),
    ("find /etc -delete", "rm -rf /etc"),
    ("find -L / -delete", "rm -rf /"),
    ("find / -exec rm -rf {} +", "rm -rf /"),
    (r"find / -exec rm -rf {} \;", "rm -rf /"),
    (
        "find /Users/alice -execdir rm -r {} +",
        "rm -rf /Users/alice",
    ),
    ("find / -print0 | xargs -0 rm -rf", "rm -rf /"),
    ("ls / | xargs rm -rf", "rm -rf /*"),
    ("ls ~ | xargs -n 1 rm -rf", "rm -rf ~/*"),
    ("echo / | xargs rm -rf", "rm -rf /"),
    ("ls /etc | xargs -I{} rm -rf /etc/{}", "rm -rf /etc/*"),
    ("xargs rm -rf / < /dev/null", "rm -rf /"),
    ("rm -rf /Users/alice", "rm -rf ~"),
    ("rm -rf /home/alice/", "rm -rf ~"),
    ("rm -rf /Users/alice/*", "rm -rf ~/*"),
    ("rm -rf /home/alice/proj/..", "rm -rf ~"),
    ("cd ~ && rm -rf *", "rm -rf ~/*"),
    ("cd; rm -rf ./*", "rm -rf ~/*"),
    ("cd $HOME && rm -rf .", "rm -rf ~"),
    ("cd / && rm -rf *", "rm -rf /*"),
    ("cd /etc; rm -rf .", "rm -rf /etc"),
    ("cd /Users && rm -rf alice", "rm -rf /Users/alice"),
    ("cd ~/code && rm -rf ..", "rm -rf ~"),
    ("cd / && find . -delete", "rm -rf /"),
    ("cd / && ls | xargs rm -rf", "rm -rf /*"),
    ("D=/; rm -rf $D", "rm -rf /"),
    ("D=/etc && rm -rf \"$D\"", "rm -rf /etc"),
    ("export D=~; rm -rf ${D}/*", "rm -rf ~/*"),
    ("D=/; find $D -delete", "rm -rf /"),
    ("D=/usr; cd $D && rm -rf *", "rm -rf /usr"),
    ("for d in / ~; do rm -rf \"$d\"; done", "rm -rf /"),
    ("(rm -rf /)", "rm -rf /"),
    ("{ rm -rf /; }", "rm -rf /"),
    ("if true; then rm -rf /; fi", "rm -rf /"),
    ("for i in 1 2; do rm -rf /; done", "rm -rf /"),
    ("while true; do rm -rf /; done", "rm -rf /"),
    ("until false; do rm -rf /; done", "rm -rf /"),
    ("case a in a) rm -rf /;; esac", "rm -rf /"),
    ("(cd ~ && rm -rf *)", "rm -rf ~/*"),
    ("sudo find / -delete", "rm -rf /"),
    ("rm -rf /*", "rm -rf /*"),
    ("rm -rf --no-preserve-root /", "rm -rf /"),
];

/// Deletions that stay inside the project: allowed, alone and nested
/// (TSK-141 AC-2).
pub const PROJECT_DELETIONS: &[&str] = &[
    "rm -rf ./build",
    "rm -rf target",
    "find . -name '*.o' -delete",
    "find ./build -delete",
    "find . -name node_modules -exec rm -rf {} +",
    "find ~ -name .DS_Store -delete",
    "D=./build; rm -rf $D",
    "D=build && find $D -delete",
    "cd build && rm -rf *",
    "cd ~/code/app && rm -rf target",
    "rm -rf ~/code/app/target",
    "rm -rf /Users/alice/project/target",
    "ls ./build | xargs rm -rf",
    "ls ~/code/app/build | xargs rm -rf",
    "find ./dist -print0 | xargs -0 rm -rf",
    "for d in build dist; do rm -rf \"$d\"; done",
    "cd /; ls",
    "find / -name '*.log'",
    "echo / | xargs ls",
    "rm -r /tmp/scratch",
];

/// The control structures and sequences a form is nested in: each `{}` is
/// replaced by the form.
pub const NESTINGS: &[&str] = &[
    "{}",
    "( {} )",
    "{ {}; }",
    "if true; then {}; fi",
    "if false; then :; else {}; fi",
    "for i in 1; do {}; done",
    "while true; do {}; break; done",
    "until false; do {}; break; done",
    "case a in a) {};; esac",
    "true && {}",
    "true; {}",
    "false || {}",
    "echo start\n{}",
    "( if true; then {}; fi )",
];

/// Each help or version invocation beside the same tokens given as a
/// prompt, after `--`, or as an option's value (TSK-141 AC-3, seeded
/// from the plan review's `followups-help-probe.log`).
pub const HELP_PAIRS: &[(&str, &str)] = &[
    ("claude --help -p", "claude -p -- --help"),
    ("claude -p --help", "claude -p -- help"),
    ("claude -p hi --help", "claude -p hi"),
    ("claude -h -p", "claude -p -- -h"),
    ("claude -ph", "claude --model help -p x"),
    (
        "claude --version --print",
        "claude --print --name version x",
    ),
    ("claude -v -p", "claude -p x -- -v"),
    ("codex exec --help", "codex exec -- --help"),
    ("codex exec -h", "codex exec -m help x"),
    ("codex exec --version", "codex exec --color help x"),
    ("codex exec -V", "codex exec -- -V"),
    ("codex --help exec x", "codex -m help exec x"),
    ("codex exec x --help", "codex exec x"),
    ("codex review --help", "codex review --title help"),
    ("codex e -h", "codex e -o help x"),
    ("grok --help -p x", "grok -p help"),
    ("grok -h --single x", "grok --single help"),
    ("grok --version agent", "grok agent -- --version"),
    ("grok -v -p x", "grok -m help -p x"),
    (
        "grok wrap claude --help -p",
        "grok wrap claude -p -- --help",
    ),
    (
        "timeout 5 codex exec --help",
        "timeout 5 codex exec -- --help",
    ),
    (
        "codex exec --help; echo done",
        "codex exec --help; claude -p hi",
    ),
];
