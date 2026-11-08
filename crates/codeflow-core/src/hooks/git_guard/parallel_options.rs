//! How GNU `parallel` reads its own options, so the guards judge the
//! command template, the replacement strings and the input separators that
//! `parallel` itself uses (TSK-326).
//!
//! `parallel` parses its options with Perl's `Getopt::Long`, bundling on and
//! stopping at the first word that is not an option. A long option may be
//! abbreviated to any unique prefix, in any letter case; a value is attached
//! with `=` or is the next word; short options bundle (`-qI %`, `-I%`).

/// GNU `parallel`'s option table, copied from `options_completion_hash` in
/// its source (version 20261022) in `Getopt::Long`'s spec form: the names
/// joined by `|`, then `=s`, `=i` or `=f` for an option that takes a value,
/// or `:s` or `:f` for one whose value is optional.
const SPECS: &[&str] = &[
    "debug|D=s", "xargs", "m", "X", "v", "sql=s", "sql-master|sqlmaster=s",
    "sql-worker|sqlworker=s", "sql-and-worker|sqlandworker=s", "joblog|jl=s",
    "results|result|res=s", "resume", "resume-failed|resumefailed", "retry-failed|retryfailed",
    "silent", "keep-order|keeporder|k", "no-keep-order|nokeeporder|nok|no-k", "group", "g",
    "ungroup|u", "latest-line|latestline|ll",
    "line-buffer|line-buffered|linebuffer|linebuffered|lb", "tmux", "tmux-pane|tmuxpane", "null|0",
    "quote|q", "parens=s", "rpl=s", "plus", "I=s", "extensionreplace|er=s", "U=s",
    "basenamereplace|bnr=s", "dirnamereplace|dnr=s", "basenameextensionreplace|bner=s",
    "seqreplace=s", "slotreplace=s", "delay=s", "ssh-delay|sshdelay=f", "load=s", "noswap",
    "max-line-length-allowed|maxlinelengthallowed", "number-of-cpus|numberofcpus",
    "number-of-sockets|numberofsockets", "number-of-cores|numberofcores",
    "number-of-threads|numberofthreads",
    "use-sockets-instead-of-threads|usesocketsinsteadofthreads",
    "use-cores-instead-of-threads|usecoresinsteadofthreads",
    "use-cpus-instead-of-cores|usecpusinsteadofcores", "shell-quote|shellquote|shell_quote",
    "nice=i", "tag", "tag-string|tagstring=s", "ctag", "ctag-string|ctagstring=s", "color|colour",
    "color-failed|colour-failed|colorfailed|colourfailed|color-fail|colour-fail|colorfail|colourfail|cf",
    "onall", "nonall", "filter-hosts|filterhosts|filter-host", "sshloginfile|slf=s",
    "controlmaster|M", "ssh=s", "transfer-file|transferfile|transfer-files|transferfiles|tf=s",
    "return=s", "trc=s", "transfer", "cleanup", "basefile|bf=s", "template|tmpl=s", "B=s",
    "ctrl-c|ctrlc", "no-ctrl-c|no-ctrlc|noctrlc", "work-dir|workdir|wd=s", "W=s",
    "rsync-opts|rsyncopts=s", "tmpdir|tempdir=s",
    "use-compress-program|compress-program|usecompressprogram|compressprogram=s",
    "use-decompress-program|decompress-program|usedecompressprogram|decompressprogram=s",
    "compress", "open-tty|o", "tty", "T", "H=i", "dry-run|dryrun|dr", "progress", "eta", "bar",
    "total-jobs|totaljobs|total=s", "shuf", "milestone|ms=s", "arg-sep|argsep=s",
    "arg-file-sep|argfilesep=s", "trim=s", "env=s", "recordenv|record-env", "session", "plain",
    "profile|J=s", "tollef", "gnu", "link|xapply", "linkinputsource|xapplyinputsource=i",
    "bibtex|citation", "will-cite|willcite|nn|nonotice|no-notice",
    "halt-on-error|haltonerror|halt=s", "limit=s", "memfree=s", "memsuspend=s", "retries=s",
    "timeout=s", "term-seq|termseq=s", "max-procs|maxprocs|P|jobs|j=s", "delimiter|d=s",
    "max-chars|maxchars|s=s", "arg-file|argfile|a=s", "no-run-if-empty|norunifempty|r",
    "replace|i:s", "E=s", "eof|e:s", "process-slot-var|processslotvar=s", "max-args|maxargs|n=s",
    "max-replace-args|maxreplaceargs|N=s", "col-sep|colsep|C=s", "match=s", "csv", "help|h", "L=s",
    "max-lines|maxlines|l:f", "interactive|p", "verbose|t", "version|V",
    "min-version|minversion=i", "show-limits|showlimits", "exit|x", "semaphore",
    "semaphore-timeout|semaphoretimeout|st=s", "semaphore-name|semaphorename|id=s", "fg", "bg",
    "wait", "shebang|hashbang", "_pipe-means-argfiles", "Y", "skip-first-line|skipfirstline",
    "unsafe", "_bug", "pipe|spreadstdin", "round-robin|roundrobin|round", "recstart=s", "recend=s",
    "regexp|regex", "remove-rec-sep|removerecsep|rrs", "output-as-files|outputasfiles|files",
    "output-as-files0|outputasfiles0|files0", "block-size|blocksize|block=s",
    "block-timeout|blocktimeout|bt=s", "header=s", "cat", "fifo", "pipe-part|pipepart", "tee",
    "shard=s", "bin=s", "group-by|groupby=s", "hgrp|hostgrp|hostgroup|hostgroups", "embed",
    "filter=s", "combineexec|combine-exec|combineexecutable|combine-executable=s", "fast",
    "_parset=s", "_pipe_block=i", "_buf_start=i", "_buf_growth=f", "_buf_cap=i",
    "_no_blocksize_warning", "shell-completion|shellcompletion=s", "sshlogin|S=s",
];

