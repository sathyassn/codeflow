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
    // Round 1 review (Codex): predicates never make a protected find safe,
    // an exec'd remover is judged on its own, wrappers and here-strings keep
    // xargs input, a value a branch, subshell or `cd -` may leave is still
    // feasible, home expressions and `~user` are the home, and rsync's
    // deletion reaches its destination.
    ("find / -name '*' -delete", "rm -rf /"),
    ("find / -name '*.unlikely' -o -delete", "rm -rf /"),
    ("find / ! -name keep-this -delete", "rm -rf /"),
    ("find ~ -name .DS_Store -delete", "rm -rf ~"),
    ("find / -type f -exec rm {} +", "rm -rf /"),
    ("find /Users -name alice -exec rm -rf {} +", "rm -rf /Users"),
    (
        r"find . -maxdepth 0 -exec rm -rf /Users/alice \;",
        "rm -rf ~",
    ),
    ("find / | xargs rm", "rm -rf /"),
    ("find / -print0 | command xargs -0 rm -rf", "rm -rf /"),
    ("find / -print0 | env xargs -0 rm -rf", "rm -rf /"),
    ("find / -print0 | xargs -0 command rm -rf", "rm -rf /"),
    ("xargs rm -rf <<< /Users/alice", "rm -rf ~"),
    ("parallel rm -rf ::: /Users/alice", "rm -rf ~"),
    (
        "find / -type d | while read d; do rm -rf \"$d\"; done",
        "rm -rf /",
    ),
    ("cd /; cd /tmp; cd -; rm -rf *", "rm -rf /*"),
    ("D=/; (D=build); rm -rf $D", "rm -rf /"),
    ("D=/; if false; then D=build; fi; rm -rf $D", "rm -rf /"),
    ("D=/; true || D=build; rm -rf $D", "rm -rf /"),
    ("D=/; unset D; rm -rf ${D:-/}", "rm -rf /"),
    ("for d in / build; do break; done; rm -rf $d", "rm -rf /"),
    ("f() { cd /; }; f; rm -rf *", "rm -rf /*"),
    ("rm -rf \"${HOME:?}\"/*", "rm -rf ~/*"),
    ("rm -rf \"${HOME:-/}\"", "rm -rf ~"),
    ("rm -rf ~root", "rm -rf ~"),
    ("cd ~bob && rm -rf *", "rm -rf ~/*"),
    ("nice -n 5 rm -rf /Users/alice", "rm -rf ~"),
    ("rm -rf \"$(printf /Users/alice)\"", "rm -rf ~"),
    ("rm -rf /{etc,tmp/x}", "rm -rf /etc"),
    ("rsync -a --delete ./empty/ /Users/alice/", "rm -rf ~"),
    ("rsync -a --delete-after ./empty/ /", "rm -rf /"),
    ("rsync -a --del ./empty/ ~", "rm -rf ~"),
    ("rsync -a --remove-source-files / ./backup/", "rm -rf /"),
    // A `break`, `continue` or `return` leaves its loop or function with
    // the value it holds there, not the one the rest of the body sets.
    (
        "D=build; for x in a; do D=/; break; D=build; done; rm -rf $D",
        "rm -rf /",
    ),
    (
        "D=build; for x in a b; do D=/; continue; D=build; done; rm -rf $D",
        "rm -rf /",
    ),
    (
        "D=build; while true; do D=/; break; D=build; done; rm -rf $D",
        "rm -rf /",
    ),
    (
        "f() { D=/; return; D=build; }; D=build; f; rm -rf $D",
        "rm -rf /",
    ),
    (
        "cd build; for x in a; do cd /; break; cd build; done; rm -rf *",
        "rm -rf /*",
    ),
    (
        "D=build; for x in a; do for y in b; do D=/; break 2; done; D=build; done; rm -rf $D",
        "rm -rf /",
    ),
    (
        "f() { for d in / build; do return; done; }; f; rm -rf $d",
        "rm -rf /",
    ),
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
    "find . -name '*.o' -exec rm -f {} +",
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
    // Round 1 review: quoting, scope and plain narrowing stay precise.
    "D=/; rm -rf '$D'",
    "(cd /; pwd); rm -rf *",
    "for d in / build; do :; done; rm -rf $d",
    "D=/; D=build; rm -rf \"$D\"",
    "cd build; cd -; rm -rf dist",
    "rm -rf \"$TMPDIR/codeflow-build\"",
    "git clean -fdx -- build/",
    "rsync -a --delete ./src/ ./build/",
    "rsync -a --delete ./dist/ host:/srv/app/",
    "find / -name '*.log' | xargs ls",
];

