### Fixed

<!-- codeflow:release-impact patch -->
- **exec-guard no longer refuses prose that only quotes a launcher word.**
  The privilege, headless and dangerous checks matched a privileged word as a
  substring of the raw line, so text such as "; supersedes", "; su" or "such
  as" inside a heredoc, an `echo` or a `grep` pattern was blocked as a chained
  `su`, `sudo` or `doas` (sathyassn/codeflow#66). A word-boundary edit to
  those checks cannot be proved safe, because a shell reaches a launcher
  through globs, extglob, zsh qualifiers and namerefs without its name
  appearing as a word, so every 3.0.0 rule is unchanged and still decides
  every line. Relief comes from a separate strict tokenizer that certifies a
  line only when all of it is `echo`, `printf` (a format of `%s`, `%%`, `\n`, `\t` and `\\` only),
  `grep` and `cat` with plain, single-quoted or double-quoted words (no
  backslash, `$` or backtick), the separators `;`, `&&`, `||`, `|` and
  newline, redirects to a document file (`.md`, `.markdown`, `.txt`, `.rst`,
  `.log`) that is new or an existing plain, non-executable file with one
  hard link (never a symbolic link, hard-linked file, pipe, device or
  executable, or a path under `/dev`, `/proc` or `/sys`), and one quoted
  heredoc for `cat`. A
  certified line skips only the privilege, headless and dangerous checks;
  every other guard still runs, PowerShell lines are never certified, and a
  `run_terminal_command` call is certified only on a Unix host whose `SHELL`
  names bash or zsh. The
  issue's `printf`, `grep '; su'`, quoted-heredoc and `echo "...; supersedes
  ..."` lines now pass. A heredoc needs a quoted delimiter (`cat <<'EOF'`);
  with an unquoted one the line keeps the raw rules. Lines with a
  double-quoted backslash, and `git commit -m` or `gh pr create --body` text,
  are not certified and still refuse: write that text to a file with the
  editor tool and pass the file by path. Two limits remain: a definition in
  a shell startup file that changes how a certified line runs, such as an
  alias or function shadowing `echo`, `printf`, `grep` or `cat` or a zsh
  global alias that expands an argument, is not seen, as an alias for `ls`
  was not seen before (agents are blocked from writing startup files where a
  sandbox exists and the guard refuses it on a best-effort basis elsewhere,
  issue 86); and a certified line can create a document that a later call runs
  as a script, as a plain `printf` into a `.sh` file already could. Projects
  need no change: this is a binary change with no policy default, scaffold or
  managed file behind it, and it allows certified prose without refusing
  anything allowed before.
