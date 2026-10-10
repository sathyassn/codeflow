### Fixed

<!-- codeflow:release-impact patch -->
- **The git guard judges what `find` and `xargs` run, and protects live
  worktrees.** `find -exec`, `-execdir` and `-delete` and `xargs` could
  edit or delete the enforcement files. As new hardening that keeps every
  refusal 3.0.0 made, a `sed`, `find`, `xargs` or `parallel` that can
  change files is now refused when its command line names an enforcement
  path anywhere, including a `sh -c` string or a producer piped into
  `xargs`, in any spelling the file system reads as one (`//`, `/./`,
  another case, or a glob that matches it). Each command, a pipeline
  member or a `sh -c` body included, is judged from every directory a
  literal `cd`, `pushd`, `env -C` or `env --chdir` on its line can move it
  to, and a glob is expanded from there with each match judged through
  symbolic links and registered worktrees, so `alias/pol*` with `alias`
  linked to `.codeflow` is refused. Patterns are read so they match at
  least every name the shell would: a plain set such as `[ab]` keeps its
  members, any other bracket expression, POSIX classes and escapes
  included, makes that part of the path match every name, a backslash
  outside brackets makes the next character literal, and only a `[` with
  no `]` after it is literal; a randomized test checks this against
  Bash. Brace expansion is read before patterns, under every reading a
  quote or escape allows, up to 64 words, and a larger one is refused;
  a word with syntax the guard reads conservatively (`(` or `)`, zsh glob
  qualifiers such as `(D)` included, `^`, `#`, a zsh range `<n-m>` or
  `**`) matches every path below its longest literal directory, at every
  depth, names that start with `.` included, while folder names above the
  word, such as a Windows short name `RUNNER~1`, stay literal; a `~` after
  the first character is read as zsh's exclusion, by the part before it;
  parentheses attached to a word or after a command word are part of the
  word, and the text inside them is also judged as commands, as Bash runs
  `if(rm ...)`, and so is the code of a zsh `e` or `+` qualifier, each
  group read once per nesting level, with text nested more than 8 levels
  deep refused; a word with a
  part filled in at run time is read by the names after that part, and a
  value assigned on the same line counts; `~+` is the current directory
  and other tilde prefixes are read by name; a line that turns on
  `dotglob`, `GLOBIGNORE` or zsh `globdots` refuses a writing command
  with a pattern. Two more randomized tests run whole command words
  through the guard as direct targets, redirect targets and `xargs`
  input, and compare them with the real expansion of Bash and of zsh. A `cd` or `pushd` operand other than a
  plain literal path, such as `~1`, `cd -` or a pattern, counts as an
  unknown directory, and a redirection counts as a read only when it is
  `<`, a heredoc, a here-string or a descriptor copy, so `1<>` and
  `{fd}>` writes are judged, after line continuations are joined. On a
  command with `$'...'` or `$"..."` quoting and a `>`, any word that
  could name an enforcement path is refused as a possible write target.
  Where a directory is filled in at run
  time, or a stack rotation or `popd` can reach a directory `pushd -n`
  stacked, a writing command or write redirect whose words could name an
  enforcement path by their names alone, such as `policy.json` or `pol*`,
  is refused, while a
  command proven to only read passes: a plain `sed` read, a `find` that
  changes nothing, or `xargs` running a read-only program. The guard
  follows at most 64 such directories per line and treats more as
  unknown. One
  expansion reads at most 4,096 directory entries; past that,
  the directory it starts from decides, so a glob over a large build tree
  passes and one over a tree holding enforcement files is refused.
  Launchers such as `nice`, `timeout`, `stdbuf` and `env --unset` no longer
  hide the command, and a launcher option the guard cannot read is
  refused.
  `find` actions are also judged on each protected path they can reach,
  in expression order and from each match's own directory for
  `-execdir`. A recursive `rm`, or a `chmod` or `chown`, of a directory
  holding enforcement files is refused from any checkout. A recursive
  `rm`, `trash`, `find -delete` or `git clean -ff` (through git's global
  options, abbreviations and aliases) of a registered worktree, of a
  directory holding one such as `.worktrees` or `.claude/worktrees`, or
  of a target the guard cannot resolve in a checkout that holds
  worktrees, is refused with `git worktree remove` as the way to remove
  it. A path built at run time, which no argument spells, is past the
  guard; in Claude sessions the sandbox's write denies are the backstop.
