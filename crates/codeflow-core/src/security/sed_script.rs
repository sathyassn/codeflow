//! Reading a `sed` script by sed's grammar (TSK-290, issue 147).
//!
//! The guards ask one question of a script: which files can it write? A
//! letter `w` answers nothing by itself. It writes only as a command (after
//! an optional address and `!`, at the start of a command), as the `w` flag
//! of an `s` command, and its capital `W` likewise. The same letter inside
//! an address regular expression, an `s` pattern or replacement, a `y`
//! operand, the text of `a`, `i` or `c`, a label or a file name read by
//! `r` or `R` is data.
//!
//! This reader walks a script by that grammar and reports the file names
//! the `w`, `W` and `s///w` forms write. It reads the union of the GNU and
//! BSD grammars where they differ, since the guard cannot tell which `sed`
//! runs:
//!
//! - A file name runs to the end of its line. [`ScriptReading::writes`]
//!   holds exactly those names. [`ScriptReading::shorter`] holds the names
//!   a `sed` would write if it ended a name at `;` or `}`, which no GNU or
//!   BSD `sed` does but a guard may not assume away.
//! - A regular expression is read twice, once taking a delimiter inside a
//!   bracket expression as the end (`s/[/]/x/` as GNU reads it) and once
//!   taking it as part of the bracket expression (as BSD reads it). A
//!   script one reading rejects is a script that `sed` itself rejects.
//! - A label ends at a blank or `;`, the shorter of the two readings, so
//!   the commands after it are read too.
//!
//! A script neither reading can parse is not modelled. The reader then
//! reports why, and falls back to the old text reading for that script
//! alone ([`crude_candidates`]): every `w` or `W` and the rest of its line.
//! That reading over-reports, and the guard refuses only when a name it
//! reports reaches an integrity path, so an unmodelled form is refused
//! where its writes could reach one and allowed otherwise.
//!
//! The reader writes and runs nothing. GNU's `e` command and the `e` flag
//! of `s` run a command; they write no file by themselves and stay judged
//! by the text floor of the guard, which refuses any argument that names an
//! enforcement path.

use std::collections::BTreeSet;

/// What one script writes.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ScriptReading {
    /// The files the script writes: each `w` or `W` command and `s///w`
    /// flag, with the file name read to the end of its line, as GNU and BSD
    /// `sed` read it.
    pub writes: Vec<String>,
    /// Further names a `sed` that ended a file name at `;` or `}` would
    /// write, and, for a script the reader does not model, every name the
    /// text reading finds.
    pub shorter: Vec<String>,
    /// Why neither reading could parse the script; `None` when it is
    /// modelled.
    pub unmodelled: Option<String>,
}

impl ScriptReading {
    /// Every name the script may write, exact names first.
    pub(crate) fn candidates(&self) -> impl Iterator<Item = &str> {
        self.writes.iter().chain(&self.shorter).map(String::as_str)
    }
}

/// Read one script, the text `sed` receives after joining its `-e` and `-f`
/// pieces with newlines.
pub(crate) fn read_script(script: &str) -> ScriptReading {
    let chars: Vec<char> = script.chars().collect();
    let mut writes = BTreeSet::new();
    let mut shorter = BTreeSet::new();
    let mut errors = Vec::new();
    let mut parsed = false;
    for brackets in [false, true] {
        let mut parser = Parser {
            chars: &chars,
            at: 0,
            brackets,
            writes: BTreeSet::new(),
            shorter: BTreeSet::new(),
        };
        match parser.run() {
            Ok(()) => {
                parsed = true;
                writes.append(&mut parser.writes);
                shorter.append(&mut parser.shorter);
            }
            Err(error) => errors.push(error),
        }
    }
    let unmodelled = (!parsed).then(|| errors.join("; "));
    if unmodelled.is_some() {
        shorter.extend(crude_candidates(script));
    }
    // A name that is also an exact write is not listed twice.
    shorter.retain(|name| !writes.contains(name));
    ScriptReading {
        writes: writes.into_iter().collect(),
        shorter: shorter.into_iter().collect(),
        unmodelled,
    }
}