/// What an option takes after its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Takes {
    Nothing,
    Value,
    /// An optional string: the next word unless it is `--` or an option.
    MaybeValue,
    /// An optional number: the next word when it reads as a number.
    MaybeNumber,
}

/// The names of `spec` and what the option takes.
fn parse_spec(spec: &'static str) -> (&'static str, Takes) {
    match spec.find(['=', ':']) {
        Some(at) => {
            let takes = match &spec[at..] {
                ":s" => Takes::MaybeValue,
                ":f" | ":i" => Takes::MaybeNumber,
                _ => Takes::Value,
            };
            (&spec[..at], takes)
        }
        None => (spec, Takes::Nothing),
    }
}

/// The option a long name (after `--`) selects: its first name and what it
/// takes. An exact name wins; otherwise the name must be a prefix of the
/// names of one option only. `None` for an unknown or ambiguous name, which
/// `parallel` refuses.
fn long_option(name: &str) -> Option<(&'static str, Takes)> {
    if name.chars().count() == 1 {
        let letter = name.chars().next()?;
        return short_option(letter);
    }
    let name = name.to_ascii_lowercase();
    let mut hit = None;
    for spec in SPECS {
        let (names, takes) = parse_spec(spec);
        let first = names.split('|').next().unwrap_or(names);
        let mut long = names.split('|').filter(|n| n.chars().count() > 1);
        if long.clone().any(|n| n == name) {
            return Some((first, takes));
        }
        if long.any(|n| n.starts_with(&name)) {
            if hit.is_some_and(|(seen, _)| seen != first) {
                return None;
            }
            hit = Some((first, takes));
        }
    }
    hit
}

/// The option a single letter selects, case sensitive.
fn short_option(letter: char) -> Option<(&'static str, Takes)> {
    SPECS.iter().find_map(|spec| {
        let (names, takes) = parse_spec(spec);
        let first = names.split('|').next().unwrap_or(names);
        names
            .split('|')
            .any(|n| n.chars().eq(std::iter::once(letter)))
            .then_some((first, takes))
    })
}

/// Whether `word` reads as a number to `Getopt::Long`.
fn number(word: &str) -> bool {
    let digits = word.trim_start_matches(['-', '+']);
    digits.chars().any(|c| c.is_ascii_digit())
        && digits
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '_' | 'e' | 'E' | '-' | '+'))
}

/// Which input a separator word starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Separator {
    /// Arguments written on the line (`:::`, `:::+`).
    Words,
    /// Files whose lines are the arguments (`::::`, `::::+`).
    Files,
}

/// A GNU `parallel` call as `parallel` reads its options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParallelCall {
    /// Where the command template starts, after the options and a `--`.
    pub(crate) template: usize,
    /// Whether `-q` or `--quote` is given, so the template's words reach the
    /// command as written rather than through a shell.
    pub(crate) quoted: bool,
    /// Whether an `--arg-file` (`-a`) supplies input.
    pub(crate) from_file: bool,
    /// The replacement string for the whole input when an option sets one
    /// (`-I`, `-i`, `--replace`); `{}` otherwise.
    pub(crate) whole: Option<String>,
    /// Other replacement strings an option sets: a part of the input
    /// (`--extensionreplace` and its siblings), an `--rpl` shorthand, or the
    /// opening of a `--parens` expression.
    pub(crate) derived: Vec<String>,
    /// Whether `--plus` or `--rpl` adds replacement strings of any shape.
    pub(crate) wide: bool,
    arg_sep: String,
    file_sep: String,
}

