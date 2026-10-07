//! Certified prose (TSK-233, issue 66).
//!
//! The privilege, headless-peer and destructive-command checks match raw text
//! with the 3.0.0 substring rules. That refuses a line whose words only look
//! like a launcher: `; supersedes` in a heredoc, `grep '; su' notes.md`. No
//! reading of the raw line can be weaker than the substring floor and stay
//! safe, because the shell can turn almost any word into a program name and a
//! program's arguments can execute. So the floor stays, and a line is
//! relieved only by a positive proof made here.
//!
//! [`certify`] is a strict tokenizer of its own, independent of git-guard's
//! reader. It accepts one small grammar and refuses everything else, so a
//! refusal means "judge this line with the 3.0.0 rules", never "unsafe":
//!
//! ```text
//! line     := command (sep command)* [ws]
//! sep      := ws* ( ';' | '&&' | '||' | '|' | NEWLINE ) ws*
//! command  := program (ws (arg | redirect | heredoc))*
//! program  := 'echo' | 'printf' | 'grep' | 'cat'   (bare, exact)
//! arg      := bare | 'single quoted' | "double quoted"
//! bare     := [A-Za-z0-9_./:,%=+@-]+   (not starting with '=')
//! squote   := "'" [^']* "'"
//! dquote   := '"' [^"\\$`]* '"'
//! redirect := ('>' | '>>' | '2>') ws* doc | '2>&1' | '>&2'
//! doc      := bare ending in .md .markdown .txt .rst or .log
//! heredoc  := '<<' ("'" delim "'" | '"' delim '"')   (cat only, one per line)
//! delim    := [A-Za-z0-9_]+
//! ```
//!
//! A token ends at whitespace or a separator, so `'a'b` and `a'b'` refuse. A
//! heredoc body runs to a line equal to the delimiter, which must be the last
//! line; the body is data and is never read as commands. A control character
//! (other than a newline or a tab), non-ASCII whitespace, a backslash outside
//! single quotes, `$`, a backtick, `#`, brackets, braces, `*`, `?`, `~`, `!`,
//! a lone `&`, a program given by path or after an assignment, an unterminated
//! quote or heredoc, an unquoted or dashed delimiter, and a redirect under
//! `/dev/`, `/proc/` or `/sys/` all refuse.
//!
//! `echo`, `grep` and `cat` execute nothing from their arguments. `printf`
//! does when its format has a conversion other than `%s` or `%%` (`%n` and the
//! numeric conversions assign and evaluate arithmetic in zsh), when an escape
//! other than `\n`, `\t` and `\\` can spell one (zsh decodes `\u0025`), or when
//! it is given an option (`-v` assigns a variable), so its format must be the
//! first word, not start with `-`, and hold only `%s`, `%%` and those escapes.
//! `rg` is not on the list: a config file or `--pre` can make it run a program.
//!
//! A certified line may write a document file (`.md`, `.markdown`, `.txt`,
//! `.rst` or `.log`) so an agent can write prose to a file and pass it by
//! path. A document name proves nothing about what the write reaches, so the
//! caller also checks each target on disk with [`writes_are_plain_files`]: a
//! new file or an existing plain, non-executable file with one hard link, in
//! a directory that is not under `/dev`, `/proc` or `/sys` once links are
//! resolved, never a link, a pipe, a device or an executable. The file's later use is judged on the line that
//! uses it, as for any file; this module cannot prove a document is never run
//! as a script.
//! Whether a line is a POSIX shell line is the caller's call: only the Bash
//! and `run_terminal_command` tools reach [`certify`].

/// One simple command: the program and its words with quotes removed.
pub type Argv = Vec<String>;

/// A certified line: its commands, and the files its redirects write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Certified {
    pub commands: Vec<Argv>,
    /// Redirect targets exactly as written, relative to the call's directory.
    pub writes: Vec<String>,
}

/// The commands of `raw` when it is certified prose, else `None`.
#[must_use]
pub fn certify(raw: &str) -> Option<Vec<Argv>> {
    certify_with_writes(raw).map(|certified| certified.commands)
}

