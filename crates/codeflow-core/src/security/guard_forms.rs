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

/// The verdict a round 2 probe is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// Refused, naming the protected target it reaches.
    Protected,
    /// Refused because its target cannot be proven: it depends on a
    /// construct the reader does not model exactly.
    Unproven,
    /// Runs.
    Allowed,
    /// The composed reader allows it, and the literal `rm` floor, which
    /// reads each `rm` whether or not the line can reach it, refuses it.
    Floor,
}

/// Every command of the Codex round 2 deletion probe (`extended-cases.json`,
/// 67 cases, read in a project holding `build`, `empty` and a `root-link`
/// to `/`), then the taint rule's own probes and the cleanups it keeps.
pub const REVIEW_ROUND_TWO_PROBES: &[(&str, &str, Expect)] = &[
    (
        "function-local-restores-outer",
        "D=/; f(){ local D=build; }; f; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "function-cd",
        "f(){ cd /; }; f; rm -rf *",
        Expect::Protected,
    ),
    (
        "function-return",
        "D=build; f(){ D=/; return; D=build; }; f; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "function-parameter",
        "f(){ D=$1; }; f /; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "function-direct-parameter",
        "f(){ rm -rf \"$1\"; }; f /",
        Expect::Protected,
    ),
    (
        "function-child-definition",
        "f(){ D=/; }; (f(){ D=build; }); f; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "function-child-unset",
        "f(){ D=/; }; (unset -f f); f; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "function-prefix-env",
        "f(){ rm -rf \"$D\"; }; D=/ f",
        Expect::Protected,
    ),
    (
        "function-cd-prefix",
        "f(){ cd \"$D\"; }; D=/ f; rm -rf *",
        Expect::Protected,
    ),
    (
        "break-simple",
        "D=build; for x in a; do D=/; break; D=build; done; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "continue-simple",
        "D=build; for x in a; do D=/; continue; D=build; done; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "break-in-function",
        "stop(){ break; }; for D in / build; do stop; done; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "break-two",
        "D=build; for a in x; do for b in y; do D=/; break 2; D=build; done; D=build; done; \
         rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "return-nested-loop",
        "D=build; f(){ for x in a; do D=/; return; D=build; done; D=build; }; f; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "nested-subshell",
        "D=/; ( (D=build) ); rm -rf \"$D\"",
        Expect::Protected,
    ),
    ("array-all", "D=(/); rm -rf \"${D[@]}\"", Expect::Protected),
    (
        "array-index",
        "D[0]=/; rm -rf \"${D[0]}\"",
        Expect::Protected,
    ),
    ("array-scalar", "D=(/); rm -rf \"$D\"", Expect::Protected),
    (
        "set-positional",
        "set -- /; rm -rf \"$1\"",
        Expect::Protected,
    ),
    ("set-at", "set -- /; rm -rf \"$@\"", Expect::Protected),
    (
        "shell-positional",
        "bash -c 'rm -rf \"$1\"' probe /",
        Expect::Protected,
    ),
    ("read-one", "read D <<< /; rm -rf \"$D\"", Expect::Protected),
    (
        "read-two",
        "read a D <<< \"build /\"; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "read-array",
        "read -a D <<< /; rm -rf \"${D[0]}\"",
        Expect::Protected,
    ),
    (
        "read-custom-ifs",
        "IFS=: read a D <<< \"build:/\"; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "export-child",
        "D=/; (export D=build); rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "export-child-nested",
        "D=/; ( (export D=build) ); rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "eval-literal",
        "eval 'D=/'; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "eval-with-command",
        "command eval 'D=/'; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "eval-with-builtin",
        "builtin eval 'D=/'; rm -rf \"$D\"",
        Expect::Protected,
    ),
    ("command-cd", "command cd /; rm -rf *", Expect::Protected),
    ("builtin-cd", "builtin cd /; rm -rf *", Expect::Protected),
    (
        "command-export",
        "command export D=/; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "alias-constant",
        "shopt -s expand_aliases; alias wipe='rm -rf';\nwipe /",
        Expect::Protected,
    ),
    (
        "ifs-mixed",
        "IFS=:; D=build:/; rm -rf $D",
        Expect::Protected,
    ),
    ("ifs-only-root", "IFS=:; D=:/; rm -rf $D", Expect::Protected),
    ("brace-system", "rm -rf /{etc,usr}", Expect::Protected),
    (
        "brace-nested",
        "rm -rf /{var/{root,empty},etc}",
        Expect::Protected,
    ),
    (
        "brace-assignment",
        "D=/; rm -rf {build,$D}",
        Expect::Protected,
    ),
    ("glob-home", "rm -rf /Users/*", Expect::Protected),
    ("glob-system", "rm -rf /et?", Expect::Protected),
    ("glob-root-link", "rm -rf root-*/*", Expect::Protected),
    (
        "glob-root-link-cd",
        "cd root-*/; rm -rf *",
        Expect::Protected,
    ),
    (
        "glob-bracket-link",
        "rm -rf root-lin[k]/*",
        Expect::Protected,
    ),
    (
        "child-variable-control",
        "D=build; (D=/); rm -rf \"$D\"",
        Expect::Allowed,
    ),
    (
        "function-local-safe",
        "D=build; f(){ local D=/; }; f; rm -rf \"$D\"",
        Expect::Allowed,
    ),
    (
        "function-not-called",
        "f(){ D=/; }; rm -rf ./build",
        Expect::Allowed,
    ),
    (
        "subshell-safe",
        "(cd /; pwd); rm -rf ./build",
        Expect::Allowed,
    ),
    ("quote-safe", "D=/; rm -rf '$D'", Expect::Allowed),
    (
        "default-safe",
        "D=build; rm -rf \"${D:-/}\"",
        Expect::Allowed,
    ),
    (
        "fixed-loop-safe",
        "for D in / build; do :; done; rm -rf \"$D\"",
        Expect::Allowed,
    ),
    (
        "loop-disclosed",
        "D=/; for d in x; do D=build; done; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "case-disclosed",
        "D=/; case x in x) D=build;; esac; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "rsync-source",
        "rsync -a --remove-source-files /Users/alice/ ./build/",
        Expect::Protected,
    ),
    (
        "rsync-del",
        "rsync -a --del ./empty/ /Users/alice/",
        Expect::Protected,
    ),
    (
        "rsync-nested",
        "D=/Users/alice; (rsync -a --delete ./empty/ \"$D\")",
        Expect::Protected,
    ),
    (
        "rsync-project",
        "rsync -a --delete ./empty/ ./build/",
        Expect::Allowed,
    ),
    (
        "find-wrapper-exec",
        r"find . -exec command rm -rf /Users/alice \;",
        Expect::Protected,
    ),
    (
        "find-name-now-block",
        "find /Users/alice -name .DS_Store -delete",
        Expect::Protected,
    ),
    (
        "find-project",
        "find . -name '*.o' -delete",
        Expect::Allowed,
    ),
    (
        "function-relative-safe",
        "f(){ cd build; }; f; rm -rf *",
        Expect::Allowed,
    ),
    (
        "unreachable-return-safe",
        "f(){ return; rm -rf /; }; f",
        Expect::Floor,
    ),
    ("unreachable-exit-safe", "exit 0; rm -rf /", Expect::Floor),
    ("glob-usr", "rm -rf /u?r", Expect::Protected),
    (
        "parameter-assign-side-effect",
        "unset D; : ${D:=/}; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "sequential-multi-assignment",
        "A=/ D=$A; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "read-option-r",
        "IFS=: read -r a D <<< \"build:/\"; rm -rf \"$D\"",
        Expect::Protected,
    ),
    // The taint rule: a construct the reader does not model exactly makes
    // what it may change unknown, and a deletion that depends on it is
    // refused as unproven.
    (
        "taint-source-variable",
        "source ./env.sh; rm -rf \"$D\"",
        Expect::Unproven,
    ),
    (
        "taint-source-cwd",
        ". ./env.sh; rm -rf build",
        Expect::Unproven,
    ),
    (
        "taint-recursion",
        "f(){ f; }; f; rm -rf \"$D\"",
        Expect::Unproven,
    ),
    (
        "taint-nameref",
        "declare -n R=D; R=/; rm -rf \"$D\"",
        Expect::Unproven,
    ),
    (
        "taint-case-attribute",
        "declare -u D; D=/usr; rm -rf \"$D\"",
        Expect::Unproven,
    ),
    (
        "taint-eval-unknown",
        "eval \"$(cat cmds)\"; rm -rf build",
        Expect::Unproven,
    ),
    (
        "taint-read-delimiter",
        "read -d '' D < list; rm -rf \"$D\"",
        Expect::Unproven,
    ),
    (
        "taint-mapfile",
        "mapfile -t L < list; rm -rf \"${L[@]}\"",
        Expect::Unproven,
    ),
    (
        "taint-read-unseen",
        "cat list | while read f; do rm -rf \"$f\"; done",
        Expect::Unproven,
    ),
    (
        "taint-substitution-op",
        "D=/xetc; rm -rf \"${D/x/}\"",
        Expect::Unproven,
    ),
    (
        "taint-indirection",
        "N=D; D=/; rm -rf \"${!N}\"",
        Expect::Unproven,
    ),
    (
        "taint-ifs-unknown",
        "IFS=$SEP; D=build:/; rm -rf $D",
        Expect::Unproven,
    ),
    ("taint-command-name", "$CMD /; rm -rf *", Expect::Unproven),
    (
        "taint-alias-unknown",
        "alias go=\"$GO\"; go; rm -rf *",
        Expect::Unproven,
    ),
    // Constructs modelled exactly keep the protected target in view.
    (
        "readonly-keeps",
        "readonly D=/; D=build; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "declare-global",
        "f(){ local D=build; declare -g D=/; }; f; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "unset-reveals-outer",
        "D=/; f(){ local D=build; g; rm -rf \"$D\"; }; g(){ unset D; }; f",
        Expect::Protected,
    ),
    (
        "function-shadows-cd",
        "cd(){ builtin cd /; }; cd build; rm -rf *",
        Expect::Protected,
    ),
    ("brace-range", "rm -rf /{a..f}tc", Expect::Protected),
    (
        "array-append",
        "D=(build); D+=(/); rm -rf \"${D[@]}\"",
        Expect::Protected,
    ),
    (
        "shift-positional",
        "set -- build /; shift; rm -rf \"$1\"",
        Expect::Protected,
    ),
    ("star-positional", "set -- /; rm -rf $*", Expect::Protected),
    (
        "shell-name-parameter",
        "bash -c 'rm -rf \"$0\"' /",
        Expect::Protected,
    ),
    ("glob-directory-link", "rm -rf */", Expect::Protected),
    (
        "alias-trailing-blank",
        "alias s='sudo '; alias w='rm -rf';\ns w /",
        Expect::Protected,
    ),
    (
        "compound-here-string",
        "while read -r d; do rm -rf \"$d\"; done <<< /",
        Expect::Protected,
    ),
    (
        "associative-array",
        "declare -A m=([a]=/); rm -rf \"${m[a]}\"",
        Expect::Protected,
    ),
    (
        "heredoc-to-shell",
        "D=/; cat <<EOF | sh\nrm -rf $D\nEOF",
        Expect::Protected,
    ),
    (
        "echo-to-shell",
        "D=/; echo \"rm -rf $D\" | sh",
        Expect::Protected,
    ),
    (
        "substitution-pwd",
        "D=$(cd / && pwd); rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "function-runs-arguments",
        "run(){ \"$@\"; }; run rm -rf /",
        Expect::Protected,
    ),
    (
        "printf-assign",
        "printf -v D %s /; rm -rf \"$D\"",
        Expect::Protected,
    ),
    (
        "trap-at-exit",
        "trap 'rm -rf \"$D\"' EXIT; D=/",
        Expect::Protected,
    ),
    (
        "mapfile-here-string",
        "mapfile -t L <<< /; rm -rf \"${L[@]}\"",
        Expect::Protected,
    ),
    (
        "heredoc-read",
        "read -r D <<EOF\n/\nEOF\nrm -rf \"$D\"",
        Expect::Protected,
    ),
    ("home-reassigned", "HOME=/; rm -rf ~/etc", Expect::Protected),
    (
        "taint-unsettled-loop",
        "D=x; while true; do D=$D/x; done; rm -rf \"$D\"",
        Expect::Unproven,
    ),
    // Ordinary cleanups built from the same constructs still run.
    (
        "control-array",
        "files=(build dist); rm -rf \"${files[@]}\"",
        Expect::Allowed,
    ),
    (
        "control-function-parameter",
        "clean(){ rm -rf \"$1\"; }; clean build",
        Expect::Allowed,
    ),
    (
        "control-function-local",
        "f(){ local d=build; rm -rf \"$d\"; }; f",
        Expect::Allowed,
    ),
    (
        "control-set-options",
        "set -euo pipefail; rm -rf build",
        Expect::Allowed,
    ),
    (
        "control-ifs",
        "IFS=:; D=build:dist; rm -rf $D",
        Expect::Allowed,
    ),
    (
        "control-read",
        "read -r D <<< build; rm -rf \"$D\"",
        Expect::Allowed,
    ),
    (
        "control-alias",
        "alias ll='ls -l'; rm -rf build",
        Expect::Allowed,
    ),
    (
        "control-builtin-cd",
        "builtin cd build && rm -rf *",
        Expect::Allowed,
    ),
    ("control-brace", "rm -rf build/{a,b}", Expect::Allowed),
    (
        "control-export",
        "export D=build; rm -rf \"$D\"",
        Expect::Allowed,
    ),
    (
        "control-readonly",
        "declare -r D=build; rm -rf \"$D\"",
        Expect::Allowed,
    ),
    (
        "control-assign-default",
        ": ${D:=build}; rm -rf \"$D\"",
        Expect::Allowed,
    ),
    (
        "control-positional-loop",
        "set -- build dist; for d; do rm -rf \"$d\"; done",
        Expect::Allowed,
    ),
    (
        "control-compound-here-string",
        "while read -r d; do rm -rf \"$d\"; done <<< build",
        Expect::Allowed,
    ),
    (
        "control-trap",
        "trap 'rm -rf ./build' EXIT",
        Expect::Allowed,
    ),
    (
        "control-substitution",
        "D=$(printf build); rm -rf \"$D\"",
        Expect::Allowed,
    ),
    ("control-glob", "rm -rf ./*", Expect::Allowed),
    ("control-pwd", "rm -rf \"$PWD/build\"", Expect::Allowed),
    (
        "control-source-then-cd",
        "source ./env.sh; cd /Users/alice/project && rm -rf build",
        Expect::Allowed,
    ),
    (
        "control-source-absolute",
        "source ./env.sh; rm -rf /Users/alice/project/target",
        Expect::Allowed,
    ),
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
    // Round 2 (Codex F7): a runner option that takes a value, the inline
    // call form, `npm exec`, `npm x`, `bun x`, a local binary through
    // `pnpm exec`, and an option the runner grammar does not know.
    ("codex exec x", "npx --cache /tmp @openai/codex exec x"),
    ("codex exec x", "npx --workspace app @openai/codex exec x"),
    (
        "codex exec x",
        "npx --package=@openai/codex --call='codex exec x'",
    ),
    ("codex exec x", "npx -c='codex exec x'"),
    ("codex exec x", "npm exec -- @openai/codex exec x"),
    ("codex exec x", "npm x -- @openai/codex exec x"),
    (
        "codex exec x",
        "npm exec --package=@openai/codex -- codex exec x",
    ),
    (
        "codex exec x",
        "npm exec --package=@openai/codex -c 'codex exec x'",
    ),
    (
        "codex exec x",
        "npm x --package=@openai/codex -c 'codex exec x'",
    ),
    ("codex exec x", "npm --prefix app exec @openai/codex exec x"),
    ("codex exec x", "bun x @openai/codex exec x"),
    ("codex exec x", "bun x @openai/codex@0.157.1 exec x"),
    ("codex exec x", "bun x --package @openai/codex codex exec x"),
    ("codex exec x", "pnpm exec codex exec x"),
    ("codex exec x", "yarn codex exec x"),
    (
        "codex exec x",
        "npx --unknown-option value @openai/codex exec x",
    ),
    // The runner's own help or version exits before any package runs.
    ("codex --help exec x", "npx --help @openai/codex exec x"),
    ("codex --help exec x", "npx --version @openai/codex exec x"),
    ("codex --help exec x", "bunx --help @openai/codex exec x"),
    (
        "codex --help exec x",
        "pnpm --help dlx @openai/codex exec x",
    ),
    (
        "codex --help exec x",
        "pnpm --version dlx @openai/codex exec x",
    ),
    (
        "codex --help exec x",
        "yarn --help dlx @openai/codex exec x",
    ),
    (
        "codex --help exec x",
        "npm exec --help @openai/codex exec x",
    ),
    ("codex --help exec x", "npx prettier codex exec x"),
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
