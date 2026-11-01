//! Independent PR 346 review. Payloads are classifier inputs, never executed.
use std::path::Path;

/// Findings 21 through 31 correspond to independent-review items 1 through 11.
/// Item 6 is exercised by the native store-profile test instead of this table.
pub const CASES: &[(u8, &str, &str, bool)] = &[
    (21, "git-guard", "cat <<E\n$(rm -rf .git)\nE", true),
    (21, "git-guard", "cat <<E\n`rm -rf .git`\nE", true),
    (
        21,
        "git-guard",
        "cat <<E\n$(printf x > .codeflow/policy.json)\nE",
        true,
    ),
    (
        21,
        "exec-guard",
        "cat <<E\n$(python3 -c 'import shutil;shutil.rmtree(\".git\")')\nE",
        true,
    ),
    (21, "exec-guard", "cat <<E\n$(cat ~/.netrc)\nE", true),
    (21, "", "cat <<'E'\n$(rm -rf .git)\nE", false),
    (21, "", "cat <<'E'\n$(cat ~/.netrc)\nE", false),
    (22, "exec-guard", "grep -r x ~", true),
    (22, "exec-guard", "grep -r x ~/", true),
    (22, "exec-guard", "rg x ~", true),
    (22, "exec-guard", "rg -uu x ~", true),
    (22, "exec-guard", "rg --hidden x ~/", true),
    (22, "exec-guard", "cp -r ~ out", true),
    (22, "", "cat ~", false),
    (22, "", "cat ~/", false),
    (22, "", "cat \"$HOME/\"", false),
    (22, "", "cat '~/notes.txt'", false),
    (23, "exec-guard", "shopt -s dotglob; cat ~/*", true),
    (23, "exec-guard", "GLOBIGNORE=x; cat ~/*", true),
    (23, "exec-guard", "setopt globdots; cat ~/*", true),
    (23, "exec-guard", "cd ~ && shopt -s dotglob && cat *", true),
    (23, "", "shopt -s dotglob; cat README.md", false),
    (23, "", "shopt -s dotglob; cat 'src/*'", false),
    (24, "exec-guard", "cat $(echo ~/.netrc)", true),
    (24, "exec-guard", "echo ~/.netrc | xargs cat", true),
    (24, "", "echo README.md | xargs cat", false),
    (25, "", "ls ~/.claude/skills/*", false),
    (25, "", "cat ~/.claude/skills/*/SKILL.md", false),
    (25, "", "rg -g '*.md' x ~/.claude/skills", false),
    (25, "", "ls ~/.cargo/bin/*", false),
    (25, "", "cat ~/.claude/projects/*/*.md", false),
    (25, "exec-guard", "cat ~/.claude/sessions/*", true),
    (25, "exec-guard", "cat ~/.claude/projects/*/*.jsonl", true),
    (25, "exec-guard", "cat ~/.cargo/credentials*", true),
    (
        27,
        "git-guard",
        "ln -s .codeflow/rules l; cd -P l/.. && printf x > policy.json",
        true,
    ),
    (27, "", "cd -P clean && printf x > policy.json", false),
    (28, "exec-guard", "rg x .", true),
    (28, "exec-guard", "grep -r x .", true),
    (28, "exec-guard", "tar cf - src", true),
    (28, "exec-guard", "cp -r src out", true),
    (28, "exec-guard", "rg -g '!*.md' x .", true),
    (28, "exec-guard", "rg --no-ignore x scan", true),
    (28, "exec-guard", "rg -uu x scan", true),
    (28, "exec-guard", "grep -r x scan", true),
    (28, "exec-guard", "tar cf - scan", true),
    (28, "exec-guard", "cp -r scan out", true),
    (28, "exec-guard", "rg x scan", true),
    (28, "exec-guard", "rg -g '!*.md' x scan", true),
    (28, "", "rg x clean", false),
    (28, "", "grep -r x clean", false),
    (28, "", "tar cf - clean", false),
    (28, "", "cp -r clean out", false),
    (28, "", "rg -g '!*.md' x clean", false),
    // TSK-1070 lists `~/.config/gh` as a token store in the action table,
    // so these readers now follow that rule into a refusal.
    (29, "exec-guard", "cat ~/.config/gh/config.yml", true),
    (29, "exec-guard", "cat ~/.config/gh/*.yml", true),
    (29, "exec-guard", "ls ~/.config/gh/*", true),
    (29, "exec-guard", "cat ~/.config/gh/empty/*.rs", true),
    (30, "", "python3 tool.py many/*", false),
    (30, "", "cat many/*", false),
    (30, "exec-guard", "cat many-secret/*", true),
    (
        31,
        "exec-guard",
        "deno eval 'Deno.readTextFileSync(\"~/.codex/auth.json\")'",
        true,
    ),
    (
        31,
        "exec-guard",
        "deno eval 'Deno.removeSync(\".git\", {recursive: true})'",
        true,
    ),
    (31, "", "deno eval 'console.log(\"hello\")'", false),
];

pub fn seed(finding: u8, root: &Path, home: &Path) {
    for directory in [
        ".claude/skills/fixture",
        ".claude/sessions",
        ".claude/projects/fixture",
        ".cargo/bin",
        ".config/gh/empty",
    ] {
        std::fs::create_dir_all(home.join(directory)).unwrap();
    }
    for file in [
        ".claude/skills/fixture/SKILL.md",
        ".claude/sessions/fixture.jsonl",
        ".claude/projects/fixture/notes.md",
        ".claude/projects/fixture/trace.jsonl",
        ".cargo/bin/cargo-fixture",
        ".cargo/credentials-fixture",
        ".config/gh/config.yml",
    ] {
        std::fs::write(home.join(file), "ordinary or fake secret fixture\n").unwrap();
    }
    std::fs::create_dir_all(root.join("clean")).unwrap();
    std::fs::write(root.join("clean/public.rs"), "public fixture\n").unwrap();
    if finding == 27 {
        std::fs::create_dir_all(root.join(".codeflow/rules")).unwrap();
    }
    if finding == 28 {
        std::fs::write(root.join("src/id_rsa"), "fake secret fixture\n").unwrap();
    }
    if finding == 30 {
        std::fs::create_dir(root.join("many")).unwrap();
        for n in 0..5000 {
            std::fs::write(root.join("many").join(format!("f{n}")), "").unwrap();
        }
        std::fs::create_dir(root.join("many-secret")).unwrap();
        std::fs::write(root.join("many-secret/id_rsa"), "fake secret fixture\n").unwrap();
    }
}