/// [`certify`] with the redirect targets, so the caller can check what each
/// one names on disk with [`writes_are_plain_files`].
#[must_use]
pub fn certify_with_writes(raw: &str) -> Option<Certified> {
    let mut text = raw;
    while let Some(rest) = text.strip_suffix('\n') {
        text = rest;
    }
    if text.is_empty() || !text.chars().all(char_allowed) {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut parser = Parser {
        chars: &chars,
        at: 0,
        commands: Vec::new(),
        writes: Vec::new(),
        heredoc: None,
        heredoc_seen: false,
        multiline_quote: false,
    };
    parser.line()?;
    if parser.heredoc_seen && parser.multiline_quote {
        return None;
    }
    parser
        .commands
        .iter()
        .all(command_allowed)
        .then_some(Certified {
            commands: parser.commands,
            writes: parser.writes,
        })
}

/// True when every redirect target is a place a prose write cannot do harm:
/// a file that does not exist yet, or an existing regular file that is not a
/// symbolic link, has no execute bit and has no other hard link, in a
/// directory that exists and, once links in the path are resolved, is not
/// under `/dev`, `/proc` or `/sys`. A symbolic link, a hard link, a named
/// pipe, a device and an executable file refuse, because a document name
/// proves nothing about what the write reaches. Checked when the call is
/// judged, so it covers a file or link made by an earlier call.
#[must_use]
pub fn writes_are_plain_files(writes: &[String], cwd: &std::path::Path) -> bool {
    writes.iter().all(|target| plain_target(target, cwd))
}

fn plain_target(target: &str, cwd: &std::path::Path) -> bool {
    let path = cwd.join(target);
    let Some(name) = path.file_name() else {
        return false;
    };
    let Some(parent) = path
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
    else {
        return false;
    };
    if ["/dev", "/proc", "/sys"]
        .iter()
        .any(|root| parent.starts_with(root))
    {
        return false;
    }
    match std::fs::symlink_metadata(parent.join(name)) {
        Ok(meta) => meta.file_type().is_file() && !is_executable(&meta) && !is_linked(&meta),
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

/// Without Unix modes an existing file cannot be shown to be plain, so any
/// existing target refuses certification; only a new file is certified.
#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    true
}

/// True when the file has another hard link, so a write here may also change
/// a file under another name.
#[cfg(unix)]
fn is_linked(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.nlink() > 1
}

#[cfg(not(unix))]
fn is_linked(_meta: &std::fs::Metadata) -> bool {
    false
}

/// Control characters other than newline and tab, and non-ASCII whitespace,
/// are never part of certified prose.
fn char_allowed(c: char) -> bool {
    if c == '\n' || c == '\t' {
        return true;
    }
    !c.is_control() && (c.is_ascii() || !c.is_whitespace())
}

fn bare_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '_' | '.' | '/' | ':' | ',' | '%' | '=' | '+' | '@' | '-')
}

fn is_ws(c: char) -> bool {
    c == ' ' || c == '\t'
}

