//! TSK-1002 planted command strings. Classifiers inspect these; none execute.
pub const CASES: &[(u8, &str, bool)] = &[
    (1, "rm -rf \"$(git rev-parse --show-toplevel)\"", true),
    (1, "printf x > \"$(git rev-parse --git-path config)\"", true),
    (1, "rm -rf `git rev-parse --show-toplevel`", true),
    (1, "rm -rf \"$(git rev-parse --show-toplevel)/.git\"", true),
    (2, "ln -s .codeflow/rules tmp-link; printf x > tmp-link/../policy.json", true),
    (2, "ln -s .codeflow/rules tmp-link; dd if=/dev/null of=tmp-link/../policy.json", true),
    (2, "ln -s .codeflow/rules tmp-link; python3 -c 'open(\"tmp-link/../policy.json\",\"w\")'", true),
    (3, "ln -s .claude tmp-link; cp target/retro/settings.json -ttmp-link", true),
    (3, "ln -s .claude tmp-link; cp target/retro/settings.json --target-directory=tmp-link", true),
    (3, "ln -s .claude tmp-link; mv target/retro/settings.json --target-directory=tmp-link", true),
    (3, "ln -s .claude tmp-link; install target/retro/settings.json --target-directory=tmp-link", true),
    (3, "ln -s .claude tmp-link; gcp target/retro/settings.json --target-directory=tmp-link", true),
    (3, "ln -s .claude tmp-link; cp -vttmp-link target/retro/settings.json", true),
    (4, "shopt -s expand_aliases\nalias wipe=\"rm -rf\"\nwipe .git", true),
    (4, "alias wipe=\"rm -rf\"; wipe .git", true),
    (4, "alias w='printf x >'\nw .codeflow/policy.json", true),
    (5, "git worktree add -b task/retro /tmp/retro-new; node -e 'require(\"fs\").rmSync(\"/tmp/retro-new\",{recursive:true})'", true),
    (5, "git worktree add -b task/retro /tmp/retro-new; python3 tool.py /tmp/retro-new", true),
    (5, "git worktree add -b task/retro /tmp/retro-new && python3 -c 'import shutil; shutil.rmtree(\"/tmp/retro-new\")'", true),
    (6, "D=$HOME/.codex/auth.json; cat \"$D\"", true),
    (6, "D=~/.ssh; cat \"$D/id_ed25519\"", true),
    (6, "wc -c \"$HOME/.codex/auth.json\"", true),
    (6, "git diff --no-index /dev/null \"$HOME/.codex/auth.json\"", true),
    (6, "od -c ~/.netrc", true),
    (6, "xxd id_rsa", true),
    (7, "rg -uu --hidden -g 'id_*' synthetic .", true),
    (7, "grep -r --include='id_*' synthetic .", true),
    (7, "rg --glob 'id_*' synthetic .", true),
    (7, "rg --iglob='*.PEM' synthetic .", true),
    (7, "rg --glob='*.{pem,txt}' synthetic .", true),
    (8, "cat {id_rsa,README.md}", true),
    (8, "cat [i]d_rsa", true),
    (8, "cat ~/{.codex/auth.json,notes}", true),
    (8, "cat *.{pem,key}", true),
    (8, "cat id_rs?", true),
    (8, "cat ~/.ss[h]/config", true),
    (9, "cat<~/.codex/auth.json", true),
    (9, "exec {fd}<~/.codex/auth.json; cat <&$fd", true),
    (9, "exec 3<~/.codex/auth.json; cat <&3", true),
    (9, "cat 0<~/.netrc", true),
    (9, "while read l; do echo \"$l\"; done<~/.netrc", true),
    (10, "base64 --input=\"$HOME/.codex/auth.json\"", true),
    (10, "grep -f$HOME/.codex/auth.json x", true),
    (10, "rg --file=$HOME/.codex/auth.json x", true),
    (10, "sort --files0-from=id_rsa", true),
    (6, "tar cf - ~/.ssh", true),
    (6, "python3 -c 'import sys;print(open(sys.argv[1]).read())' id_rsa", true),
    (6, "zip -r out.zip ~/.aws", true),
    (11, "cat 'dir\nname/id_rsa'", true),
    (13, "D=src; printf x > \"$D/settings.json\"", false),
    (13, "cat > notes.txt <<'EOF'\npython3 -c 'open(\".codeflow/policy.json\",\"w\")'\nEOF", false),
    (13, "cat > notes.txt <<'EOF'\npython3 edit.py .claude/settings.json\nEOF", false),
    (13, "rg 'id_rsa' README.md", false),
    (13, "grep -n id_rsa docs/policy-reference.md", false),
    (13, "cat '~somebody/.codex/auth.json'", false),
    (13, "cat < 'id_rs?'", false),
    (13, "cat README.md", false),
    (13, "rg -n \"fn main\" src", false),
    (13, "rg -g \"*.rs\" foo .", false),
    (13, "cat src/*.rs", false),
    (13, "ls -la", false),
    (13, "git status", false),
    (13, "cargo build", false),
    (13, "printf x > target/out.txt", false),
    (13, "cp README.md target/x.md", false),
    (13, "echo hello > /tmp/x", false),
];

/// The guard that owns a refusal. Allowed data must pass both guards.
pub fn refusing_guard(ac: u8, line: &str) -> &'static str {
    if ac >= 5 || (ac == 2 && line.contains("python3")) {
        "exec-guard"
    } else {
        "git-guard"
    }
}

/// Synthetic names make the accepted expansion rules deterministic. The
/// payloads remain classifier inputs; no fixture contents are executed.
pub fn seed_glob_matches(root: &std::path::Path) {
    for name in ["id_rsa", "fixture.pem", "fixture.key"] {
        std::fs::write(root.join(name), "planted fixture\n").unwrap();
    }
}