impl ParallelCall {
    /// Reads `args`, the words after `parallel`.
    pub(crate) fn read(args: &[String]) -> Self {
        let mut call = Self {
            template: 0,
            quoted: false,
            from_file: false,
            whole: None,
            derived: Vec::new(),
            wide: false,
            arg_sep: ":::".into(),
            file_sep: "::::".into(),
        };
        let mut at = 0;
        while let Some(word) = args.get(at) {
            if word == "--" {
                at += 1;
                break;
            }
            if !word.starts_with('-') || word.len() < 2 {
                break;
            }
            at += 1;
            if let Some(long) = word.strip_prefix("--") {
                let (name, attached) = long
                    .split_once('=')
                    .map_or((long, None), |(n, v)| (n, Some(v)));
                let Some((option, takes)) = long_option(name) else {
                    continue;
                };
                let value = match (takes, attached) {
                    (Takes::Nothing, _) => None,
                    (_, Some(value)) => Some(value.to_string()),
                    (takes, None) => next_value(args, &mut at, takes),
                };
                call.apply(option, value.as_deref());
                continue;
            }
            let cluster = &word[1..];
            for (offset, letter) in cluster.char_indices() {
                let Some((option, takes)) = short_option(letter) else {
                    break;
                };
                if takes == Takes::Nothing {
                    call.apply(option, None);
                    continue;
                }
                let rest = &cluster[offset + letter.len_utf8()..];
                let value = if rest.is_empty() {
                    next_value(args, &mut at, takes)
                } else {
                    Some(rest.to_string())
                };
                call.apply(option, value.as_deref());
                break;
            }
        }
        call.template = at.min(args.len());
        call
    }

    fn apply(&mut self, option: &str, value: Option<&str>) {
        let value = value.unwrap_or_default();
        match option {
            "quote" => self.quoted = true,
            "arg-file" => self.from_file = true,
            "arg-sep" => self.arg_sep = value.to_string(),
            "arg-file-sep" => self.file_sep = value.to_string(),
            "I" | "replace" if !value.is_empty() => self.whole = Some(value.to_string()),
            "extensionreplace"
            | "basenamereplace"
            | "dirnamereplace"
            | "basenameextensionreplace"
            | "seqreplace"
            | "slotreplace"
                if !value.is_empty() =>
            {
                self.derived.push(value.to_string());
            }
            "rpl" => {
                self.wide = true;
                let shorthand = value.split(char::is_whitespace).next().unwrap_or_default();
                if !shorthand.is_empty() {
                    self.derived.push(shorthand.to_string());
                }
            }
            "parens" => {
                let half: String = value.chars().take(value.chars().count() / 2).collect();
                if !half.is_empty() {
                    self.derived.push(half);
                }
            }
            "plus" => self.wide = true,
            _ => {}
        }
    }

    /// Which input `word` starts, when it is one of this call's separators.
    pub(crate) fn separator(&self, word: &str) -> Option<Separator> {
        let base = word.strip_suffix('+');
        if word == self.file_sep || base == Some(self.file_sep.as_str()) {
            Some(Separator::Files)
        } else if word == self.arg_sep || base == Some(self.arg_sep.as_str()) {
            Some(Separator::Words)
        } else {
            None
        }
    }

    /// The command template in `args`, as its start and end: from
    /// [`Self::template`] to the first separator.
    pub(crate) fn command(&self, args: &[String]) -> (usize, usize) {
        let start = self.template.min(args.len());
        let end = (start..args.len())
            .find(|at| self.separator(&args[*at]).is_some())
            .unwrap_or(args.len());
        (start, end)
    }
}

/// The value of an option that takes one and has none attached, read from
/// the next word.
fn next_value(args: &[String], at: &mut usize, takes: Takes) -> Option<String> {
    let next = args.get(*at)?;
    let take = match takes {
        Takes::Value => true,
        Takes::MaybeValue => next != "--" && !(next.starts_with('-') && next.len() > 1),
        Takes::MaybeNumber => number(next),
        Takes::Nothing => false,
    };
    take.then(|| {
        *at += 1;
        next.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(line: &str) -> ParallelCall {
        let words: Vec<String> = line.split(' ').map(str::to_string).collect();
        ParallelCall::read(&words)
    }

    #[test]
    fn options_are_read_as_parallel_reads_them() {
        let call = read("-qI % sh");
        assert!(call.quoted);
        assert_eq!(call.whole.as_deref(), Some("%"));
        assert_eq!(call.template, 2);
        assert_eq!(read("-I% sh").whole.as_deref(), Some("%"));
        assert_eq!(read("--repl=% sh").whole.as_deref(), Some("%"));
        assert_eq!(read("--replace % sh").template, 2);
        assert_eq!(read("--replace -q sh").template, 2);
        assert_eq!(read("-i sh").whole.as_deref(), Some("sh"));
        assert_eq!(
            read("--arg-s %% sh").separator("%%"),
            Some(Separator::Words)
        );
        assert_eq!(read("--argsep=%% sh").separator(":::"), None);
        assert_eq!(
            read("--argfilesep @@ sh").separator("@@+"),
            Some(Separator::Files)
        );
        assert_eq!(read("--delim , sh").template, 2);
        // `--del` names `--delay` and `--delimiter`, which `parallel` refuses.
        assert_eq!(read("--del , sh").template, 1);
        assert_eq!(read("--tag sh").template, 1);
        assert_eq!(read("--tagstring x sh").template, 2);
        assert_eq!(read("-j4 -- sh").template, 2);
        assert!(read("-a refs sh").from_file);
        assert_eq!(read("--bnr % sh").derived, ["%"]);
        assert_eq!(read("--parens ,,.. sh").derived, [",,"]);
        assert!(read("--plus sh").wide);
        // `--re` names several options, which `parallel` refuses.
        assert_eq!(read("--re x sh").template, 1);
    }
}