struct Parser<'a> {
    chars: &'a [char],
    at: usize,
    commands: Vec<Argv>,
    writes: Vec<String>,
    /// A heredoc delimiter whose body starts at the next newline.
    heredoc: Option<String>,
    heredoc_seen: bool,
    multiline_quote: bool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn starts_with(&self, pattern: &str) -> bool {
        pattern
            .chars()
            .enumerate()
            .all(|(i, c)| self.chars.get(self.at + i) == Some(&c))
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(is_ws) {
            self.at += 1;
        }
    }

    /// `line := command (sep command)* [ws]`, with the heredoc body read at
    /// the newline that ends the line that named it.
    fn line(&mut self) -> Option<()> {
        loop {
            self.skip_ws();
            self.command()?;
            self.skip_ws();
            match self.peek() {
                None => return self.heredoc.is_none().then_some(()),
                Some('\n') => {
                    if let Some(delim) = self.heredoc.take() {
                        self.at += 1;
                        return self.body(&delim);
                    }
                    self.at += 1;
                }
                Some(';') => {
                    self.at += 1;
                    if matches!(self.peek(), Some(';' | '&' | '|')) {
                        return None;
                    }
                }
                Some('&') => {
                    if !self.starts_with("&&") {
                        return None;
                    }
                    self.at += 2;
                    if matches!(self.peek(), Some('&' | '|' | ';')) {
                        return None;
                    }
                }
                Some('|') => {
                    self.at += 1;
                    if self.peek() == Some('|') {
                        self.at += 1;
                    }
                    if matches!(self.peek(), Some('&' | '|' | ';')) {
                        return None;
                    }
                }
                Some(_) => return None,
            }
        }
    }

    /// The heredoc body: lines up to one equal to `delim`, which must be the
    /// last line.
    fn body(&mut self, delim: &str) -> Option<()> {
        let rest: String = self.chars[self.at..].iter().collect();
        let mut offset = 0;
        for line in rest.split('\n') {
            if line == delim {
                let after = offset + line.len();
                return (after == rest.len()).then(|| self.at = self.chars.len());
            }
            offset += line.len() + 1;
        }
        None
    }

    /// One simple command: a listed program and its words.
    fn command(&mut self) -> Option<()> {
        let program = self.bare()?;
        if !matches!(program.as_str(), "echo" | "printf" | "grep" | "cat") {
            return None;
        }
        self.token_end()?;
        let mut argv = vec![program.clone()];
        loop {
            let before = self.at;
            self.skip_ws();
            let spaced = self.at > before;
            match self.peek() {
                None | Some('\n' | ';' | '&' | '|') => break,
                Some(_) if !spaced => return None,
                Some('\'') => argv.push(self.squote()?),
                Some('"') => argv.push(self.dquote()?),
                Some('<') => self.heredoc_marker(&program)?,
                Some('>') => self.redirect()?,
                Some('2') if self.starts_with("2>") => self.redirect()?,
                Some(_) => argv.push(self.bare()?),
            }
            self.token_end()?;
        }
        self.commands.push(argv);
        Some(())
    }

    /// A token ends at whitespace, a separator or the end of the line.
    fn token_end(&self) -> Option<()> {
        match self.peek() {
            None | Some(' ' | '\t' | '\n' | ';' | '&' | '|') => Some(()),
            Some(_) => None,
        }
    }

    fn bare(&mut self) -> Option<String> {
        let start = self.at;
        while self.peek().is_some_and(bare_char) {
            self.at += 1;
        }
        (self.at > start && self.chars[start] != '=')
            .then(|| self.chars[start..self.at].iter().collect())
    }

    fn squote(&mut self) -> Option<String> {
        self.at += 1;
        let start = self.at;
        while self.peek()? != '\'' {
            self.at += 1;
        }
        let word: String = self.chars[start..self.at].iter().collect();
        self.at += 1;
        self.multiline_quote |= word.contains('\n');
        Some(word)
    }

    fn dquote(&mut self) -> Option<String> {
        self.at += 1;
        let start = self.at;
        while self.peek()? != '"' {
            if matches!(self.peek(), Some('\\' | '$' | '`')) {
                return None;
            }
            self.at += 1;
        }
        let word: String = self.chars[start..self.at].iter().collect();
        self.at += 1;
        self.multiline_quote |= word.contains('\n');
        Some(word)
    }

    /// `>`, `>>` and `2>` with a bare target, or exactly `2>&1` and `>&2`.
    fn redirect(&mut self) -> Option<()> {
        for exact in ["2>&1", ">&2"] {
            if self.starts_with(exact) {
                self.at += exact.len();
                return Some(());
            }
        }
        for operator in ["2>", ">>", ">"] {
            if self.starts_with(operator) {
                self.at += operator.len();
                self.skip_ws();
                let target = self.bare()?;
                if !document_file(&target) {
                    return None;
                }
                self.writes.push(target);
                return Some(());
            }
        }
        None
    }

    /// `<<'DELIM'` or `<<"DELIM"` after `cat`; the body starts at the next
    /// newline.
    fn heredoc_marker(&mut self, program: &str) -> Option<()> {
        if program != "cat" || self.heredoc_seen || !self.starts_with("<<") {
            return None;
        }
        self.at += 2;
        let quote = self.peek().filter(|c| matches!(c, '\'' | '"'))?;
        self.at += 1;
        let start = self.at;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.at += 1;
        }
        let delim: String = self.chars[start..self.at].iter().collect();
        if delim.is_empty() || self.peek() != Some(quote) {
            return None;
        }
        self.at += 1;
        self.heredoc = Some(delim);
        self.heredoc_seen = true;
        Some(())
    }
}

/// A file name that ends in a document extension, with a name before it, and
/// is not under `/dev/`, `/proc/` or `/sys/`.
fn document_file(target: &str) -> bool {
    if ["/dev/", "/proc/", "/sys/"]
        .iter()
        .any(|prefix| target.starts_with(prefix))
    {
        return false;
    }
    let name = target.rsplit('/').next().unwrap_or(target);
    name.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty() && matches!(extension, "md" | "markdown" | "txt" | "rst" | "log")
    })
}