/// The text reading: after each `w` or `W`, the rest of its line and that
/// rest cut at `;` and at `}`. It names far more than a script writes, so it
/// serves only a script the reader does not model, and the tail of a file
/// name that holds `;` or `}`.
pub(crate) fn crude_candidates(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in text.split('\n') {
        for (at, _) in line.match_indices(['w', 'W']) {
            let rest = &line[at + 1..];
            for candidate in [
                rest,
                rest.split(';').next().unwrap_or(rest),
                rest.split('}').next().unwrap_or(rest),
            ] {
                let start = candidate
                    .find(|c| c != ' ' && c != '\t')
                    .unwrap_or(candidate.len());
                let candidate = &candidate[start..];
                if !candidate.is_empty() && seen.insert(candidate) {
                    out.push(candidate.to_string());
                }
            }
        }
    }
    out
}

/// The blanks `sed` skips between the parts of a command.
fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// The characters that separate commands, and the rest of `sed`'s space.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}')
}

struct Parser<'a> {
    chars: &'a [char],
    at: usize,
    /// Whether a delimiter inside a bracket expression of a regular
    /// expression is part of it.
    brackets: bool,
    writes: BTreeSet<String>,
    shorter: BTreeSet<String>,
}

type Step = Result<(), String>;

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += 1;
        Some(c)
    }

    fn blanks(&mut self) {
        while self.peek().is_some_and(is_blank) {
            self.at += 1;
        }
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
    }

    fn run(&mut self) -> Step {
        loop {
            while self.peek().is_some_and(|c| is_space(c) || c == ';') {
                self.at += 1;
            }
            let Some(first) = self.peek() else {
                return Ok(());
            };
            if first == '#' {
                self.line();
                continue;
            }
            self.addresses()?;
            self.blanks();
            while self.peek() == Some('!') {
                self.at += 1;
                self.blanks();
            }
            let Some(command) = self.next() else {
                return Err("an address with no command".into());
            };
            self.command(command)?;
        }
    }

    /// The rest of the line, and its newline.
    fn line(&mut self) -> String {
        let start = self.at;
        while self.peek().is_some_and(|c| c != '\n') {
            self.at += 1;
        }
        let text = self.chars[start..self.at].iter().collect();
        if self.peek() == Some('\n') {
            self.at += 1;
        }
        text
    }

    fn command(&mut self, command: char) -> Step {
        match command {
            '{' => Ok(()),
            '}' | '=' | 'd' | 'D' | 'g' | 'G' | 'h' | 'H' | 'n' | 'N' | 'p' | 'P' | 'x' | 'z'
            | 'F' => self.end_of_command(),
            'l' | 'L' | 'q' | 'Q' => {
                self.blanks();
                self.digits();
                self.end_of_command()
            }
            ':' => {
                self.label(false);
                Ok(())
            }
            'b' | 't' | 'T' => {
                self.label(true);
                self.end_of_command()
            }
            'v' => {
                self.label(false);
                self.end_of_command()
            }
            'r' | 'R' => {
                let name = self.file_name()?;
                self.note_tail(&name, false);
                Ok(())
            }
            'w' | 'W' => {
                let name = self.file_name()?;
                self.note_tail(&name, true);
                self.writes.insert(name);
                Ok(())
            }
            'a' | 'i' | 'c' => self.text(),
            // GNU runs the rest of the line as a command: no file is named.
            'e' => {
                self.line();
                Ok(())
            }
            's' => self.substitute(),
            'y' => self.transliterate(),
            other => Err(format!("the command `{other}`")),
        }
    }

    /// What may follow a command: blanks, then a separator, the end of the
    /// script, or a `}` or `#` that is read as the next command.
    fn end_of_command(&mut self) -> Step {
        self.blanks();
        match self.peek() {
            None | Some('}' | '#') => Ok(()),
            Some('\n' | ';') => {
                self.at += 1;
                Ok(())
            }
            Some(other) => Err(format!("`{other}` after a command")),
        }
    }

    /// A label or version, which ends at a blank or `;`, the shorter of the
    /// readings of GNU and BSD. A branch label ends at `}` too.
    fn label(&mut self, branch: bool) {
        self.blanks();
        while self
            .peek()
            .is_some_and(|c| !is_space(c) && c != ';' && !(branch && c == '}'))
        {
            self.at += 1;
        }
    }

    /// A file name: after blanks, the rest of the line.
    fn file_name(&mut self) -> Result<String, String> {
        self.blanks();
        let name = self.line();
        if name.is_empty() {
            return Err("a file name command with no file name".into());
        }
        Ok(name)
    }

    /// The names a shorter reading of a file name adds. `writes` is false
    /// for a name that is only read.
    fn note_tail(&mut self, name: &str, writes: bool) {
        if !name.contains([';', '}']) {
            return;
        }
        if writes {
            for cut in [';', '}'] {
                if let Some((head, _)) = name.split_once(cut) {
                    if !head.is_empty() {
                        self.shorter.insert(head.to_string());
                    }
                }
            }
        }
        self.shorter.extend(crude_candidates(name));
    }

    /// The text of `a`, `i` or `c`: a one-line text, or `\` and the text on
    /// the following lines, each ending in a backslash while the text goes
    /// on.
    fn text(&mut self) -> Step {
        self.blanks();
        if self.peek() == Some('\\') {
            self.at += 1;
            self.blanks();
            if self.peek() == Some('\n') {
                self.at += 1;
            }
        } else if matches!(self.peek(), None | Some('\n')) {
            return Err("an `a`, `i` or `c` command with no text".into());
        }
        loop {
            match self.next() {
                None | Some('\n') => return Ok(()),
                Some('\\') => {
                    self.next();
                }
                Some(_) => {}
            }
        }
    }

    fn addresses(&mut self) -> Step {
        if !self.address(false)? {
            return Ok(());
        }
        self.blanks();
        if self.peek() == Some(',') {
            self.at += 1;
            self.blanks();
            if !self.address(true)? {
                return Err("a range with no second address".into());
            }
        }
        Ok(())
    }

    /// One address: a number (`first~step` in GNU), `$`, a regular
    /// expression between `/` or between `\` and the delimiter after it,
    /// with GNU's `I` and `M` flags, and, as a second address, GNU's `+N`
    /// and `~N`.
    fn address(&mut self, second: bool) -> Result<bool, String> {
        match self.peek() {
            Some(c) if c.is_ascii_digit() => {
                self.digits();
                if self.peek() == Some('~') {
                    self.at += 1;
                    self.digits();
                }
                Ok(true)
            }
            Some('$') => {
                self.at += 1;
                Ok(true)
            }
            Some('/') => {
                self.at += 1;
                self.delimited('/', true)?;
                self.address_flags();
                Ok(true)
            }
            Some('\\') => {
                self.at += 1;
                let delimiter = match self.next() {
                    Some(c) if c != '\n' && c != '\\' => c,
                    _ => return Err("a custom address delimiter that is not one".into()),
                };
                self.delimited(delimiter, true)?;
                self.address_flags();
                Ok(true)
            }
            Some('+' | '~') if second => {
                self.at += 1;
                self.digits();
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn address_flags(&mut self) {
        while matches!(self.peek(), Some('I' | 'M')) {
            self.at += 1;
        }
    }

    /// Text up to the next `delimiter` that a backslash does not escape,
    /// which is consumed with it. A raw newline ends nothing: `sed` rejects
    /// it, and so does this.
    fn delimited(&mut self, delimiter: char, regex: bool) -> Step {
        loop {
            match self.next() {
                None | Some('\n') => return Err("an unterminated pattern or text".into()),
                Some('\\') => {
                    if self.next().is_none() {
                        return Err("a backslash at the end of the script".into());
                    }
                }
                Some('[') if regex && self.brackets => self.bracket()?,
                Some(c) if c == delimiter => return Ok(()),
                Some(_) => {}
            }
        }
    }

    /// A bracket expression, read after its `[`: a leading `^`, a leading
    /// `]` that is a member, and the classes `[:x:]`, `[.x.]` and `[=x=]`.
    /// A backslash is a member there.
    fn bracket(&mut self) -> Step {
        if self.peek() == Some('^') {
            self.at += 1;
        }
        if self.peek() == Some(']') {
            self.at += 1;
        }
        loop {
            match self.next() {
                None => return Err("an unterminated bracket expression".into()),
                Some(']') => return Ok(()),
                Some('[') if matches!(self.peek(), Some(':' | '.' | '=')) => {
                    let kind = self.next().unwrap_or(':');
                    loop {
                        match self.next() {
                            None => return Err("an unterminated bracket class".into()),
                            Some(c) if c == kind && self.peek() == Some(']') => {
                                self.at += 1;
                                break;
                            }
                            Some(_) => {}
                        }
                    }
                }
                Some(_) => {}
            }
        }
    }

    /// `s`, the delimiter, the pattern, the replacement, and the flags.
    fn substitute(&mut self) -> Step {
        let delimiter = match self.next() {
            Some(c) if c != '\n' && c != '\\' => c,
            _ => return Err("an `s` command with no delimiter".into()),
        };
        self.delimited(delimiter, true)?;
        self.delimited(delimiter, false)?;
        loop {
            self.blanks();
            match self.peek() {
                Some('g' | 'p' | 'i' | 'I' | 'm' | 'M' | 'e') => self.at += 1,
                Some(c) if c.is_ascii_digit() => self.at += 1,
                Some('w') => {
                    self.at += 1;
                    let name = self.file_name()?;
                    self.note_tail(&name, true);
                    self.writes.insert(name);
                    return Ok(());
                }
                _ => return self.end_of_command(),
            }
        }
    }

    /// `y`, the delimiter, and the two operands.
    fn transliterate(&mut self) -> Step {
        let delimiter = match self.next() {
            Some(c) if c != '\n' && c != '\\' => c,
            _ => return Err("a `y` command with no delimiter".into()),
        };
        self.delimited(delimiter, false)?;
        self.delimited(delimiter, false)?;
        self.end_of_command()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writes(script: &str) -> Vec<String> {
        let reading = read_script(script);
        assert_eq!(reading.unmodelled, None, "{script}");
        reading.writes
    }

    fn none(script: &str) {
        let reading = read_script(script);
        assert_eq!(reading, ScriptReading::default(), "{script}");
    }

    #[test]
    fn a_w_in_an_address_pattern_is_not_a_command() {
        none("/^### sathyassn\\/codeflow#94/,/^### sathyassn\\/codeflow#121/p");
        none("/swap/p");
        none("\\,show/w,p");
        none("\\%a\\%w%p");
        none("$!{/w/d}");
        none("/w/I,/W/Mp");
        none("0,/w/p");
        none("2~3p;/w/,+2p;/w/,~4p");
    }

    #[test]
    fn a_w_in_an_s_command_is_not_a_command() {
        none("s/  - The operator re-trusts w/  - With the operator w /");
        none("s/w/W/g");
        none("s,a\\,w,b\\,w,");
        none("s/[/w]/x/");
        none("s/a/b/gI;s/w/w/2p");
        none("s|w|w\\|w|");
    }

    #[test]
    fn a_w_in_y_a_i_c_or_a_label_is_not_a_command() {
        none("y/w/W/");
        none("y,w\\,,W\\,,");
        none("1a\\\nwrite w here");
        none("1i\\\nwrite w here\\\nand w there");
        none("$c\\\nwritten");
        none("a write notes");
        none("a\\write notes");
        none(":swap\nb swap");
        none(":w\nbw");
        none("/x/{p;b swap}");
        none("r network.txt");
        none("R network.txt");
    }

    #[test]
    fn w_and_capital_w_are_commands_after_an_address_or_a_separator() {
        assert_eq!(writes("w notes.md"), ["notes.md"]);
        assert_eq!(writes("W notes.md"), ["notes.md"]);
        assert_eq!(writes("1w one.md"), ["one.md"]);
        assert_eq!(writes("1,3 ! w three.md"), ["three.md"]);
        assert_eq!(writes("/x/ w x.md"), ["x.md"]);
        assert_eq!(writes("p;w semi.md"), ["semi.md"]);
        assert_eq!(
            writes("p\nw line.md\nW capital.md"),
            ["capital.md", "line.md"]
        );
        assert_eq!(writes("/x/{w block.md\n}"), ["block.md"]);
        assert_eq!(writes("/x/{p;w block.md\n}"), ["block.md"]);
        assert_eq!(writes("$!N;w joined.md"), ["joined.md"]);
        assert_eq!(writes("\\,a,w comma.md"), ["comma.md"]);
        assert_eq!(writes("/a\\/w/w x.md"), ["x.md"]);
    }

    #[test]
    fn the_w_flag_of_s_writes() {
        assert_eq!(writes("s/a/b/w flag.md"), ["flag.md"]);
        assert_eq!(writes("s/a/b/gpw flag.md"), ["flag.md"]);
        assert_eq!(writes("s/w/w/w flag.md"), ["flag.md"]);
        assert_eq!(writes("1,2s/a/b/2w flag.md"), ["flag.md"]);
        assert_eq!(writes("s/a/b/ w flag.md"), ["flag.md"]);
    }

    #[test]
    fn a_file_name_runs_to_the_end_of_its_line() {
        assert_eq!(writes("w notes; p"), ["notes; p"]);
        assert_eq!(writes("w a b.md"), ["a b.md"]);
        assert_eq!(writes("w  \tspaced.md"), ["spaced.md"]);
        let reading = read_script("w notes; p");
        assert_eq!(reading.shorter, ["notes"]);
        let reading = read_script("w notes} p");
        assert_eq!(reading.shorter, ["notes"]);
    }

    #[test]
    fn a_shorter_name_reads_the_commands_after_a_semicolon() {
        let reading = read_script("w a; w b");
        assert_eq!(reading.writes, ["a; w b"]);
        assert!(reading.shorter.iter().any(|name| name == "a"));
        assert!(reading.shorter.iter().any(|name| name == "b"));
    }

    #[test]
    fn text_continues_over_an_escaped_newline() {
        none("1a\\\nline one w\\\nline two w\np");
        assert_eq!(writes("1a\\\nline one w\\\nline two w\nw out"), ["out"]);
        assert_eq!(writes("i\\\ntext\nw out"), ["out"]);
    }

    #[test]
    fn the_delimiter_in_a_bracket_expression_is_read_both_ways() {
        // BSD reads the `/` inside the brackets as part of the expression,
        // GNU as the end of it; one reading parses, and sed rejects the
        // script under the other.
        none("s/[^/]*$//");
        assert_eq!(writes("s/[/]/x/w out"), ["out"]);
        none("/[/]w/p");
        none("s/[]/]w/x/");
        none("s/[[:alpha:]/]w/x/");
    }

    #[test]
    fn blocks_comments_and_numbers_are_read() {
        none("# a comment with w and W\np");
        none("#n\np");
        none("/x/{\np\nd\n}");
        none("q5;Q 3;l 70");
        none("n;N;g;G;h;H;x;z;=");
        assert_eq!(writes("p # w no\nw yes"), ["yes"]);
        none("");
        none("   \n  ;; ");
    }

    #[test]
    fn a_script_neither_reading_parses_falls_back_to_the_text() {
        let reading = read_script("zzz w .codeflow/policy.json");
        assert!(reading.unmodelled.is_some());
        assert!(reading.writes.is_empty());
        assert!(reading
            .candidates()
            .any(|name| name == ".codeflow/policy.json"));
        // Neither reading parses an unterminated `s`.
        let reading = read_script("s/a/b");
        assert!(reading.unmodelled.is_some());
        let reading = read_script("1,p");
        assert!(reading.unmodelled.is_some());
        let reading = read_script("a");
        assert!(reading.unmodelled.is_some());
        let reading = read_script("w");
        assert!(reading.unmodelled.is_some());
    }

    #[test]
    fn an_unmodelled_form_names_no_file_it_does_not_hold() {
        let reading = read_script("Q q");
        assert!(reading.unmodelled.is_some());
        assert!(reading.candidates().next().is_none());
    }

    #[test]
    fn a_gnu_exec_command_names_no_file() {
        none("e echo w");
        none("1e echo w\np");
    }

    #[test]
    fn crude_candidates_cut_at_the_separators() {
        assert_eq!(crude_candidates("a w x;y}z"), ["x;y}z", "x", "x;y"]);
        assert_eq!(crude_candidates("W  spaced"), ["spaced"]);
        assert!(crude_candidates("nothing").is_empty());
    }

    /// TSK-290 AC-3: for each script in the set, the files the `sed` on this
    /// machine creates are the files the reader reports. GNU `sed` runs on
    /// Linux and BSD `sed` on macOS; each script is written in the spelling
    /// both accept, and a script the local `sed` rejects is skipped and
    /// counted.
    #[cfg(unix)]
    #[test]
    fn the_files_native_sed_writes_are_the_files_the_reader_reports() {
        let scripts: &[&str] = &[
            "w w1.txt",
            "W w2.txt",
            "1w a1.txt",
            "/a/w a2.txt",
            "$w a3.txt",
            "1,2w a4.txt",
            "1!w a5.txt",
            "2 ! w a6.txt",
            "s/a/b/w a7.txt",
            "s/a/w/gw a8.txt",
            "s,a,b,w a9.txt",
            "s|w|w|w b1.txt",
            "\\,w,w c1.txt",
            "/w\\/w/w c2.txt",
            "p;w d1.txt",
            "/a/{\nw b2.txt\n}",
            "1{\np\nw b3.txt\n}",
            "w name with spaces.txt",
            "w semi;colon.txt",
            "w brace}.txt",
            "/w/p",
            "s/w/W/",
            "s/a/w/",
            "s/[a-z]/w/",
            "/[w]/p",
            "s/[^w]/W/gp",
            "y/w/W/",
            "y,w,W,",
            "1a\\\nwrite w here",
            "1i\\\nwrite w here\\\nand w there\n1w e1.txt",
            "$c\\\nwritten w",
            "r w.txt",
            "R w.txt",
            "/a/b w\np\n:w",
            "# w comment\n1w f1.txt",
            // sed opens every file it names when it reads the script, so a
            // `w` that no run reaches still creates its file.
            "q;w never.txt",
        ];
        let mut ran = 0;
        let mut skipped = Vec::new();
        for script in scripts {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("in.txt"), "a w\nb\nw\n").unwrap();
            let status = std::process::Command::new("sed")
                .args(["-n", "-e", script, "in.txt"])
                .current_dir(dir.path())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            if !status.success() {
                skipped.push(*script);
                continue;
            }
            ran += 1;
            let mut native: Vec<String> = std::fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name != "in.txt")
                .collect();
            native.sort();
            let reading = read_script(script);
            assert_eq!(reading.unmodelled, None, "{script:?}");
            let mut reported: Vec<String> = reading.writes.clone();
            reported.sort();
            assert_eq!(native, reported, "{script:?}");
        }
        eprintln!(
            "native sed ran {ran} of {} scripts; rejected: {skipped:?}",
            scripts.len()
        );
        assert!(
            ran >= scripts.len() - 6,
            "the local sed ran {ran} scripts and rejected {skipped:?}"
        );
    }
}