/// Every command of the Codex round 1 deletion probe
/// (`deletion-results.json`, 66 cases), with the verdict this task holds it
/// to: `true` is refused. The two symlink cases need a fixture and are in
/// [`REVIEW_SYMLINK_PROBES`]. Codex expected `find ~ -name .DS_Store
/// -delete` to pass; the round 1 ruling refuses it, and `for d in / build;
/// do :; done; rm -rf $d` passes because the loop leaves `build`.
pub const REVIEW_PROBES: &[(&str, &str, bool)] = &[
    ("root-control", "rm -rf /", true),
    ("home-control", "rm -rf ~", true),
    ("quoted-root", "rm -rf \"/\"", true),
    ("escaped-rm", r"r\m -rf /", true),
    ("quoted-words", "'rm' '-rf' '/'", true),
    ("concat-word", "r''m -rf /", true),
    ("find-root", "find / -delete", true),
    ("find-name-star", "find / -name '*' -delete", true),
    ("find-type-dir", "find / -type d -exec rm -rf {} +", true),
    ("find-or", "find / -name '*.unlikely' -o -delete", true),
    (
        "find-negated-name",
        "find / ! -name keep-this -delete",
        true,
    ),
    (
        "find-parent-name",
        "find /Users -name alice -exec rm -rf {} +",
        true,
    ),
    (
        "find-exec-home",
        r"find . -maxdepth 0 -exec rm -rf /Users/alice \;",
        true,
    ),
    (
        "find-execdir",
        "find /Users/alice -execdir rm -rf {} +",
        true,
    ),
    ("xargs-null", "find / -print0 | xargs -0 rm -rf", true),
    (
        "xargs-wrapper",
        "find / -print0 | command xargs -0 rm -rf",
        true,
    ),
    ("xargs-env", "find / -print0 | env xargs -0 rm -rf", true),
    (
        "xargs-rm-wrapper",
        "find / -print0 | xargs -0 command rm -rf",
        true,
    ),
    ("xargs-here", "xargs rm -rf <<< /Users/alice", true),
    (
        "xargs-format",
        r"printf '%s\0' /Users/alice | xargs -0 rm -rf",
        true,
    ),
    ("parallel", "parallel rm -rf ::: /Users/alice", true),
    ("cd-home", "cd ~ && rm -rf *", true),
    ("cd-last", "cd /; cd /tmp; cd -; rm -rf *", true),
    ("pushd-home", "pushd /Users/alice; rm -rf *", true),
    ("subshell-cwd-leak", "cd /; (cd /tmp); rm -rf *", true),
    ("subshell-var-leak", "D=/; (D=build); rm -rf $D", true),
    (
        "false-branch-var",
        "D=/; if false; then D=build; fi; rm -rf $D",
        true,
    ),
    (
        "for-cd-multivalue",
        "for d in /tmp /; do cd \"$d\"; rm -rf *; done",
        true,
    ),
    (
        "for-var-narrowing",
        "for d in / build; do :; done; rm -rf $d",
        false,
    ),
    ("variable-assignment", "D=/Users/alice; rm -rf \"$D\"", true),
    ("variable-reassign", "D=/; D=build; rm -rf \"$D\"", false),
    ("variable-unset", "D=/; unset D; rm -rf ${D:-/}", true),
    ("home-braced", "rm -rf \"${HOME}\"", true),
    ("home-required", "rm -rf \"${HOME:?}\"/*", true),
    ("home-default", "rm -rf \"${HOME:-/}\"", true),
    ("home-tilde-user", "rm -rf ~root", true),
    ("dotdot", "rm -rf /home/alice/project/..", true),
    ("eval-simple", "eval 'rm -rf /Users/alice'", true),
    ("eval-compound", "eval 'D=/Users/alice; rm -rf $D'", true),
    ("sh-simple", "sh -c 'rm -rf /Users/alice'", true),
    ("sh-compound", "sh -c 'cd /Users/alice; rm -rf *'", true),
    ("bash-compound", "bash -c 'D=/Users/alice; rm -rf $D'", true),
    ("env-simple", "env rm -rf /Users/alice", true),
    ("command-simple", "command rm -rf /Users/alice", true),
    ("nice-simple", "nice -n 5 rm -rf /Users/alice", true),
    ("sudo-simple", "sudo rm -rf /Users/alice", true),
    (
        "substitution-path",
        "rm -rf \"$(printf /Users/alice)\"",
        true,
    ),
    ("backtick-path", "rm -rf `printf /Users/alice`", true),
    ("ifs", "IFS=:; D=/Users/alice:; rm -rf $D", true),
    (
        "rsync-delete",
        "rsync -a --delete ./empty/ /Users/alice/",
        true,
    ),
    ("powershell-home", "Remove-Item -Recurse $HOME", true),
    ("powershell-root", r"Remove-Item -Recurse C:\", true),
    ("powershell-short", r"Remove-Item -r C:\", true),
    ("build", "rm -rf ./build", false),
    ("find-dot", "find . -delete", false),
    ("find-objects", "find . -name '*.o' -delete", false),
    ("find-home-dsstore", "find ~ -name .DS_Store -delete", true),
    ("git-clean", "git clean -fdx", false),
    ("git-clean-project", "git clean -fdx -- build/", false),
    ("tmpdir", "rm -rf \"$TMPDIR/codeflow-build\"", false),
    ("project-variable", "D=./build; rm -rf \"$D\"", false),
    ("quoted-variable-data", "D=/; rm -rf '$D'", false),
    ("subshell-false-refusal", "(cd /; pwd); rm -rf *", false),
    ("printf-data", "printf '%s' 'find / -delete'", false),
];

/// The probe's two symlink cases: `root-link` is a project entry that
/// links to `/`, so both reach the root.
pub const REVIEW_SYMLINK_PROBES: &[(&str, &str)] = &[
    ("symlink-glob", "rm -rf root-link/*"),
    ("symlink-cd", "cd root-link && rm -rf *"),
];

/// A peer run started through a package runner beside its direct form:
/// both are judged alike (TSK-141 AC-6).
pub const PACKAGE_RUNNER_PAIRS: &[(&str, &str)] = &[
    ("claude -p hi", "npx @anthropic-ai/claude-code -p hi"),
    (
        "claude -p hi",
        "npx -y @anthropic-ai/claude-code@latest -p hi",
    ),
    (
        "claude -p hi",
        "npx --package @anthropic-ai/claude-code claude -p hi",
    ),
    ("claude -p hi", "bunx @anthropic-ai/claude-code -p hi"),
    ("codex exec x", "npx @openai/codex exec x"),
    ("codex exec x", "bunx @openai/codex@0.157.1 exec x"),
    ("codex exec x", "pnpm dlx @openai/codex exec x"),
    ("codex exec x", "yarn dlx @openai/codex exec x"),
    ("grok -p x", "pnpm --silent dlx @vibe-kit/grok-cli -p x"),
    ("grok -p x", "yarn dlx -q grok -p x"),
    (
        "claude --help -p",
        "npx @anthropic-ai/claude-code --help -p",
    ),
    ("codex exec --help", "pnpm dlx @openai/codex exec --help"),
    (
        "claude -p -- --help",
        "yarn dlx @anthropic-ai/claude-code -p -- --help",
    ),
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