/// A `printf` format that holds only `%s` and `%%` conversions and only the
/// escapes `\n`, `\t` and `\\`. zsh decodes `\u0025` and every other numeric
/// escape before it reads conversions, so such an escape can spell `%n`.
fn string_format(format: &str) -> bool {
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        match c {
            '%' if !matches!(chars.next(), Some('s' | '%')) => return false,
            '\\' if !matches!(chars.next(), Some('n' | 't' | '\\')) => return false,
            _ => {}
        }
    }
    true
}

/// The per-program floor: a `printf` format of string conversions only.
fn command_allowed(argv: &Argv) -> bool {
    match argv[0].as_str() {
        "printf" => argv
            .get(1)
            .is_some_and(|format| !format.starts_with('-') && string_format(format)),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(line: &str) -> Vec<Vec<&'static str>> {
        let commands = certify(line).unwrap_or_else(|| panic!("should certify: {line:?}"));
        commands
            .iter()
            .map(|c| {
                c.iter()
                    .map(|w| &*Box::leak(w.clone().into_boxed_str()))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn redirect_targets_are_recorded_and_system_paths_refuse() {
        let certified =
            certify_with_writes("echo a > x.md; cat <<'EOF' >> d/y.log\nb\nEOF").unwrap();
        assert_eq!(certified.writes, ["x.md", "d/y.log"]);
        assert!(certify_with_writes("echo a 2>&1")
            .unwrap()
            .writes
            .is_empty());
        for target in ["/dev/example.md", "/proc/x.txt", "/sys/x.log"] {
            assert!(certify(&format!("echo a > {target}")).is_none(), "{target}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_write_must_reach_a_new_or_plain_non_executable_file() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let check = |name: &str| writes_are_plain_files(&[name.to_string()], cwd);
        assert!(check("new.md"), "a file that does not exist yet");
        std::fs::write(cwd.join("plain.md"), "x").unwrap();
        assert!(check("plain.md"), "an existing plain file");
        std::fs::write(cwd.join("job.sh"), "x").unwrap();
        std::fs::set_permissions(cwd.join("job.sh"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        std::fs::set_permissions(cwd.join("plain.md"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(!check("job.sh"), "an executable");
        symlink("job.sh", cwd.join("link.md")).unwrap();
        assert!(!check("link.md"), "a link to an executable");
        symlink("missing", cwd.join("dangling.md")).unwrap();
        assert!(!check("dangling.md"), "a dangling link");
        std::fs::hard_link(cwd.join("job.sh"), cwd.join("hard.md")).unwrap();
        assert!(!check("hard.md"), "a hard link to an executable");
        std::fs::write(cwd.join("script.py"), "x").unwrap();
        std::fs::set_permissions(
            cwd.join("script.py"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        std::fs::hard_link(cwd.join("script.py"), cwd.join("py.md")).unwrap();
        assert!(
            !check("py.md"),
            "a hard link to a script without execute bits"
        );
        assert!(!check("missing/new.md"), "a directory that does not exist");
        symlink("/dev", cwd.join("devlink")).unwrap();
        for target in [
            "devlink/x.md",
            "/./dev/x.md",
            "//dev/x.md",
            "/./proc/x.md",
            "/./sys/x.md",
        ] {
            assert!(!check(target), "{target}");
        }
        assert!(check("./new2.md"), "a leading dot-slash");
        std::fs::create_dir(cwd.join("dir.md")).unwrap();
        assert!(!check("dir.md"), "a directory");
        let fifo = cwd.join("pipe.md");
        assert!(std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        assert!(!check("pipe.md"), "a named pipe");
    }

    #[test]
    fn issue_66_lines_certify_with_their_argv() {
        assert_eq!(
            argv("printf '%s\\n' 'first; such as this' > note.md"),
            vec![vec!["printf", "%s\\n", "first; such as this"]]
        );
        assert_eq!(
            argv("grep -n '; su' file.txt"),
            vec![vec!["grep", "-n", "; su", "file.txt"]]
        );
        assert_eq!(
            argv("echo \"(...; supersedes A3's ...)\""),
            vec![vec!["echo", "(...; supersedes A3's ...)"]]
        );
        assert_eq!(
            argv("cat <<'EOF' > a.md\nA3 (supersedes; summary below)\nEOF"),
            vec![vec!["cat"]]
        );
        assert_eq!(
            argv("grep -rn -i 'chained su' docs/ | cat\n"),
            vec![
                vec!["grep", "-rn", "-i", "chained su", "docs/"],
                vec!["cat"]
            ]
        );
        assert_eq!(
            argv("grep -n '; su' src && echo ok; echo done"),
            vec![
                vec!["grep", "-n", "; su", "src"],
                vec!["echo", "ok"],
                vec!["echo", "done"]
            ]
        );
    }

    #[test]
    fn what_the_grammar_does_not_prove_is_not_certified() {
        for line in [
            "",
            "\n",
            "echo $HOME",
            "echo `id`",
            "echo \"$HOME\"",
            "echo \"a\\\"b\"",
            "echo a\\ b",
            "echo 'a'b",
            "echo a'b'",
            "echo ''x",
            "echo a # c",
            "echo a; ",
            "echo a;; echo b",
            "echo a &",
            "echo a & echo b",
            "echo a |& cat",
            "echo a &>f",
            "echo a > /dev/sda",
            "echo a >",
            "echo a >&f",
            "echo a 2>&2",
            "echo a>b",
            "echo a 1>b",
            "echo $(id)",
            "echo (a)",
            "echo {a,b}",
            "echo *",
            "echo a?",
            "echo [a]",
            "echo ~",
            "echo !x",
            "echo =sudo",
            "echo a <f",
            "echo a <<<b",
            "echo a <<EOF\nx\nEOF",
            "cat <<EOF\nx\nEOF",
            "cat <<-'EOF'\nx\nEOF",
            "cat <<'E-F'\nx\nE-F",
            "cat <<''\nx\n",
            "cat <<'EOF'\nx",
            "cat <<'EOF'\nx\nEOF\necho more",
            "cat <<'EOF'\nx\nEOF \n",
            "cat <<'EOF' <<'EOF2'\nx\nEOF\nEOF2",
            "echo <<'EOF'\nx\nEOF",
            "cat <<'EOF' 'a\nb'\nx\nEOF",
            "X=1 echo a",
            "/bin/echo a",
            "./echo a",
            "'echo' a",
            "\"echo\" a",
            "sudo echo a",
            "echo a; sudo id",
            "echo a && ls",
            "ECHO a",
            "echo a\r",
            "echo a\u{b}b",
            "echo a\u{a0}b",
            "echo a\u{2003}b",
            "printf -v x y",
            "printf -vx y",
            "printf '-v' x y",
            "rg --no-config x",
            "printf",
            "printf '%d' 1",
            "printf '%n' 'a[$(id)]'",
            "printf '%s %q' a b",
            "printf '%(%Y)T' -1",
            "printf '\\u0025n' marker",
            "printf '\\x25n' marker",
            "printf '\\045n' marker",
            "printf '\\0451' marker",
            "printf '\\U00000025n' marker",
            "printf '\\e' marker",
            "printf 'a\\' marker",
            "printf '%' a",
            "printf -- '%s' a",
            "printf '%s' a > s.sh",
            "echo a > s",
            "echo a > s.md.sh",
            "echo a > .md",
            "echo a > dir/",
            "echo a > s.MD",
            "echo a >> ~/.bashrc",
            "echo a > /dev/null",
            "cat <<'EOF' > run.sh\nx\nEOF",
            "echo 'a",
            "echo \"a",
        ] {
            assert!(certify(line).is_none(), "should not certify: {line:?}");
        }
    }

    #[test]
    fn quoted_data_may_hold_any_shell_text() {
        for line in [
            "echo 'a; sudo id && su - | doas sh $(x) `y` \\ # ~ * ?'",
            "echo \"a; sudo id && su - | doas sh # ~ * ? ! ( ) { } [ ]\"",
            "printf '%s' '$(rm -rf /)'",
            "printf '%s\\n%%' 'a' b",
            "printf 'a\\tb\\\\c\\n' x",
            "printf 'plain text' > notes/summary.md",
            "echo a >> log.txt 2> err.log",
            "grep -n 'LD_PRELOAD=x' f",
            "cat <<'EOF'\n$(sudo id)\n`su -`\n\\\nEOF",
            "cat <<\"EOF\" > s.md\ntrue; sudo id\nEOF\n",
        ] {
            assert!(certify(line).is_some(), "should certify: {line:?}");
        }
    }
}
