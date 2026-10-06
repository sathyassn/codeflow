# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

An undated version section above the latest verified public release is pending
source state, not a claim that the version is available. The public release
records the actual publication date; published sections and their impact
annotations are frozen. A correction to a published section is a dated
erratum below, never an edit of the section.

## Errata

- 2026-09-27, 2.1.0: the `v2.1.0` tag and the published `source.tar.gz`
  identify different commits. The published archive is the release's source;
  the tag stays where it is. See "Public version baseline" in
  `docs/releasing.md`.

## [3.1.0]

### Added

<!-- codeflow:release-impact minor -->
- **One planning amendment can span several epics.** A planning pull
  request names every epic it changes on its one `Task:` line, such as
  `Task: EPC-002, EPC-003`, so one reviewed plan change lands as one pull
  request instead of one per epic (ADR-0078). It may also carry files under
  `docs/` outside the adopter-facing paths and the project section of
  `AGENTS.md`, as long as the managed block stays byte-identical to the
  target's; a byte inside that block is refused by name, and product code,
  `CLAUDE.md`, harness settings, skills, policy, hooks and CI still keep a
  range out of the planning class, as do a hidden path, an instruction
  file, a symbolic link or a submodule entry in any folder, judged
  against the target's policy; docs and `AGENTS.md` ride only in a
  repository with no symbolic link or submodule. A release brings such an amendment's
  criteria change as a planning landing. `codeflow ci` prints the class as a
  planning-only amendment of the named epics and adds a
  `work.planning_amendment` note per change, grouped by epic: each task's
  criteria delta, records added or removed, status changes, the doc and
  instruction files touched, and a task record that changed on its
  integration line since the line last merged the target. A change to a
  record of an epic the line does not name is refused, a criteria change
  to a complete task is flagged, and a standalone task or a spec is
  listed. A task pull request still changes only its own
  criteria, and the frozen message now names the planning amendment as the
  route for another task's criteria. There is no policy key.
  Migration: CodeFlow 3.0.0 reads `Task: EPC-001, EPC-002` as a malformed
  `Task:` line and refuses the pull request, so run 3.1.0 locally and in
  the CI that judges a multi-epic amendment; a single `Task: EPC-NNN` works
  on both. A planning pull request that names one epic but changes another
  epic's records, such as a breakdown that creates two epics, now fails
  until its `Task:` line names both.

<!-- codeflow:release-impact minor -->
- **Pull request Summaries open with a prose lead, then bullets.**
  `codeflow ci` now checks the shape of a pull request body's Summary under
  a new policy key, `git.pr_summary`, which blocks by default: one prose
  paragraph that anchors the reader, then the details as a list or a table,
  then at most one closing paragraph. Only visible blocks count, so text in
  an HTML comment never supplies the lead or the list, and a heading, code
  block, quote or HTML block in the Summary fails. It judges shape, never a
  word or sentence count (ADR-0071, note of 2026-10-03). It runs at warn
  while a kept PR template is diagnosed, a trusted automation profile skips
  it, and a project lowers it by setting `git.pr_summary` to `warn` or
  `off`. The PR template, `writing.md` "Summaries" and cf-ship's PR
  evidence reference teach the shape. The shipped policy file does not list
  the key, so neither `init` nor `update` writes it and an older binary
  never meets it; a project that sets it runs 3.1.0 or later locally and in
  CI.
  cf-ship also says that a pull request already reported ready goes back
  to draft before any further change to its branch, and its release
  integration steps move to their own reference, read only after an
  epic-line landing with a configured release branch.

<!-- codeflow:release-impact minor -->
- **`codeflow ci` warns when a pull request body is too long.** A body over
  1,000 words, counted as a reader sees it (HTML comments left out, fenced
  blocks and tables counted), draws one warning that names the count, the
  limit and the three largest `##` sections, and asks for the body to be
  rewritten to its final state with records linked instead of copied. It
  joins the existing presentation warnings under `git.pr_sections`: no new
  policy key, advisory at any level, and never a blocking finding. It runs
  wherever `codeflow ci` is given a body: a pull request event in hosted CI,
  `CODEFLOW_PR_BODY`, `--pr-body` or `--pr-body-file`. The pre-push hook
  passes no body, so it runs no body check. cf-ship's PR evidence reference
  and the PR template say so.

<!-- codeflow:release-impact minor -->
- **`codeflow test --only` runs a gate in parts.** `--only <targets>` runs
  the named targets and their prerequisites, comma separated or repeated,
  so one gate can be split across parallel CI jobs. A limited run is
  recorded as not complete, so it never serves as a green base for
  `--since`, and a name that is not an enabled target of the mode is
  refused before any target starts. With `--all`, the named targets keep
  the full-strength checks of the epic close.

<!-- codeflow:release-impact patch -->
- **Native Windows support returns.** The release publishes an x86-64
  Windows archive with `codeflow.exe` and a PowerShell installer again,
  beside the macOS and Linux builds; 3.0.0 published neither. The Windows
  jobs gate every pull request to `main` and every publication again,
  behind one `windows` check.
  The defects it found are fixed: the guards, `codeflow doctor` and the
  Codex and Grok trust checks now treat the short (`RUNNER~1`), long and
  `\\?\` spellings of one Windows path as the same file; the full-gate lock
  can be read by a second gate on Windows; a directory in the way of a
  ledger file is named as a directory there; a shipped spec checked out
  with CRLF line endings no longer counts as edited; and the model
  evaluation kit keeps its signing key owner-only through the key's and
  its folder's access lists, since Windows has no POSIX mode bits: it
  writes the key only once both lists are proven private and refuses
  either list that lets in another account.
  On Windows the guards refuse a recursive delete below any `/`-rooted
  path, such as `rm -r /tmp/scratch`: that path names no fixed place
  there, and a junction can send the delete anywhere. A relative path is
  judged as before.

<!-- codeflow:release-impact minor -->
- **`codeflow doctor --check grok` checks that Grok can run the CodeFlow
  guards.** It warns, naming each file, about a CodeFlow hook command
  Grok would skip because it carries a `$`, with the step that fixes it:
  `codeflow update` where update's own steps rewrite the file or write a
  `.new` merge beside it, and a hand edit for any other file, with the
  reason update leaves it: a file update does not manage, such as
  `.claude/settings.local.json`, a file it skips as a symlink or through
  `[scaffold] ignore`, or an edit it keeps because the shipped version
  has not changed. When both kinds of file are stale, it gives both
  steps. When no exec-guard is bound at all, it offers `codeflow update`
  only where update would bind the shipped guard again, and otherwise
  quotes the shipped guard group to add to `.grok/hooks/codeflow.json`.
  It names a `.new` file update left waiting. When the
  shipped exec-guard handler (its command, timeout and environment) is
  bound where Grok's shell tool hits it, matched as Grok matches, doctor
  judges a fixed canary dangerous command in the payload Grok sends with
  the handler `codeflow hook exec-guard --contract 3` runs, in its own
  process under the catastrophic-command floor alone, reading no policy,
  repository, working directory or environment and recording no
  refusal, and warns unless it refuses with exit 2, a reason and Grok's
  deny answer. Doctor executes nothing for the check, so no hook text,
  hook environment, `codeflow` found on PATH or swapped binary can answer
  for it: a customised handler is reported as unverified, and where PATH
  resolves `codeflow` is reported, flagged when it lies inside the
  repository or is not the binary doctor started from. The canary does
  not exercise a shell, the command-line parsing, the `codeflow` on PATH
  or Grok's own hook call; a live session's hook lines prove those.

<!-- codeflow:release-impact minor -->
- **Task records name their deliverables and where they go.** The task
  template has a `## Deliverables` section after Description: each output
  (files or a folder, a decision record, a research note, evidence, a
  record update, a human board) and its home as a path in the project's
  structure, or a provisional home with what decides it. The epic
  template's "Affected surfaces and interfaces" asks for the homes the
  epic's tasks write, or a pointer to the project's structure authority,
  and the `cf-method` clarity checklist that `cf-plan` applies checks every
  task's deliverables and homes against that authority before records are
  materialized. `codeflow validate --docs` warns about an open task with
  no filled section and no path in its Description; the warning never
  blocks, has no policy key, and never reads a complete or cancelled
  record. It errs toward silence: anything that plausibly names a path,
  Windows paths and `README` included, satisfies it. After `codeflow update`, an adopter's
  `project-management/templates/` carries the new section.

<!-- codeflow:release-impact minor -->
- **Reported defects are fixed by cause and class, with a written critical
  path.** A new `cf-method` reference, `issue-handling.md`, takes a reported
  defect from intake to closure: reproduce it and judge its severity, name
  the cause and the defect class, search the tree for every site of the
  class, group issues that share a cause into one unit with a design first
  for guards, parsers, policy, acceptance rules, hooks and CI, fix the whole
  class with a durable check where one can be written, and close the issue
  with the sites fixed and deferred and the release. A review round that
  finds a new instance of the same class stops the rounds and sends the
  unit back to design. A critical defect (live in a release or blocking
  current work, and blocking adopters, weakening a security boundary,
  losing data or hanging a gate) gets interim guidance the same day, a
  prioritized fix when that guidance does not clear the block, a recorded
  release decision and a notice to affected adopters; moving another
  epic's planned work for it is the operator's call. The lifecycle reference's repair bullet points a
  reported defect at it, and `cf-reviewer` checks that the fix covers the
  class and that the sweep is recorded. It adds no check, pull request,
  approval or review round. Standard and full tiers receive it with
  `codeflow update`; nothing else needs to change. For CodeFlow itself,
  `docs/releasing.md` "Critical issues" states the routes and why releases
  stay on `main` with no maintenance branch, and the bug report template
  asks for a severity.

### Changed

<!-- codeflow:release-impact minor -->
- **The managed model roster adds Claude Sonnet 5.5 and adopts GPT-6.1 Sol.**
  The primary seats do not change: the Claude primary seat is Opus 5.5, then
  Fable 5.1, and the Codex primary seat is GPT-6 Astra, then the Sol line.
  After `codeflow update`, the catalog has a `sonnet` worker line
  (`claude-sonnet-5-5`, medium and high effort) that holds no seat. Among
  workers it is tried before Opus for bounded execution, evidence
  collection and the Claude fallback in engineering implementation, and it
  never serves orchestration, planning, design or review. The Sol line
  adopts GPT-6.1 Sol (`gpt-6.1-sol`), designated for `codex-primary` on
  2026-10-02; GPT-6 Sol and GPT-5.6 Sol stay as its fallback versions.
  Orchestration, technical planning and review stay with the Claude
  primary seat, Opus first and Fable second. Design stays with Opus alone:
  Fable takes design only under a task's operator override, so without
  Opus the design duty stays open. Fable comes first for consultation and
  reasoning support.
  No new version has a qualification record yet, so `codeflow doctor
  --check model-bindings` also warns for GPT-6.1 Sol.

<!-- codeflow:release-impact minor -->
- **One rule for how one model family calls another.** The skills and
  managed instructions gave hosts different answers (issues 30 and 31).
  After `codeflow update`, the rule lives in one file,
  `cf-model-orchestrator/resources/routing/transport.md`, and every other
  skill, the CLAUDE.md and AGENTS.md rows and the exec-guard refusal cite
  it (ADR-0077). Another family runs as its own interactive CLI in a Herdr
  tab: Codex on its app-server, Claude with its turns tracked by
  `codeflow delegate`, Grok through Grok Build. The same family runs as
  native subagents, and print or exec modes stay refused. The official
  Codex plugin becomes an optional fallback on a Claude Code host, and
  tmux the last fallback when no Herdr server is reachable. `cf-herdr` now
  drives any reachable Herdr server from any host, inside a Herdr pane or
  not, with its anti-hijack rules unchanged. The seats' launch flags are
  stated once, so an edit handoff to Claude now launches in the production
  posture instead of auto mode, and a new seat's first-run prompts are
  named: the caller answers folder trust for the task's own folder, the
  operator answers every hook trust prompt (Grok's included), and a
  self-update offer is skipped. A Grok builder seat is marked not qualified
  until ADR-0075 D3's sandboxed route is proven, so building goes to a
  Claude or Codex seat. A long
  Codex reply and its observed model and effort are read from the seat's
  session record. Review and consult briefs ask for one holistic pass over
  the whole unit and its blast radius, earlier findings being checks within
  it, and `cf-herdr` states how a review seat runs. `codeflow doctor
  --check delegates` no longer warns about a missing Codex plugin and now
  warns when `herdr` is missing, naming tmux as the fallback. Projects that
  relied on the plugin keep it as the fallback; to use the default route,
  install Herdr. Where a project customised these skills and its edits
  overlap the new text, `codeflow update` leaves a `.new` proposal beside
  the file; reconcile it so the project's routing prose agrees. "Any host"
  adds no native Windows support: delegate state there still needs WSL2.

<!-- codeflow:release-impact minor -->
- **Claude sessions compact at half the context window by default.**
  `codeflow init` now writes `CLAUDE_CODE_AUTO_COMPACT_WINDOW` = `"1000000"`
  and `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` = `"50"` into the `env` of
  `.claude/settings.json` at every tier and permission preset, and
  `codeflow update` adds them to existing projects. A session on a model
  with a native 1M window then compacts at about 500K tokens instead of
  about 967K, and a session limited to 200K compacts at about 100K instead
  of at the 200K boundary. The status line's `used_percentage` then
  measures against the full model window and no longer shows when
  compaction runs. To opt out, delete the two keys or the whole `env`
  object; to use other values, edit them. Update keeps a value the project
  changed and does not restore a key the project deleted after CodeFlow
  wrote it. A project with no recorded baseline and its own `env` gets no
  keys added; the update report names them. The settings of delegated runs
  do not change.

<!-- codeflow:release-impact patch -->
- **The docs say plainly what CodeFlow is, and read in one order.**
  `docs/product.md` now states what CodeFlow is, the problem it solves, who
  it is for, what it does and what it is not, with a figure of the four
  enforcement planes. The README opens from it and adds a table of the three
  init tiers: what each installs and when to pick it. The docs are listed
  in the order understand, start, use, configure, reference and maintain,
  in the README and in the portal navigation, whose routes follow the new
  groups (`understand/`, `start/`, `use/`, `configure/`, `reference/`,
  `maintain/`). The other guides were swept for mannered prose and long
  blocks, and gained text flows where a flow carries the point. No command,
  flag, policy key or behavior changes. Links from the decision and task
  records to `docs/verification/` now point at the copy on `main`, because
  the source archive leaves that folder out. A workflow, `portal-pages.yml`,
  builds the portal and deploys it to GitHub Pages once the repository owner
  enables Pages; the portal `base` is now `/codeflow/`.

### Fixed

<!-- codeflow:release-impact patch -->
- **Record commands no longer write through a symbolic link above the
  record** (issue 94). A branch that committed `project-management`, a
  kind folder, `docs` or `docs/decisions` as a link made `epic new`,
  `task new`, `spec new`, `adr new`, `task status` and their siblings
  write, create folders or delete files where the link pointed. Every
  record write, delete and folder creation under `project-management/`
  and `docs/` now opens each path component from the repository root
  without following a link, junction or other reparse point, and refuses
  with the linked component named; this covers the status verbs,
  follow-ups, `ids backfill` and `ids retarget`, and the folders that
  `codeflow status` and the orient digest create, which now say why the
  work summary is missing. A link at the old `<record>.tmp` name is no
  longer written through. Other visible changes: a replaced record keeps
  its Unix permission bits and is synced to disk; `ids retarget` refuses a
  file already at the new number before it changes anything; `codeflow
  status` and the orient digest name a linked records folder instead of
  reading through it. Migration: none.

<!-- codeflow:release-impact patch -->
- **`codeflow init` no longer hangs on a full pipe.** In a repository with
  enough folders, `codeflow init` could block forever: it wrote all of the
  folder names to `git check-ignore -v -n --stdin -z` before reading any
  answer, while git wrote an answer per name, so once the output pipe
  filled each side waited for the other. The input is now written from
  its own thread while the output is read, through one module that is the
  only place codeflow pipes a child's stdin: the nested-repository scan,
  the `codeflow doctor` probes that pass input, the `gh` calls that pass a
  request body, the ID registry, the pre-push and CI git reads, the
  conflict-marker attribute check and the portal's git reads. Output of
  any size completes, and a child that stops reading early is judged by
  its exit status. A test that parses the Rust sources fails when new code names a child's
  stdin outside that module; code inside macro invocations and raw file
  descriptors are not covered.

<!-- codeflow:release-impact patch -->
- **Model evaluation trials launch Codex and Grok 1.0.46 again.** A
  signed-in Codex 0.159.1 synced the account's installed remote plugins
  into the dedicated evaluator home at every start, and one of them holds a
  folder named `hooks`, so every Codex launch with the hook-trust bypass
  refused, Codex peers in Claude trials included. `cf-evaluate-model`'s
  `eval_kit.py prepare-eval-homes` now writes `plugins = false` and
  `remote_plugin = false` under `[features]` in the dedicated Codex
  config, adding only what is missing, and moves a stale remote plugin
  cache out of that home into `~/.codeflow-eval/removed-remote-plugins/`,
  printing each move and why; every Codex start the kit builds passes the
  same two settings. Run it again once after updating. The repository's
  qualification runner refuses any launch whose dedicated Codex home lacks
  the settings or holds that cache, naming `prepare-eval-homes`, and checks
  again after readiness and for each Codex peer. The plugin check still
  refuses any folder named `hooks`. The runner also recognises Grok Build
  1.0.46's trust dialog beside 1.0.44, records the version, and refuses any
  other version or screen with a message naming both, keeping "evaluator
  home not signed in" for a visible sign-in screen.

<!-- codeflow:release-impact patch -->
- **The full gate runs inside CodeFlow's own Claude sandbox.** The full
  gate takes a machine-wide lock under `~/.codeflow/locks` and keeps its
  evidence under `~/.codeflow/gate-runs`, which the shipped Claude settings
  presets did not let a sandboxed command write, so every full gate in a
  sandboxed session refused with `gate lock unavailable`. The presets now
  allow writes to those two directories and nothing else in the CodeFlow
  home, and `codeflow update` adds them to existing settings. Two full
  gates still never run at once on one machine. The refusal now points at
  `codeflow doctor --check permissions`, which names each gate directory
  this process cannot write. When `SANDBOX_RUNTIME` is set, doctor notes
  it, probes the network over HTTPS instead of a DNS lookup the sandbox
  cannot make and passes only on an HTTP 2xx or 3xx answer, and reports a
  failed Codex sign-in probe as unconfirmed, quoting its error, unless the
  probe says you are signed out.

<!-- codeflow:release-impact patch -->
- **An in-place `sed` on macOS is no longer read as an edit of the
  enforcement files.** In a worktree under `.claude/worktrees/`, the git
  guard refused `sed -i '' ...` on any file, and an empty operand of `rm`
  and the other write commands, as an edit of the repository's enforcement
  files. It now reads GNU and BSD sed's own option grammars (BSD `-i` and
  `-I` take a separate backup suffix, `-l` is a flag), never treats an
  empty argument as a path, and judges a worktree nested in the main
  checkout's `.claude/` by its own files. Writes to the worktree's own
  `.claude/settings.json` or `.codeflow/policy.json` are still refused,
  and so is any `sed` whose script, options or `-f` script file names an
  enforcement path; a plain read such as `sed -n p <file>` passes.

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

<!-- codeflow:release-impact patch -->
- **The git guard refuses a forced move of a protected branch.**
  `git branch -f main HEAD~3` passed the guard, and the
  reference-transaction hook lets a rewind behind the remote through. The
  guard now refuses `git branch -f`, `-M` and `-C`, `git checkout -B`,
  `git switch -C` and `git worktree add -B` aimed at a protected branch
  under `git.local_ref_protection`, as it already refused `git
  update-ref`. It reads flag clusters such as `-fv` and abbreviations
  such as `--force-c`, resolves `@{-1}` and `@{upstream}` in the target
  repository, and refuses a forced move it cannot resolve, or one whose
  expression an earlier git command on the same line may change.

<!-- codeflow:release-impact patch -->
- **`codeflow present show` and `close` no longer fail on Linux because of an
  unrelated process.** The scan for the presentation browser read the command
  line of every process of the same user and refused the whole scan with
  "browser identity is not UTF-8" when one was not UTF-8, or with an
  "exceeded its bound" error when one was over 64 KiB, so a single such
  process anywhere on the machine broke `present show`, `present close` and
  the recovery of an interrupted launch (issue 60, seen in hosted CI). The
  scan now reads each command line once, in fixed memory, and skips a process
  that lacks an argument equal to the profile argument or one equal to the
  instance argument of the browser it is looking for, whatever its other
  bytes are. A process that has both is checked as strictly as before, and a
  command line over 8 MiB, which the kernel does not allow a new process,
  still fails the scan. macOS lists processes with `ps` and already
  tolerated such lines.

<!-- codeflow:release-impact patch -->
- **The CodeFlow guards run in Grok sessions.** Grok expands `$name` and
  `${...}` in a hook command itself and skips the hook, letting the tool
  call through, when a name is unset. Every CodeFlow hook command in 3.0.0
  carried shell variables, so in a Grok session git-guard, exec-guard,
  edit-guard and session-orient never ran, and doctor reported only folder
  trust. After `codeflow update`, the hook commands in
  `.grok/hooks/codeflow.json`, `.claude/settings.json` (which Grok also
  reads) and `.codex/hooks.json` carry no `$`: each runs
  `codeflow hook <name> --contract 3` and exits 2 with a reason whenever
  the hook fails, since Codex lets a call through on exit 2 with no
  reason, naming the installer and `codeflow update` when the binary is
  missing. A
  binary older than 3.0.0 still blocks, now with its own usage error in
  place of the install line. Grok 1.0.46 also sends each payload field
  under both spellings (`toolName` and `tool_name`), which the guards took
  for an unreadable payload and allowed; they now read it, and a payload
  whose two spellings disagree is still reported as unreadable. Grok shows
  only the first line of a hook's error output as the reason it denied a
  call, so a guard refusing a Grok call also returns Grok's deny decision
  with the whole refusal, the rule and its sanctioned path included.
  `codeflow update` keeps your own edits to these files. It merges Claude
  settings by their hook entries, and merges a Grok or Codex hook file
  you edited by a 3-way merge that applies on its own when your edit does
  not overlap the new commands. When it overlaps, update leaves the file
  as it is and writes a `.new` file beside it holding the merge: resolve
  its conflict markers in favour of the shipped CodeFlow hook commands,
  replace your file with it and delete the `.new` file. Update never
  touches a hook file it does not manage, such as
  `.claude/settings.local.json`, which Grok also reads, nor one that is a
  symlink, and it keeps an edit when the shipped version has not changed:
  in such a file, replace any CodeFlow hook command that carries a `$`
  with the shipped one, as `codeflow doctor --check grok` names.
  Then run `codeflow doctor --check grok` and confirm that a live Grok
  session refuses a dangerous shell command.

<!-- codeflow:release-impact patch -->
- **The pre-push hook judges a push by the default branch's policy.**
  `codeflow ci` and the pre-push hook judged commit, branch and PR-body
  standards with the branch's own `.codeflow/policy.json`, while the hosted
  policy job reads the policy of the target tip it checks out. A branch
  that loosened its own rules, such as more commit-body bullets, passed
  locally and failed after the push. The pre-push hook now judges every
  pushed branch with the policy at the destination default branch's
  advertised tip, fetched at most once per push when this clone lacks it.
  That policy is a candidate destination authority, not the known pull
  request target, and hosted CI stays the enforcement: no other branch is
  an authority, even a protected one, so a pull request into an
  integration line is judged locally by the default branch's policy and
  the host may judge it differently. The policy is strictly validated,
  sets the rules, and decides whether and at what level `codeflow ci`
  gates the push (`git.test_gate_on_push`), so neither the head nor the
  working copy can lower or turn off that check. For a head that shares
  history with that tip, the commit checks run over everything the head
  adds to the tip, as the hosted job's range from the target tip does, so
  a commit the destination already holds under a tag or another branch,
  or one an earlier push carried before the policy tightened, is still
  checked. A head whose recorded history shares nothing with the tip,
  such as a `gh-pages` deployment branch, is never diffed against it: a
  fast-forward of a branch the destination already has keeps its own new
  commits as its commit range, and a new branch keeps the commits the
  destination does not hold yet, under that policy, and the hook says
  this is not default-target parity. A shallow clone, a graft or a
  replace ref cannot show the histories are unrelated, so it keeps the
  tip. A target the task record
  declares bounds only the other checks: nothing local proves the pull
  request goes there, so a branch built on an integration line has the
  line's inherited commits judged by the default branch's current policy
  as well. That is stricter than the hosted job for a pull request into
  the line, for inherited commits only; those commits must pass that
  policy when the line's pull request reaches the default branch anyway.
  `codeflow ci` names both ranges when its commit checks run from another
  commit than its base, and then does not call the run the hosted verdict.
  A policy there that this codeflow cannot read or validate refuses the
  push, whether or not its range resolves; when a newer codeflow wrote it, upgrade the
  local one. With no candidate authority (a destination that does not
  answer, a failed fetch, or a default branch with no policy yet), the
  hook says so and, where a range resolves, still runs `codeflow ci` at
  block level with the policy at the range's base, as a best-effort
  check. When an `upstream` remote points elsewhere than the
  push, the hook notes that a pull request may target the upstream, whose
  policy can differ. The tree checks in the pre-push hook
  (`codeflow validate --docs` and the quick targets) and the release
  preflight still follow the working copy's gate, and the commit-msg and
  other local hook stages still read the working copy, so a commit relying
  on a loosened rule is made and then refused at push, before it leaves
  the clone. Run directly, `codeflow ci` judges with the policy at the base
  it is given, or at the commit `--policy-from` names, as do its
  automation profiles; its banner says the result is the hosted verdict
  only when that commit is the pull request's target tip, which the hosted
  jobs pass. A base with no policy yet, as in the change that adopts
  CodeFlow, still uses the working copy's. A branch that changes the
  policy lands that change before commits that rely on it.
  When a local target branch is behind its upstream, `codeflow work start`
  and `codeflow ci` no longer print a note asking you to fast-forward it:
  they already anchor on the upstream, `work start` names it, and the
  guards refuse the fast-forward steps the note printed. When git itself
  refuses the base, for example a `GIT_REPLACE_REF_BASE` without a trailing
  slash on git 2.55 or later, `codeflow ci` now prints git's message in
  place of the advice to fetch.

<!-- codeflow:release-impact patch -->
- **The release binary reports a clean build.** The 3.0.0 binaries print
  `dirty=true` in `codeflow --version` although they were built from the
  tagged source: the release job writes cargo-dist's manifest into the
  checkout before it builds, and the build counted that untracked file.
  The file is now ignored, so a release build reports `dirty=false`.

<!-- codeflow:release-impact patch -->
- **A double-quoted backslash stays in the guards' reading.** Inside double
  quotes Bash removes a backslash only before `$`, a backquote, `"` or
  another backslash. The git guard removed it everywhere, so
  `git -C "C:\Users\a\repo" commit` was judged against a path that does not
  exist and refused. It now reads the path the shell passes.

<!-- codeflow:release-impact patch -->
- **The dependency audit names the suppression file osv-scanner reads.**
  When the CI template's audit step fails, it told you to record a
  justified suppression in `.osv-scanner.toml`, a name osv-scanner never
  reads. It now names `osv-scanner.toml` in the same directory as the
  lockfile it covers. A docs portal scaffolded by CodeFlow currently reports
  GHSA-ch52-4w7c-c8xp in `http-cache-semantics`, which has no fixed
  version; Astro uses it only to time its build-time cache of remote
  images. A new portal now starts with an `osv-scanner.toml` that ignores
  it, with that reason, until 2026-11-30; the file is yours to edit or
  delete. An existing portal is not changed: if your policy blocks on
  advisories and you accept the reasoning, add the same entry to
  `docs-portal/osv-scanner.toml`.

<!-- codeflow:release-impact patch -->
- **The scaffold passes its own secret scan.** In every repository
  scaffolded by 3.0.0, the CI secret scan failed from the first commit:
  gitleaks' `generic-api-key` rule took the words "vulnerable/malicious" in
  the pipeline workflow's security stage (line 209 of
  `.claude/workflows/pipeline.workflow.js` and its baseline copy) for a key.
  New scaffolds word it differently. For repositories that already hold the
  3.0.0 line, the CI workflow `codeflow update` installs runs gitleaks with
  your configuration as before, then drops only `generic-api-key` findings
  whose value and matched text are exactly that prose in those two paths, so
  the scan passes without editing history. Anything else, on the same line
  included, still fails the job, and so does a scan that logs an error,
  reads no commit, or whose git run fails part way. A wrapper that
  runs gitleaks itself can add the entry the CI README shows.

<!-- codeflow:release-impact patch -->
- **A pull request can no longer exempt its own leak from the secret
  scan.** The CI template's gitleaks step read `.gitleaksignore`,
  `.gitleaks.toml`, a `.gitleaks.json` beside it and inline
  `gitleaks:allow` comments from the pull request's checkout, so the change
  that added a secret could add its exemption too and pass. The step now
  reads every exemption from the trusted commit: the pull request's base,
  or the pushed commit on a push. A new exemption takes effect once its own
  pull request merges, and an inline `gitleaks:allow` comment counts only
  on a commit the trusted commit already holds. A file your configuration
  extends by `[extend] path`, and a `GITLEAKS_CONFIG` file in the
  repository, are read from the trusted commit as well. The step fails
  with a message naming the fix when the trusted commit is not in the
  checkout, when it does not hold an extended file, or when an extended
  file is named by an absolute path into the checkout. gitleaks is now
  downloaded and unpacked under the runner's temp directory, so a file or
  link a pull request commits at that name is not written through. Nothing
  from the checkout runs or steers the scan: its Python helpers run
  isolated (Python 3.11 or later), git reads `.gitattributes` from the
  trusted commit (git 2.41 or later), and no step before the scan runs
  code from the checkout. If you add a step to the secret-scan job, add it
  after the scan. **If you customised the workflow and the secret-scan job
  already runs a step of yours before the gitleaks step, move that step
  after the scan or into another job when you update:** the 3-way merge
  keeps it, and `codeflow update` now warns about it on every run until it
  moves. If you renamed the scan step, update cannot check the order and
  says on every run that the job's step order needs your review. gitleaks
  now reads the whole history of HEAD, the base's
  included (on a pull request, the pull request merged into its base; on
  a push, the pushed commit), instead of every fetched branch and tag, so
  an unrelated branch can no longer fail a pull request's scan. This
  narrows coverage on purpose: a branch with no pull request, or a tag, is
  not scanned by this workflow unless its commits become reachable from a
  scanned HEAD, so a repository-wide audit needs a scan of its own. The
  refusals below check only the commits a pull request brings; on a push
  that range is empty, since the pushed commit is the trusted commit, so a
  push scan reads its history without them. gitleaks also reads what a merge
  itself adds, files whose type changes and files git judges binary, which
  gitleaks' default history scan leaves out, so a secret added in a merge
  resolution, in a file that replaces a link or after a NUL byte is
  reported, under the file's own path. In the commits a pull request
  brings, git cannot show what an octopus merge adds, so the step refuses
  one; merge the branches one at a time. It also refuses a path with a
  backslash, a double quote or a control character that one of those
  commits changes, or that either side of a merge among them changes,
  since gitleaks cannot read such names reliably. Each refusal names the
  commit and the refs that hold it; rewrite the pull request's commits, or
  rebase onto the base when the change is on the base's side, since a
  later rename leaves the name in the earlier commit. Names with spaces or
  non-ASCII letters pass. The scan pins git's patch format, so git
  configuration on the runner, such as `diff.noprefix`, cannot move a
  finding to another path.

<!-- codeflow:release-impact patch -->
- **A human's override covers protected commits and pushes.** The README
  says a human can override the git-hook plane with
  `CODEFLOW_HUMAN_OVERRIDE=1`, but the pre-commit and pre-push hooks
  ignored it, so the first push of `main` to an empty remote after
  `codeflow init` needed `--no-verify`. Both hooks now honour the override
  for the protected-branch rules, as the merge hooks already did:
  `CODEFLOW_HUMAN_OVERRIDE=1 git push -u origin main` works. A force push
  or deletion of a protected branch and the secret checks stay refused, and
  the git-guard still refuses an agent that sets the override. A push to a
  protected branch whose remote tip this clone has not fetched cannot be
  proven a fast-forward, so it is refused as a force push until you fetch.
  The git-guard also sees git behind launcher options (`command -p`,
  `exec -a NAME`, `nohup`, `/usr/bin/time -o FILE`), including an override
  hidden in a git alias declared that way. It skips a launcher option it
  does not know rather than trust it, so a command that runs nothing, such
  as `nohup --help git push`, may be refused.

<!-- codeflow:release-impact patch -->
- **A release refuses a binary that is not a clean build.** Before a
  release is hosted, and on every dry run, each platform archive is opened
  and its `codeflow` binary must identify as the release version at the
  release commit with `dirty=false`. A dirty build, a build from another
  commit, or an archive without a binary stops the release.

<!-- codeflow:release-impact patch -->
- **Catch-up merges no longer count as extra pull requests.**
  `codeflow validate --docs` warned that most completed standalone tasks
  were "completed by N pull requests", because it counted every merge whose
  subject names the task branch, including the merges of the target into
  that branch that the rules ask for. It now counts only merges that brought
  the task branch in. With the default merge subjects (GitHub's "Merge pull
  request", git's and GitLab's "Merge branch"), a task that landed once
  draws no warning and a task that really landed twice still does. A merge
  with a custom subject may still be miscounted, and a task landed by
  squash, rebase or `codeflow integrate`, which write no merge, is not
  counted, as before.

<!-- codeflow:release-impact patch -->
- **`codeflow present show` says when the browser is already open.** On a
  session whose browser is still running, `show` exited 4 with
  "presentation browser launch is not qualified". It now says the session's
  browser is already open and tells you to switch to its window, or quit
  that browser and run `show` again; `show --no-launch` prints the
  session's address. The exit code is still 4.

<!-- codeflow:release-impact patch -->
- **A new task may change its criteria after a reopen in its own pull
  request.** A standalone task whose record exists only on its branch,
  completed, reopened and given another criterion there, could not be
  completed again: `codeflow task status` and `codeflow ci` refused with
  "a reopened task keeps its criteria as the anchored target has them",
  though the target holds no criteria to keep. Such a task now completes
  with its new criteria. A task the target already records still keeps
  its criteria across a reopen, also when the branch moves its record to
  another layout, renumbers it with its uid kept, or retargets it away
  from `main` or from the integration line it was planned on, and
  whether the target is read from a stale local branch, an upstream on
  another remote or an older comparison base. A task is new only when no
  other branch adds or edits its record, so rewriting the branch's own
  history cannot hide a recorded task; the refusal names the branch that
  records it. The default branch is the
  one `origin/HEAD` names, else `main` or `master`. A clone that lacks a
  target the task's record names or the default branch cannot tell, so
  it refuses the change and names the branch to fetch, or explains how
  to record `origin/HEAD` when it finds no default branch.

<!-- codeflow:release-impact patch -->
- **A follow-up of a standalone task can land.** `codeflow task new
  --follow-up-of` ran only on a `plan/` branch, but a planning pull request
  must name an epic, and a standalone task has none, so no branch or
  `Task:` line could land the record. A standalone task's follow-up is now
  a standalone task too: cut a task branch from the target, file the
  follow-up there, fill in and commit its record, then run `codeflow work
  claim`, which renames that branch to `task/TSK-NNN-<slug>` and pushes it.
  Its record lands with its work in one pull request. A standalone task
  never uses a `plan/` branch, and the command now refuses on one and names
  that route. A follow-up of an epic task still rides in the epic's batched
  amendment on a `plan/` branch.

<!-- codeflow:release-impact patch -->
- **An epic criterion served only by cancelled tasks can close.** `codeflow
  epic status <EPC> complete` refused such a criterion, while a criterion
  no task serves could close on the epic's own acceptance block, so the
  only way out was a new task that cited evidence already in hand. The
  epic's own block now proves it under the same rules: verified with its
  evidence, or waived with its planning commit, and the journey verified
  for a journey criterion. A cancelled task still never verifies a
  criterion, and a ticked checkbox does not either. The epic's own block,
  for criteria no task serves as well, is now bound as a task's block is,
  in `epic status`, `codeflow ci` and the release-line check: before, any
  commit-shaped value passed as its reviewed commit or a waiver. Its
  reviewed commit must exist, with only the epic's status and Closeout
  changed after it, and a waiver must name a planning-only commit that
  amends that criterion and that the reviewed commit contains. An epic
  close that names a fabricated or stale review, or a waiver that is no
  such amendment, is now refused, and the refusal prints the epic's
  repair: correct the block and have it reviewed, then rerun `codeflow
  epic status <EPC> complete --acceptance <file>` for an open epic, or,
  for an epic the pull request already completes, replace the block in
  its Closeout by hand in that pull request; an epic is never reopened.
  The cf-method project-organization reference states the same route.

<!-- codeflow:release-impact patch -->
- **The pre-push hook checks the journey criterion its pull request
  will.** `codeflow ci` classified a range only when a pull request body
  was given, so the pre-push run never reached `work.journey_criterion`,
  and a task branch that changes an adopter-facing path without a journey
  criterion passed the push and was blocked by hosted CI once its pull
  request opened. A run without a body now holds a branch that carries its
  task (`task/TSK-NNN-...`) to the journey rule over its range, which needs
  only the task record and the paths the range changes, so the push is
  refused with the finding the pull request check gives. The refusal also
  names a criterion that carries `(journey)` inside its text and says the
  tag counts only where it opens or closes the criterion.

<!-- codeflow:release-impact patch -->
- **The exec-guard refusal names the file route for text that mentions a
  peer.** On a line exec-guard cannot fully parse, such as one with a
  variable as the program or a here-string, it judges the raw text, so a
  heredoc or inline string that names a peer with a headless flag is
  refused, and a review brief or commit message written that way was
  refused with no way forward. That matching is unchanged and is flagged
  by design, refused at the default block level: a reader that tried to
  leave such text out kept
  missing shell forms that still run a peer. The refusal now says so and
  names the route that works: write the text to a file with the editor
  tool and pass it by path, as `git commit -F <file>`,
  `gh pr create --body-file <file>` or `gh api ... -F body=@<file>`.
  Verdicts are unchanged: 1,618 headless run forms and 12 text shapes
  compared with 3.0.0 get the same verdict.

<!-- codeflow:release-impact patch -->
- **A reviewed task can take its moved target without a new review.** The
  release-impact check needs a pull request to contain the current target,
  so after every merge to `main` a reviewed task merges `main` in. In a
  clone whose local `main` lags `origin/main`, such as a root checkout that
  is never pulled, `codeflow ci` and the pre-push hook then refused the
  completion with `work.acceptance_binding`, because they read the task's
  target from the stale local branch, took the merge for foreign work and
  asked for a new review. The binding now checks the merge against the
  target tip the run is judged against: the base `codeflow ci` is given,
  which hosted CI sets to the pull request's base, and in the pre-push hook
  also the destination default branch's advertised tip. A local branch, its
  upstream configuration or a remote-tracking ref never decides it. A merge
  whose second parent is on that tip's first-parent line, and whose
  recorded result equals the conflict-free automatic merge of its parents,
  keeps the binding. Any other merge, a merge of more than two parents, a
  later commit beyond the record's status and Closeout, a graft or replace
  ref, or a shallow cut on the walked chain refuses, even when the change
  cancels out, and the refusal names that commit. For the reopen rule in
  the entry on new tasks above, a landed record named `TSK-NNN.MD` counts
  as on the target, since the record reader takes the `.md` extension in
  any case, and only the target the run is judged against supplies the
  criteria a reopened task keeps: a local branch or remote-tracking ref,
  such as an `origin/main` or another remote's upstream pointed at the
  task's own branch, can make the check stricter but never supplies
  criteria.

<!-- codeflow:release-impact patch -->
- **The portal's claim quarantine test no longer fails at random.** The
  docs-portal test "unverified retired claims are quarantined without
  regaining authority" replaced the retired workflow claim by deleting it
  and writing a new file. The portal identifies a claim by device and inode
  number, so on a filesystem that hands a freed inode straight back (ext4,
  tmpfs) the replacement could be taken for the original, the lease was
  released, and the test saw no error code. It failed once in hosted CI and
  passed on rerun. The test, in the shipped starter and in this repository's
  own copy, now builds the replacement while the original still exists, so
  the two inodes differ on a filesystem that numbers coexisting files
  uniquely, and asserts that. A portal adopted earlier gets the fix when
  `codeflow portal setup` reconciles the starter, with an updated binary
  and managed files you have not edited; a portal whose ownership was
  transferred, or whose managed test file was modified, is not updated.
  The portal's runtime and its claim identity check are unchanged.

## [3.0.0]

_Staging evidence: this section was first staged on 2026-08-02; that was not a
publication date._

> **Upgrading from 2.1.0.** Take these steps in order; the entries below give
> the detail.
>
> 1. On a planning branch, make the repairs in the "Breaking migrations" note
>    of this section (coverage scopes, test modes,
>    `security.dangerous_commands`, and what `codeflow validate --docs`
>    reports), and merge them before updating.
> 2. Install the 3.0.0 `codeflow` on `PATH`. The hooks run that binary, and
>    an older one rejects the new policy keys and blocks every commit.
> 3. Land a pull request that raises only `scaffold_version` in
>    `.codeflow/project.toml`, so CI installs the pinned, checksum-verified
>    3.0.0 binary.
> 4. Run `codeflow update` on a new branch and review what it proposes.
>    Reasoning effort now defaults to high. A project with a remote and
>    existing records runs `codeflow ids seed` once.
> 5. Every pull request names its work on a `Task:` line. With durable
>    work tracking that is `Task: TSK-NNN`, or `Task: EPC-NNN` for a
>    planning change or an epic's integration line; without it, the name
>    of the tracked unit. Give each `git.automation_profiles` entry a
>    `task` so a bot's pull requests keep passing.
> 6. A project that adopted the bundled portal follows the ownership table
>    in `docs/releasing.md` before its next portal update.

> **Known limits of this release.**
>
> - No designated seat has a full-suite qualification record yet, so
>   `codeflow doctor --check model-bindings` warns on every scaffold. The
>   first native batch (Claude Opus 5.5, high effort, auto mode) passed 2
>   of 11 process-round cases; most failures stopped to ask instead of
>   taking the required step. Three trials were invalid because of kit
>   gaps, fixed in this release. The full suite on the Claude, Codex and
>   Grok seats follows in a patch release.
> - The release gate ran on macOS. The native Linux and WSL2 rows of the
>   release qualification were not run. WSL2 uses the Linux archive.
> - Native Windows is not supported. This release publishes no Windows
>   archive, PowerShell installer or `codeflow.exe`. The hosted Windows
>   test job found real defects, among them a git guard that misreads
>   Windows paths and refuses ordinary work. Native Windows support is
>   planned for 3.0.1; until then, use WSL2.

### Fixed

<!-- codeflow:release-impact patch -->
- **An old Git is named when it cannot read attributes from a commit.**
  `codeflow ci` reads each changed file's conflict-marker size with
  `git check-attr --source`, which needs Git 2.40 or later. An older Git
  refuses the option and exits before it reads the paths. On Linux the path
  write then failed first, and the check reported a broken pipe with advice
  to fetch the whole range. It now reports Git's own refusal and says to
  upgrade Git.

<!-- codeflow:release-impact patch -->
- **The docs portal starter no longer pins a vulnerable `fast-uri`.** The
  starter's development dependency override moves from 3.1.6 to 3.1.8,
  which fixes GHSA-58mr-gqgx-xq4g, GHSA-qw65-cvwx-89v3 and
  GHSA-hrr3-gc8f-f4qj. A portal adopted from the starter takes it at its
  next `codeflow portal setup`.

<!-- codeflow:release-impact patch -->
- **The release pull request passes classification.** A release pull
  request names its release-integration task (`Task: TSK-NNN`), as SPC-013
  R-120 says. `codeflow ci` judged that body by the task pull request rules
  and refused it: a release brings every line's records, and the task
  completes at the head. On a release head, under the built-in or the
  configured release branch pattern, the task the release checks select as
  owner now classifies as the release pull request, which those checks
  judge. Any other task named there keeps the task rules, including a
  `role: release-integration` task cancelled or completed before the range.

<!-- codeflow:release-impact patch -->
- **Portal publication keeps committed public files' modes.** The docs portal
  rewrote the committed files in `public/`, such as `favicon.svg`, as
  owner-only (0600). In a fresh clone the first full test gate then failed
  with "generation changed the candidate". Committed files now keep their git
  mode, and other files the portal preserves keep the mode they had.

<!-- codeflow:release-impact patch -->
- **Tracking authority transport checks.** Fetch, pull and remote update reject
  URL rewrites and configuration overrides; fetch and pull also reject arbitrary
  tracking-ref destinations. Direct writes to global Git config
  files are refused while ordinary config reads and user-name updates pass.

<!-- codeflow:release-impact patch -->
- **Worktree removal under a harness sandbox.** The cleanup rules say that a
  sandbox denying writes under `.claude/` or `.git/` stops `git worktree
  remove` partway. After the proof, the removal runs once through the
  harness's sanctioned unsandboxed path; with no such path the worktree is
  kept and the proven removal goes to the operator. Never `--force`.

<!-- codeflow:release-impact patch -->
- **Peers in qualification trials.** The repository's qualification runner
  routes seats a trial subject opens in Herdr through checked launchers: the
  trial environment and workspace, the same argument allowlist and isolation,
  and the Codex hook-trust option only for Codex peers. Trust dialogs are
  answered only for verified launches; other peers flag the trial invalid.

<!-- codeflow:release-impact patch -->
- **Hooks in `.git/hooks` on adoption.** Setting `core.hooksPath` stops git
  running a project's own hooks, such as those `pre-commit install` writes.
  `init` and `update` now name each executable hook in that folder, also
  when run from a linked worktree, and give two choices: move the check into
  CI or a hook manager, or keep it in a project-owned hooks folder set as
  `core.hooksPath` that also calls the CodeFlow shims (see
  `docs/adoption.md`). `codeflow doctor` warns while they stay. Nothing is
  moved, and hooks are not chained.

### Added

<!-- codeflow:release-impact major -->
- **Landed policy authority.** Agent guards read landed policy and protect
  its remote-tracking authority.
  Local policy edits cannot relax it. Contract-3 hooks refuse a missing or
  older binary. Install the new binary before `codeflow update`; configure
  the remote HEAD with operator `git remote set-head origin --auto` when
  needed. Doctor and orient report policy sources and local drift.
  Without remote HEAD, guards merge main and master by the stricter levels; custom defaults need operator set-head.

<!-- codeflow:release-impact minor -->
- **Fetched work targets.** Work claims discover records on fetched target
  branches. Work start prefers a fetched origin target when a local branch
  without an upstream is stale. The automation profile schema also documents its `task` field.

<!-- codeflow:release-impact major -->
- **Agent sessions refuse instead of prompting (ADR-0075).** The Claude,
  Codex and Grok presets are generated from one action table and carry no
  ask rules, so delegated and primary sessions no longer stop on prompts.
  Agent sessions refuse these actions, and the operator performs them:
  privilege escalation (`sudo`, `su`, `doas`, `pkexec`, `gsudo`, `runas`,
  `Start-Process -Verb RunAs`, `osascript ... with administrator
  privileges`); package and gist publishing; release changes and tag
  pushes; repository deletion, archiving, renaming and visibility, `git
  push --mirror`, `gh secret` set and delete, `gh auth` login, switch,
  setup-git, token, refresh and logout, and `git credential`; keychain
  reads; and user-level persistence (`defaults write`, `launchctl`,
  `crontab -e` and `-r`, `systemctl enable`, registry writes). The deny
  rules match these commands as written; exec-guard and git-guard also
  parse covered wrappers, leading flags and tag-push spellings, including
  `cargo +stable publish` and `git push origin v1.2.3`. Codex gets `.codex/rules/codeflow.rules`, and its `cf-guard`
  profile now runs the network proxy, with a `cf-builder` profile defined
  beside it but not selected (tested on Codex 0.159.1; earlier versions
  are unqualified). Grok gets `.grok/sandbox.toml`, written only when
  absent. The Claude presets' sandbox now withholds model, cloud and
  publishing credential variables and the common credential stores
  (`~/.ssh`, `~/.aws`, `~/.netrc` and others) from every subprocess.
  `security.privilege_escalation` and `security.headless_peer_runs` default
  to `block`, so exec-guard refuses a headless peer run (`claude -p`,
  `codex exec`, `grok -p`) and a privilege launcher run directly, chained
  or wrapped in a shell `-c` string or `eval`; a shell string that reaches
  no launcher, `source` and `LD_LIBRARY_PATH` are not refused. The policy
  file carries `outward_actions` and `secret_reads` for their action
  families. Interpreter forms use the underlying action's policy level.
  The reserved keys `script_bypass`, `interpreter_scan`,
  `enforcement_baseline`, `workflow_pushes`, `sandbox_retry` and
  `sandbox_retry_allow` remain unread. Legacy `headless_opt_in` is
  accepted but ignored with a warning; `codeflow update` removes it.
  - Order: install the new `codeflow` on `PATH`, then run `codeflow
    update`. Update merges the permission arrays three ways against the
    last shipped copy: a rule the preset retired is removed, the 2.x ask
    rules included; a rule you removed stays removed and is reported on
    every run; your own rules stay, and none of them moves where that would
    change what a `!` exception lifts. A policy value still equal to the
    previous shipped default moves to the new default and is reported; a
    value you set is kept. Where no shipped copy was recorded, update
    compares with the files 2.1.0 shipped, says so, and adds back every
    shipped deny.
  - Relief: to let agents run one of these actions in a project, remove its
    deny entry from `.claude/settings.json` and relax the policy level
    exec-guard also checks in `.codeflow/policy.json`:
    `security.privilege_escalation` for privilege escalation,
    `security.secret_reads` for keychain reads, and
    `security.outward_actions` for publishing, release, account and
    persistence actions. To allow headless peer runs, set
    `security.headless_peer_runs`. Update keeps these changes. A local ask
    rule cannot restore a denied action, because a deny rule wins.

<!-- codeflow:release-impact minor -->
- **One delivery process (ADR-0076).** The method, the rules and the
  skills state one process, each part in one home. A brief or spec is
  planned once, into an epic with outcome-sized tasks or one standalone
  task, in one planning pull request; work attaches to a task and reuses
  the plan unless its outcome, interface, dependencies or safety change.
  One task is one pull request that carries its own record. Review is one
  independent pass over the whole change per revision, with no round or
  count cap. The primary assembles reviewed task heads into a small batch
  and runs one full gate on that exact candidate before the line moves; a
  standalone pull request is its own candidate. There
  is no per-task planning pull request, closeout narrative or review cap,
  and adding a rule that puts a pull request, approval, round or record on
  every piece of work needs the operator. ADR-0076 is accepted and six
  earlier ADRs carry dated notes on the clauses it changes. `codeflow
  update` rewrites the managed skills, agents, rules and templates, and
  removes the files whose duties moved: five cf-model-orchestrator routing
  resources, its other-hosts reference and the cf-present review example
  (an unmodified copy is removed, a modified one is kept and left
  unmanaged).

<!-- codeflow:release-impact minor -->
- **Hooks name the codeflow that judges.** Every git hook stage except
  reference-transaction prints one line on stderr naming the binary that
  judged the change, its version, the source commit it was built from,
  whether that source was dirty, and a digest of its source inputs.
  `codeflow --version` prints the same identity after the version, as in
  `codeflow 3.0.0 source=<commit> dirty=false inputs=<digest>`, so a script
  that reads the version reads the first token after the name.

<!-- codeflow:release-impact minor -->
- **Workspace mode for umbrella repositories.** An umbrella that holds
  several projects, each its own repository, keeps its root checkout on a
  working branch, `integration/workspace` by convention.
  `codeflow init --workspace` creates or reuses that branch, sets
  `git.root_branch` and adds every nested repository to `.gitignore`,
  leaving registered submodules alone; it refuses over uncommitted changes.
  Plain `init` and `update` in such a folder switch nothing and name the
  flag. `codeflow doctor` reports the root branch, nested repositories no
  tracked `.gitignore` covers, and linked worktrees outside
  `git.worktree_locations`, whose default covers `.worktrees/` and the
  folders Claude, Codex and Grok manage; it reads the root checkout from a
  linked worktree too. The guide and `.codeflow/rules/worktrees.md` say how
  a change lands in an umbrella: small edits on the root branch, larger
  work in a short-lived worktree merged back, `main` moved forward only by
  the operator at a milestone, and each nested repository through its own
  pull requests. See `docs/workspace-mode.md` and ADR-0074.

<!-- codeflow:release-impact minor -->
- **Unresolved conflict markers are refused.** The pre-commit hook and
  `codeflow ci` refuse an unresolved conflict marker on a line a change adds
  to a text file, under the new `git.conflict_markers` key. It defaults to
  `block`, a behaviour change: a commit that adds a leftover marker now
  stops, and `codeflow update` adds the key and reports it. A separator line
  counts only between an opening and a closing marker, so a Markdown heading
  underline passes. A file that must hold markers sets
  `conflict-marker-size` for its path in `.gitattributes`, git's own rule,
  and a team can set the key to `warn` or `off`. `codeflow ci` also catches
  a marker left while resolving `git rebase --continue`, which runs no
  pre-commit hook.

<!-- codeflow:release-impact minor -->
- **Release integration after landings.** CodeFlow's repository workflow imports
  verified epic lines after a landing and daily, checking the combined release
  before pushing. A conflict or finding leaves the release branch unchanged and
  names its owning task with local reproduction commands. Task pull requests do
  not wait for integration; adopters receive only conditional shipping guidance.

<!-- codeflow:release-impact minor -->
- **Guidance retention evaluations.** `cf-evaluate-model` gains a scripted
  multi-turn case kind: the fixture supplies warm-up turns, the case prompt
  is the probe, and only the probe turn is graded. A new
  `guidance-retention` pack checks three rules (a landing time asked for in
  a plan, a status report, a multi-part explanation), each with a paired
  negative, in a fresh arm and an arm that compacts automatically inside the
  disposable fixture on the Claude host. `eval_kit.py check-session` checks a
  trial's transcript against its turn plan and extracts the probe turn, and
  `retention-report` applies the retention bar. `codeflow update` installs
  the changed skill at the standard and full tiers.

<!-- codeflow:release-impact minor -->
- **Outcome-first working and reporting (ADR-0071).** The contract, the
  lifecycle reply rule and the report owners put the result first: an agent
  names the result, who uses it and the evidence that would establish it,
  and a gate or criterion counts only as evidence toward it. A reply or
  report opens with the result and where it stands, then what would change
  it, then what the reader must do. Items the operator must act on go once
  under NEED YOUR ATTENTION, and not at all when nothing is owed. Em and en
  dashes are a prose guideline judged in review. `codeflow ci` no longer
  warns on a code span, a path or the sentence count of a pull request
  Summary. The evaluation
  kit adds requirement CF-OUT-007, an amended CF-OUT-002 and an
  `outcome-first` pack.

<!-- codeflow:release-impact patch -->
- **One work lifecycle section.** cf-method's project organization reference
  states once how a work item moves and which verbs move it: allocation on a
  planning branch, `work next`, `work claim` and `work start`, the dependency
  forms and guarded selections, every `task status` transition, and spec and
  epic status. cf-plan, cf-develop, cf-ship, cf-customize and the task graph
  point to it, and the research folder, spec and epic, and standalone rules
  are each stated there once.

<!-- codeflow:release-impact minor -->
- **Review findings, repair and a copy guide.** The duo quality contract has a
  new section on review findings and repair, read when a defect is fixed or
  review findings are briefed, written or acted on. A defect fix states its
  evidenced mechanism and adds a regression test that fails before the fix.
  Every change names its bounded impact set, and each blocker or major
  finding carries the smallest evidenced remedy. Review is one holistic
  pass per revision: every assigned reviewer reviews the whole change in
  parallel, the builder applies the accepted findings as one cycle, and the
  reviewer who raised a material finding confirms its fix. No round or
  cycle count ends review: it ends when every criterion has evidence, the
  needed checks are green, no material finding is open and every nit has a
  disposition, and a stalled repair is split, redesigned or taken to the
  operator.
  Blocker navigation now stops before a change departs from what was
  approved. The writing reference gains a copy guide with ten sections, each
  with an example quoted from a named source. `codeflow update` installs the
  new section and the changed skills at the standard and full tiers, and the
  writing reference at every tier.

<!-- codeflow:release-impact minor -->
- **Acceptance bound to the reviewed commit.** Completing a task with `task
  status complete`, and every completion in a pull request range in `codeflow
  ci`, now checks that the acceptance block's `reviewed` commit is the
  completing commit (a task pull request's head) or an ancestor after which
  only the record's status and Closeout changed, or the second parent of a
  clean landing merge (or an ancestor of it followed only by the record's
  status and Closeout) that only merges and planning records follow. Reviewed
  task heads that land together keep their binding, and so does a task branch
  that merged its target after review when its tree equals the clean re-merge;
  a hand-resolved product hunk unbinds it. Each waiver names a record-only
  amendment commit that changed that criterion and is in the completion's
  history, either on the task's own integration target or, before review, in
  the task's own range. `task status complete` also refuses uncommitted
  changes outside the record. A task's own pull request may change its
  criteria before its first completion, and `codeflow ci` prints the change
  for the reviewer; the pre-push check, which has no pull request body,
  lets the task branch carry that change too. A change to another task's criteria is refused unless the
  pull request's validated class is planning-only or a checked epic line,
  whatever its branch prefix, and a reopened task keeps its criteria as its
  target has them. A range touching the adopter-facing path set needs a task
  with a `(journey)` criterion or one serving the epic's journey, and a leaf
  serving it links the evidence that ran or names its narrower path. A
  criterion tagged `(after release)` is `deferred` with an owner, a window and
  a listed follow-up, never verified at build time. A tag opens or closes its
  criterion, and a period, comma, semicolon or colon after a closing tag still
  reads as the tag; a tag inside the text does not count. `git.work_records`
  sets the binding and journey rules to block or warn; frozen criteria always
  block. The output states that the check proves structure and binding only.
  An open task that changes product paths without a journey criterion gains
  one by a planning pull request, or the project sets `git.work_records: warn`
  while it catches up.

<!-- codeflow:release-impact minor -->
- **Release branches judged where each change was introduced.** A branch
  whose name matches the new policy key `git.release_branch_pattern`, read
  from the policy at the destination's default branch (default
  `integration/release-*`), is a release branch. Pre-push, `codeflow ci`
  and `task status complete` judge a push to it, a pull request into it and
  its pull request into the default branch by where each change came from.
  A merge whose other parents are on a verified epic line or the default
  branch is an import: what it brings keeps the verdict of its line, and a
  completion binds where it was introduced. A criteria change it brings is
  judged again where it landed on its line, unless that landing is at or
  before the cutoff of the line the task targets, on that line's
  first-parent chain, recorded in the
  `release_rule_baseline` table of project config on the default branch,
  which lists it as information. A criteria change that the task's own
  reviewed and completed pull request landed is accepted with a notice;
  one brought for another task, for an incomplete or reopened task, or by
  direct work stays frozen. The adoption marker `release_rules = 1`
  in project config never decides whether these rules apply; once the
  default branch carries it, removing it or changing its value makes
  every release check refuse. The marker's history is read from the
  parents each commit records. History the check needs that is cut short
  by a shallow boundary, or a config object missing from the clone, makes
  it refuse, since adoption cannot be read; so does a graft file or a
  replace ref, which would change the commits a release check walks.
  Everything else, including a
  merge resolution, is direct work: it may not change criteria (removing
  or re-creating a task record counts as a change), and code
  needs the one open task marked `role: release-integration`, completed at
  the release head. A completion made on the release branch, or brought
  earlier, is superseded only by a later one brought from the task's own
  line that binds where the line landed it, ordered by where that line
  landed each; the earlier one is never accepted, and a direct completion
  is judged as it was made, whatever a later import writes. An octopus import is
  judged as git merges it, so an older parent of a line adds nothing.
  The policy check refuses a pattern that matches the
  default branch or an epic line, and the validator refuses a second open
  holder of the role. When the default branch's policy file is missing or
  unreadable, or the destination names a default branch it does not have,
  the check fails closed instead of using the ordinary rules. A push
  to a release branch is judged on everything it adds to the default
  branch's tip, as its pull request is, however much of it the destination
  already holds under other names. Whether durable work is tracked is
  read at the checkout, the pushed commit and the destination's default
  tip, fetched when missing; a push is ordinary only when all three are
  read and none tracks. Pre-push now needs the destination to answer: a
  push is refused when it does not, when its default branch has no
  readable policy or project state (a state schema version this binary
  does not support included), or when its HEAD names no branch it has, in
  every project, tracked or not. The hook asks the destination once per
  push and passes the answer to each check it runs; a check given that
  hand-off is advisory only, and hosted CI never takes it. A task
  branch that merged its own line is judged from the newest line commit
  the destination holds, as its pull request is, so what the merge brought
  stays the line's. `task status
  complete` judges a completion whose task targets a release branch as CI
  judges that pull request. `codeflow ci` gains `--into`.

<!-- codeflow:release-impact minor -->
- **Release records judged where they landed, and a one-time bridge.** On
  a release range the records rule judges a spec approval brought from an
  epic line at the merge that landed it there, and does not judge a
  brought `uid` backfill again. A complete task brought without an
  acceptance block, last changed on its line at or before that line's
  cutoff in the new `release_records_baseline` table, is listed as a
  legacy record. `codeflow init` and `update` write the adoption marker
  `release_rules = 1` and never a table. Both tables are CodeFlow's own
  2.x to 3.0 transition only, and a consuming project cannot use one:
  each is honoured when it was added in one commit at or before
  adoption, after project config without the marker, never changed
  since, with every cutoff from before adoption, on its line's
  first-parent chain and one of CodeFlow's approved cutoffs, which the
  judge compiles in; otherwise every release check refuses, naming the
  condition and the commit. No flag, variable
  or policy key skips them.

<!-- codeflow:release-impact minor -->
- **Shared id registry.** With tracking on, `epic new`, `spec new` and
  `task new` reserve their number on the `codeflow/registry` data branch of
  `origin` by a non-forced push, so two clones can no longer take the same
  id. Each new record carries a hidden `uid` bound to its number. Offline,
  the number stays pending until `codeflow ids sync` (also run by pre-push)
  publishes it. New `codeflow ids` commands: `seed` builds the registry from
  existing records, `backfill` writes uids, `check` judges the registry, the
  merge rule and a uniqueness scan over all refs, `admit`, `retarget` and
  `restore` handle the exceptions. Pre-push and git-guard refuse deletion or
  force on the registry, `remote protect` adds its data profile, `doctor`
  gains an `id-registry` check, and the scaffolded `codeflow-policy`
  workflow runs `codeflow ids check` in its `commit standards` job on pull
  requests, pushes, a daily schedule and manual runs. A project
  with a remote and existing records runs `codeflow ids seed` once. Upgrade
  the `codeflow` on `PATH` first: an older pre-push hook refuses a registry
  push by branch name.

<!-- codeflow:release-impact minor -->
- **One readiness rule with `work next` and `work claim`.** `codeflow work
  next [--epic] [--json]` lists the ready tasks first, then waiting and
  blocked ones with their reasons, from the refs as last fetched, and names
  that snapshot. `codeflow work claim TSK-NNN` fetches, checks the task on
  its target tip as `work start` resolves it, refuses one a branch on any
  remote already carries, and pushes `task/TSK-NNN-<slug>`; the branch is
  an advisory claim. `work start`, CI,
  `status` and `orient` read the same rule: `status` shows active, ready,
  landed and conflicting branches with epic progress and keeps a live
  integration line, and `orient` prints a task summary. A `depends_on` entry
  may be `{id, kind: research | decision, pin: "<commit>"}`, met at its pin;
  an unquoted pin that YAML reads as a number, such as `70283613`, or a
  written `pin: null`, is refused with a message that says to quote it,
  while leaving `pin` out keeps the edge unmet. A code dependency complete
  only on another line waits until its change is in this base. A join can
  carry `awaiting_selection`, which only a `plan/` pull request removes,
  and `spec new --for` accepts several consumers.
  `work start` now names a blocked task's reason instead of its status, and
  refuses a task whose epic is complete, cancelled or archived.
  `work claim` and `work start` take `--on TSK-NNN@<commit>`, once per code
  dependency, to build on a predecessor that is reviewed but not complete,
  at its reviewed commit; the pin must name the predecessor's reviewed head
  on a fetched remote branch, and the task still lands only when that
  predecessor is complete at the merge base. `work next` suggests such a
  predecessor only at its exact reviewed commit.

<!-- codeflow:release-impact minor -->
- **Headless peer runs are flagged, and delegated turns carry their
  provenance.** exec-guard now recognizes a Bash command that runs a peer
  harness headless: `claude -p`/`--print` and `claude ultrareview`, `codex
  exec` (or `e`) and `codex review`, and `grok -p`/`--single`,
  `--prompt-file`, `--prompt-json` and `grok agent`. It reads the line the
  way git-guard does (control-structure bodies, groups, substitutions, `bash
  -c`), unwraps launchers such as `nice`, `timeout`, `sudo`, `xargs` and
  `find -exec`, and never reads an option's value or anything after `--` as
  the headless flag. A line it cannot resolve, such as an alias or a
  here-string, is flagged when its text names the peer with a headless
  flag, and the message says so. It names the interactive route instead: the Codex plugin, `codeflow
  delegate` over the interactive `claude` CLI, or a named Herdr tab. The new
  policy key `security.headless_peer_runs` sets the level: fresh installs
  and the built-in default use `warn`, `block` refuses, `off` disables it,
  and `codeflow update` adds it at `warn` while keeping an explicit level.
  `codeflow delegate init` takes `--model` and `--effort`; the terminal
  `codeflow delegate wait` result then carries `provenance`: the thread
  (the Claude session), and model and effort as requested at `init` and as
  observed in the `SessionStart` payload, each `unknown` when not given; a
  ready record that `wait --until ready` would reject, or from another
  session, is not cited, and `observed_unknown_reason` says why.

<!-- codeflow:release-impact minor -->
- **Adopter fit for bots, kept PR templates and release tools.** Trusted
  automation profiles (`git.automation_profiles`) let a named bot's pull
  requests skip branch naming and the commit message shape rules, and supply
  the PR sections its body omits; a profile's `task` field names the unit
  the bot's pull requests land under and supplies their `Task:` line.
  `codeflow ci --actor` passes the actor, the
  profile is read from the target branch, and the actor is trusted only in a
  same-repository GitHub Actions pull request event, so a local run, another
  CI or a fork pull request is `unknown` and nothing applies. A kept PR
  template is never
  shadowed: `init` and `update` record `git.pr_section_mapping` as
  `diagnosed` with a proposed heading mapping, an interactive run asks for
  accepted, refused or custom, and the check runs at `warn` only while a
  policy file this `init` created awaits that decision. `codeflow ci` prints
  the effective level and origin of every check it runs, and `doctor` gains
  an `adopter-fit` check. `release.backend` in `.codeflow/project.toml`
  (`none` by default, `external`, `codeflow`) names who owns versions, and
  release-please, Changesets, semantic-release, cargo-release and GoReleaser
  are recognised. Policy edits and the keys `update` adds are spliced into
  the adopter's bytes. `epic new`, `spec new` and `task new` use a project
  template from `project-management/templates/` when a record rendered from
  it carries the allocated id, uid, title, parent and target, and the
  shipped record and PR templates carry no em or en dash.

<!-- codeflow:release-impact minor -->
- **Pinned, checksum-verified CI binary.** The scaffolded workflows install
  the `codeflow` release the target branch pins in `.codeflow/project.toml`
  and verify it against the release's `sha256.sum`; a missing or wrong
  checksum fails the job, and no unverified binary is installed. The commit
  and PR-body standards and the id registry check run in a new
  `codeflow-policy` workflow on `pull_request_target`, so a pull request
  cannot edit the job that judges it. That job checks out the pull request's
  base commit, because GitHub's default checkout for the event is the default
  branch; a pull request into an integration branch is judged by that branch's
  pin and policy. Upgrade in this order: install the new binary, land a pull
  request that raises only `scaffold_version`, then run `codeflow update` on a
  new branch. Hook shims now warn when the `codeflow` on `PATH` is older than
  they are, and a policy with keys the binary cannot read names this order,
  including a pull request's own policy judged from the target branch.

<!-- codeflow:release-impact minor -->
- **The pinned CI binary on GitLab, Bitbucket and any other CI.** The
  `.gitlab-ci.yml`, `bitbucket-pipelines.yml` and `ci-generic.sh` templates
  no longer suggest the `releases/latest` installer. They install the
  release the target commit pins, verified against its `sha256.sum`, run
  `codeflow ci` from a checkout of the target, test a raised pin's release
  separately, and fail a lowered pin. `ci-generic.sh` now takes the target
  commit as its first argument and refuses to run without it. On GitLab the
  target is the target branch's current commit, never the diff base, and
  the job fails when it cannot fetch it. A branch that kept the pin it
  started from is not a lowered pin. If you copied
  one of these files and wired your own install, replace that install with
  the new template's shared script, or install the version your target's
  `scaffold_version` names and verify it the same way. `codeflow doctor`'s
  `ci-perimeter` check now names the version CI installs, and warns on a
  missing or lowered pin and on policy keys or schema carried before a
  raised pin has landed, naming the two-step order.

<!-- codeflow:release-impact patch -->
- **The Bitbucket template finds its destination without the commit
  variable.** Atlassian does not list `BITBUCKET_PR_DESTINATION_COMMIT`,
  and without it the step stopped at "no target commit". It now fetches
  `BITBUCKET_PR_DESTINATION_BRANCH` from `origin` and judges by that
  branch's current commit, failing with the branch's name when it cannot
  fetch it. The template also records that Bitbucket runs `codeflow test`
  and `validate --docs` on the merge of the destination into the source,
  while `codeflow ci` judges `BITBUCKET_COMMIT`.

<!-- codeflow:release-impact patch -->
- **Commit subject separator.** The commit-msg hook and `codeflow ci` now
  require a blank line after the subject, since git reads a following line
  as part of the subject. Reword such commits before pushing them.

<!-- codeflow:release-impact patch -->
- **Update proposals keep your changes.** When `codeflow update` cannot merge
  a managed file, the `.new` proposal is the three-way merge with conflict
  markers, so a job you added to a workflow is kept instead of dropped.

<!-- codeflow:release-impact minor -->
- **Work record lifecycle.** `codeflow task status`, `epic status` and
  `spec status` change a record's status only by a legal transition and write
  only what it needs: a Blocker for blocked, a cancellation reason and scope,
  an acceptance block on completion, a superseded block with the reason on
  reopen. `validate --docs`, the new `validate --since <ref>` and
  `codeflow ci` judge hand edits with the same rules. New records list
  criteria as `- AC-n` without a checkbox, spec `implemented` is derived,
  `in_progress` is no longer written, and the producerless work-graph ledger
  events are retired. The rules apply from a `work_records_baseline`:
  `codeflow update` to 3.0.0 records the full commit id of your current
  `HEAD` once when the project has records, so every existing record is
  legacy and only records added or changed afterwards follow the new rules.
  The baseline may list one commit per line of work, in any order (a single
  string still works). Each entry must be a full 40-character commit id that
  is an ancestor of the commit being judged; tags, branch names, `HEAD`
  expressions and abbreviations are refused. A record is legacy when it is
  unchanged from its copy in any listed baseline; an edited record is judged
  as a transition from its latest copies. A pull request is judged by the
  baseline list at its target's tip, even when the branch forked before the
  target had one, so a list change is reported and takes effect once it
  lands. The pull request that introduces the list (your first
  `codeflow update` pull request, or a release into a branch that has none)
  is judged by its own list, and a human reviews every entry it names. The new
  `git.work_records` key accepts `block` or `warn`; upgrade the `codeflow` on
  `PATH` before `codeflow update`, since an older binary rejects the key.

<!-- codeflow:release-impact patch -->
- **Record checks without false alarms.** `validate` no longer warns on an
  approved spec whose consumers are all done: `implemented` is its derived
  state and is never written, so that is the healthy state; a spec with no
  delivering consumer, or a written `implemented` the consumers do not
  show, still warns. A task completed before the `work_records_baseline`,
  and not reopened since, may carry a Closeout item
  `- acceptance: historical evidence unavailable; ...` naming its landing
  merge in place of an acceptance block. `ids check` judges a copy of a
  record on a branch that never landed by where that copy landed, and a
  backfilled `uid` alone no longer costs a record its baseline exemption.

<!-- codeflow:release-impact minor -->
- **cf-present from the agent sandbox.** The Claude settings presets add
  one sandbox write root, the per-user `codeflow present` state directory
  (`~/Library/Application Support/codeflow/present` on macOS,
  `~/.local/state/codeflow/present` on Linux), so an agent can open, update,
  read feedback on and close a session without a sandbox bypass.
  `codeflow update` merges the entry into existing settings, and `codeflow
  init` and `codeflow update` create that directory owner-only, since the
  sandbox cannot create its parents; a sandboxed first use without it names
  `codeflow update` as the fix. `present open
  --no-launch` and `present show --no-launch` also print a percent-encoded
  `file:` handoff link to the single-use bootstrap page, which the operator
  opens within 120 seconds. A Linux `XDG_STATE_HOME` outside the default is
  not covered by the preset.

<!-- codeflow:release-impact minor -->
- **Model catalog resolution (ADR-0069).** `codeflow models resolve --duty
  <duty>` reads the managed catalog, the personal overlay and the project
  selection and prints each participant a duty needs, with the pinned id to
  launch, its effort, the remaining alternatives and any obligation, or the
  open participant and why; `--json` serves launchers. It exits non-zero when
  a required participant is open and launches nothing. A design override
  counts only from an `OPERATOR_OVERRIDE` block committed in that task's
  record on its integration target. `codeflow doctor --check model-bindings`
  diagnoses the catalog and scans for pinned model selectors outside it.

<!-- codeflow:release-impact minor -->
- **One reference for when an agent stops (ADR-0070).** The new
  `cf-method/references/autonomy.md` holds the only full list of decisions
  that belong to the operator, a short ladder and a decision table, and the
  trust prompt rule: an agent answers a workspace trust prompt for its own
  task's folder or a sample it created, and any other folder goes to the
  operator. The contracts, the lifecycle, the orchestrator and cf-plan point
  at it instead of keeping their own lists.

<!-- codeflow:release-impact minor -->
- **Autonomy evaluation cases.** The `autonomy-with-judgment` pack adds
  seventeen blind cases, nine where an agent asks when it should act and
  eight where it acts when it should stop, each with a faulty control that
  fails. The stand-in answers the cases check are replayed from outside the
  trial's checkout, so a trial cannot read them.

<!-- codeflow:release-impact minor -->
- **Present conversations and revision checks (TSK-193).** Agent replies appear
  in the thread rail; reviewers can reopen resolved threads and delete notes
  with visible tombstones. `present diff` compares revision blocks and carries
  notes and answers; revisions record commit and dirty state. `present check`
  reports framing, anchor and form faults without a browser. Export includes
  the private conversation only with `--with-notes`. Codex model wake still
  depends on upstream CLI background tasks; a bounded wait on the agent's next
  turn delivers stored events until that support arrives. Safari on loopback
  HTTP receives deterministic gzip assets; Brotli remains preferred.

<!-- codeflow:release-impact minor -->
- **Portable pull request checks.** `codeflow ci` reads Markdown sections,
  rejects explicitly empty PR bodies and ambiguous headings, and warns about
  summary detail, missing testing limits and oversized evidence. Generic
  release checks default to warn, with a project-owned breaking level and
  commit floor. Fresh installs require Summary, Changes and Reviews, require
  Release impact on a pull request into a protected branch or with a breaking
  commit, and ship the PR template at every tier. Without an explicit list,
  the built-in default stays Summary and Changes. Updates preserve existing
  policy values and customized templates. Upgrade order matters: `codeflow
  update` adds `git.pr_release_impact` and `git.pr_breaking_level`, and an
  older binary then fails every `codeflow ci` run with exit 2 and `unknown key
  git.pr_release_impact`. Upgrade the local and CI binaries first, then commit
  the policy change from `codeflow update`.

<!-- codeflow:release-impact minor -->
- **Written content policy check (ADR-0067).** The commit-msg hook and
  `codeflow ci` report em and en dashes in new commit messages, pull request
  bodies and lines a change adds under `docs/`, `project-management/` and the
  skill trees. Existing lines are left alone, and so is a file whose bytes
  equal the managed asset the running `codeflow` ships for that path, so the
  scaffold and `codeflow update` ranges never trip on the managed skills. The
  new `git.policy_characters` key defaults to `warn`, including when a policy
  file omits it; set it to `block` to enforce the guideline, as CodeFlow's own
  repository does. `codeflow update` adds the key to an existing
  `policy.json`. Upgrade the `codeflow` on `PATH` before running
  `codeflow update`: the hooks call that binary, and an older one rejects the
  new key and blocks every commit.

<!-- codeflow:release-impact minor -->
- **Responsible-autonomy diagnostics.** The standard/full model-evaluation kit
  adds ten synthetic privacy, authority, recovery, identity and fairness cases,
  plus a bounded loopback effect simulator with tested setup and cleanup. These
  focused trials preserve failed and incomplete outcomes and do not claim model
  qualification, Hermes behavior or universal safety.

<!-- codeflow:release-impact minor -->
- **Consumer-owned same-work-PR release guidance.** cf-ship offers a compact
  starter only when a project lacks a compatible release process, preserves
  adopted tools and independent version domains, and adds four scoped release
  diagnostics without turning CodeFlow metadata into product version authority.

<!-- codeflow:release-impact minor -->
- **Rules come back after compaction and when a prompt needs them.** When
  a Claude or Codex session resumes, forks or restarts after a compaction,
  the session hook adds a guidance block after the digest: the always rules
  by title, the "when you are about to" moments with their first pointer,
  and every skill and agent the tier installs, generated from the rule-map
  kernel and the scaffold manifest (about 1.4 to 1.5 KB). On
  `UserPromptSubmit` the same command, `codeflow hook session-orient`, adds
  one rule line when a prompt asks for a duration, a status or a complex
  explanation, and nothing otherwise; the command reads the event from the
  hook payload. It is advisory and never blocks: the new
  `guidance.prompt_reminders` key defaults to `warn`, and `off` silences
  it. The key is not written into `policy.json`, so older binaries still
  read the file; setting it needs this `codeflow` or later. The Claude
  `SessionStart` matcher now names its sources,
  `startup|resume|clear|compact|fork`; Codex keeps its documented four.
  Grok Build's events exist but it ignores their output, so `codeflow
  update` removes CodeFlow's `session-orient` registrations from the Grok
  hook file and keeps its guards; after a Grok compaction, run `codeflow
  orient` yourself. `update` adds the wiring, moves CodeFlow's own
  `SessionStart` hook under the named sources without a duplicate, and
  keeps the project's own hooks. On a machine still running an older
  `codeflow`, the prompt hook prints that binary's session digest on every
  prompt instead of a reminder, and the prompt goes through; upgrading
  replaces the digest with the reminder. `codeflow hook prompt-reminder`
  prints the line alone for manual use and is never wired.

<!-- codeflow:release-impact minor -->
- **Release state checked before the pull request.** In a project that
  adopted CodeFlow's release calculator (`release.backend = "codeflow"` with
  `scripts/release.py`, as CodeFlow's own repository does), the pre-push
  hook runs `release.py preflight` for each pushed branch. It checks the
  release tree against the recorded baseline and local tags and says that it
  was not checked against the host. It warns when the range touches
  behaviour paths with no pending entry and no `Impact: none` in the draft
  that `CODEFLOW_PR_DRAFT` names, and it blocks only a push that breaks a
  release tree its base kept valid. `codeflow integrate` runs the same
  structural check in its test stage. `codeflow ci` now reads the Release
  impact block as `release.py` does, so a placeholder Rationale or the
  template's Migration choices left in place are reported. The
  adopter-facing path set now includes `.codeflow/policy.json`,
  `.codeflow/project.toml` and the record schema, so a pull request that
  changes them needs a journey criterion, as other adopter-facing changes
  do. `release.py` identifies each pending entry by its bold label (a
  duplicate blocks) and assesses an edit under a kept label at that entry's
  impact whatever the pull request declares; entries compare byte for byte,
  so a rewrap is an edit too. It accepts a typed repair of a base whose
  release state is invalid, which keeps every existing entry byte for byte
  and every stamp, baseline and hash
  consistent, and takes errata as dated notes in a `## Errata` block.

<!-- codeflow:release-impact minor -->
- **Evaluation grades file state and tool effects.** A model-evaluation case
  can now carry `expected.files` and `expected.effects`, and `eval_kit.py
  grade` judges the work a session left: file content and frontmatter, reviews
  read only in the reviewer's verdict format, from the verdict field alone and
  only when a recorded judgement finds them coherent, claimed branches and
  their tracking, records consistent with the id registry, the paths the
  session changed, meaning settled by recorded judgements bound to the exact
  text, product checks run under confinement with their expected output, and
  commit gates and acceptance blocks judged by the shipped `codeflow`
  checkers, whose failure to finish fails the assertion. It measures the
  result, not how it was made: CLI use, readiness checks and review before
  completion need the harness's own record of the session, and a command's
  process record is only reported beside the effect it names. A judgement
  counts only from a judge, with its exact configuration, whose calibration
  meets every labelled control of the graded suite (`judge-check`, `grade
  --calibration`); otherwise its assertion is ungraded and the trial is never
  scored as a pass. `record-judgement` signs each judgement under an
  evaluator key kept in the evaluator's CodeFlow home, and grading and
  scoring count only judgements whose signature verifies. Grading signs the
  whole grade, with every judgement it read, as a receipt under the same key,
  and scoring counts a pass or a failure only when that receipt verifies,
  names the result's run, the retained judgements rederive it, and the
  trial, graded again from its retained record, workspace and files, gives
  the same outcome. Trial records and reservations are signed under the
  evaluator key, so a rewritten baseline or a forged registration counts as
  an error. Keep a run's roots until every consumer has read it. Graded cases live in a graded
  suite outside the shipped kit (`--graded-suite`); a qualification holdout
  stays outside the published repository, and `holdout-check` fails when a
  holdout path, file, JSON object or copied run of text appears in the tracked
  tree. Subjects work in a separate subjects root, the fixture boundary covers
  both roots at every depth, a timed-out or errored session is kept and graded
  as a failure, and a pack result must keep every trial.

<!-- codeflow:release-impact minor -->
- **The root checkout keeps its root branch.** Task work happens in a
  linked worktree. A commit at the root checkout on any branch other than
  its root branch (`git.root_branch`, by default the default branch) is now
  refused for agents by git-guard and by the git hooks when a harness marks
  the session; a human at their own terminal is warned. This is a behaviour
  change for adopters whose agents commit at the root on a feature branch:
  move that work into a worktree, or set `git.root_checkout_commits` to
  `warn` or `off`. See ADR-0074.

<!-- codeflow:release-impact minor -->
- **Present no longer draws Mermaid diagrams.** The `diagram` block leaves
  the 3.0.0 document schema with its Mermaid renderer, so the review document
  entry above no longer caps diagram count, source or enhancement.
  `codeflow present open` and `update` refuse a document that holds one and
  name the block that replaces it on this release: an html block holding an
  inline SVG, a table or a tree block, as the conversion section of the
  `cf-present` authoring reference shows. A session a pre-release build stored
  with a diagram block still opens, read only: every other block renders as
  before, and each diagram shows its source and its conversion in its place,
  until `present update` stores the converted document. The
  web bundle, its licence list and SBOM no longer carry Mermaid.

<!-- codeflow:release-impact minor -->
- **Comments on one part of a figure, and framed figures (SPC-014).** A
  `cf-present` reviewer can comment on one node, arrow, label or legend
  entry: hover, click, touch and the keyboard resolve to the named part,
  thin strokes take a 6 px hit margin, and "select enclosing" climbs to the
  part around it and then the block. The service checks each part note
  against the revision and stores its own label, a PNG crop, and
  `crop_check: "unverified"` where it cannot measure the part. Earlier notes
  re-anchor by part, then quote, then block, and say when they moved. A
  `schema_version: 2` document frames every figure, stage and table as
  "Figure N · title" or "Table N · title" with a caption, legend and one
  Details disclosure, resolves `[fig:<id>]` references, and names stage parts
  with `data-cf-target`, `data-cf-group`, `data-cf-label` and `data-cf-for`.
  Version 1 documents render as before, except that an `html` title now
  shows, and the v1 `feedback` stream is unchanged. Drawn figures in present
  and the portal show their title line and one Details disclosure in place
  of the kicker and "Table twin"; the portal gate fails a figure whose title
  is not visible.

<!-- codeflow:release-impact minor -->
- **Form answers reach the agent (SPC-014).** `codeflow present feedback
  --wait --format v2` blocks until an answer, a correction (an `amendment`
  event) or a review arrives, `--timeout` bounds it with exit 6, and a closed session exits 7.
  `present responses list` reads events without delivering them, and
  `present ack` records that the agent handled one, so the page moves from
  "Stored, waiting for agent" to "Delivered to agent" to "Acknowledged by
  agent". A second copy of a page cannot send a second original answer; it
  offers a correction instead. The cf-present skill runs the wait as a
  background loop in Claude Code and Grok Build, so each answer wakes the
  agent. Codex CLI has no such background task: with Codex, an answer, a
  correction or a review is stored at once and delivered on the agent's
  next turn, and an answer's page state stays "Stored, waiting for agent"
  until then. The v1 `feedback` stream is
  unchanged and names pending answers on stderr.

### Changed

<!-- codeflow:release-impact patch -->
- **Managed skills follow the dash guideline.** Em and en dashes in the
  shipped skills are rewritten as commas, colons, full stops, hyphens or
  parentheses with no change of meaning; a dash stays only in a numeric
  range, a literal record string or text a test pins. The managed contract
  now states the dash rule as a prose guideline that `git.policy_characters`
  checks at the level policy sets. `codeflow update` replaces an unmodified
  swept file; an adopter who edited one gets a 3-way merge, or a `.new`
  sidecar where the edit conflicts.

<!-- codeflow:release-impact minor -->
- **Shared portal and present chrome.** Graphite, Slate and Sage use the
  approved design kit in both utilities and the installed portal starter.
  Search, Display, panel controls and narrow layouts follow the shared shell.
  Existing export values remain aliases: instrument and technical select
  Graphite, editorial selects Slate, and ink selects Sage. Portal signal
  selects Graphite and folio selects Sage. These values select a skin only;
  Inter is now the independent typeface default, replacing the portal's
  Archivo or Plex defaults for signal or folio. Present also starts in Inter;
  exports previously used a system-first sans stack. The export default stays
  editorial, resolving to Slate. Old saved Display skin and explicit font
  choices normalize independently before first paint and in the controls.
  No existing CLI or config value is removed.

<!-- codeflow:release-impact patch -->
- **Figure marks read without colour.** The stop mark is a square-capped bar
  and the merge diamond an accent stroke, so every mark pair in a figure
  differs on two channels besides hue. The boxed-text check judges each mark
  before the figure, so empty shapes no longer hide a figure drawn as
  labelled boxes, and coverage cells no longer count as boxed text. Narrow
  coverage grids bind their column labels and share one set of columns.

<!-- codeflow:release-impact patch -->
- **A narrow figure may keep its marks when it says why.** The figure gate
  accepts a narrow composition that draws the wide mark set again only when
  its declaration sets `marks: "same"` and gives a `reason`; without one it
  still fails as a reflow, and the height ceiling still applies. Narrow
  coverage cells are drawn at the wide size again, and a partial cell is
  shaded from its line, so it reads apart from an empty one in dark mode.

<!-- codeflow:release-impact patch -->
- **The present method and the minimal contract keep their full guidance.**
  The present method again gives the five-second test with its examples, the
  attention cost of each block and the bad and good page walk, and names a
  flow figure where it named the retired `diagram` block. The minimal-tier
  contract again says which harness each in-session guard wiring serves.

<!-- codeflow:release-impact patch -->
- **Narrow figure labels clear their marks in every engine.** A narrow extent
  row sets its label a full text box above its value, so a short bar's value
  no longer runs into its label in Firefox, and narrow coverage cells sit
  clear of their row name. The figure rule checks now measure text in the
  portal's own typefaces, not an engine's fallback.

<!-- codeflow:release-impact patch -->
- **Browser verification cleans up after a failed fetch.** A page fetch that
  fails during the portal figure check fails the check instead of ending the
  run. The verifier releases its workflow lock and stops its preview server
  on every exit, including an unexpected error.

<!-- codeflow:release-impact patch -->
- **Browser verification keeps its results on a long run.** Each engine's
  trace is kept only when that engine fails, and the results are written
  before the evidence files are counted. A file over its size cap is recorded
  as a failed artifact in the results instead of ending the run.

<!-- codeflow:release-impact patch -->
- **Portal altitude tabs, records table and home reading path.** The
  Concept, Architecture and Technical tabs sit on one line; before, the
  second and third tab sat lower. At phone width the records page stacks
  each folder's row, so the purpose reads as a sentence instead of one word
  per line. The home page shows every step of the reading path, one row per
  step from top to bottom, under a tighter title block.

<!-- codeflow:release-impact minor -->
- **Reading is checked by structure; sizes are reported, not failed.** The
  shipped instruction files load progressively: a small kernel (the managed
  `AGENTS.md` block) at session start, and everything else through an index
  entry or a reviewed trigger at the moment it is needed. CodeFlow's own
  tests now fail when that structure breaks (a shipped reference nothing
  reaches, or a conditional read without a trigger) instead of when a file
  passes a byte number. `codeflow doctor` gains a `reading` check that
  reports the kernel, the per-task reading chain and each shipped skill
  against guideline numbers, and warns above one with the step that clears
  it: move detail behind a trigger. The one size that still fails is the
  whole generated `AGENTS.md` with a realistic project section against
  Codex's 32 KiB instruction limit. Passages that earlier byte budgets had
  cut are restored where they are read: the turn lifecycle adapter now loads
  on a Grok or other non-Claude host that launches Claude, not only on Codex;
  `cf-herdr` again says the dangerous-permissions flag needs the operator to
  name it, that Herdr is not a sandbox, and how agent names look; the Grok
  host detail is read before a Grok launch; and the git reference explains
  arming the remote plane. The unreferenced `cf-present` example
  `assets/review-document.example.json` is retired; `codeflow update` removes
  an unmodified copy.

<!-- codeflow:release-impact minor -->
- **A short rule map replaces the long root contract.** Every tier's
  managed `AGENTS.md` block is now a map of about 12 KB (was 28.7 KB at
  standard and full, 16.4 KB at minimal), rendered with `CLAUDE.md` from one
  kernel. It opens with one-line always rules, each with a pointer: routing
  by touched paths comes first, and they include planning once at the
  breakdown, independent review with no round cap, the enforced git floor
  and that only the operator adds process. Two "when you are about to"
  tables follow, delivery in the order work moves and situations, 26 moments
  in all; each row names an inline action, a skill or a file, and a
  `MUST OPEN` pointer is read before acting and says why. `CLAUDE.md` opens
  with the routing gate. The doctrine moved unchanged in substance to four
  references installed at every tier under `.codeflow/rules/` (workflow
  discipline, git rules, worktrees, writing). The full tier gets its own
  map, which alone names `project-management/`. The block stays under a
  12 KiB guideline so a project section fits within Codex's 32 KiB limit.
  `codeflow update` replaces the managed block and keeps the project section
  byte for byte, CRLF line breaks and a missing final newline included;
  `doctor` gains an `instructions` check that warns when the `AGENTS.md`
  chain Codex loads for any directory, root to nested, passes its 32 KiB
  limit. Map rows print skill references as paths from the repository root
  (`.agents/skills/...`). Migration: `codeflow update` never edits the
  project section, so a project section that cites the old section names
  ("Git rules", "Worktree doctrine", "Workflow discipline", "Entry points",
  "Planning and tracking", "Session flow") should point at
  `.codeflow/rules/git-rules.md`, `worktrees.md` or
  `workflow-discipline.md`, or at the map, instead.

<!-- codeflow:release-impact minor -->
- **Planning checked once, at a level you set.** The planning checks (a
  valid workgraph, a record for the task the branch carries, and that
  record anchored on its target) run once per task: at `codeflow work
  start` and in `codeflow ci`, on every work prefix, never in the
  pre-commit hook. The new `git.work_planning` key sets their level:
  `block` (the default) or `warn`, which reports the finding and lets the
  work continue; there is no `off`. `codeflow update` adds the key and
  keeps a value you set. An undeterminable tracking state and pull request
  classification still block.

<!-- codeflow:release-impact minor -->
- **PR body and spec checks scaled to the change.** A pull request whose
  range changes only Markdown under `docs/` or `project-management/` needs
  just Summary and Changes, under your mapped headings where you accepted a
  mapping. Release impact is required only on a pull request into a
  protected branch or one whose range carries a breaking commit; elsewhere
  it is optional and checked when present. The target is read from `--into`,
  the host's pull request target variable on GitHub, GitLab or Bitbucket, or
  a named base. A path in your product or watched contract
  paths, a shipped template, the record templates, a dependency manifest,
  an instruction tree or any other file keeps every configured section, and
  the range is read from one tree diff that counts merge resolutions. The
  PR template says which sections each kind of change needs. Spec approval
  now reads a new `open_questions` frontmatter list and needs it present
  and empty, in place of guessing from the words under `## Open questions`;
  the prose stays as context. Existing specs, approved ones included, stay
  valid, and `codeflow update` adds `open_questions: []` to an unmodified
  spec template. Migration: before approving a spec written without the
  field, add one line, `open_questions: []`, or list the questions its
  prose still leaves open; a draft without the line is not approved.

<!-- codeflow:release-impact patch -->
- **Smaller per-task reading.** The duo quality contract and the
  capability-routing resource are now indexes: each links the sections read
  on every task and the sections read only when a named trigger fires, such
  as a UI change, a red gate or a route qualification. cf-delegate is a
  common core plus one file per lane, so a Claude host reads the plugin lane
  and a Codex host the lifecycle lane, and the Claude turn lifecycle adapter
  is read only on a Codex host. cf-model-orchestrator keeps what every task
  needs and moves trigger-only guidance, such as project model overrides
  and parallel tasks, to references. Duplicated rules now live
  in one place with pointers from the others. `codeflow update` replaces the
  old whole files and installs the new section files at the standard and
  full tiers. cf-ship's PR evidence now carries the release-impact rules
  every PR needs, and the full release policy is read for a minor, major or
  disputed impact, release preparation, or publication. A new test walks
  the per-task reading chain from its entry points and caps it at 148 KiB,
  down from about 210 KiB.

<!-- codeflow:release-impact patch -->
- **Work start past a stale local target.** `codeflow work start` and the
  `codeflow ci` work-start and classification checks resolve a
  task's integration target, such as `main`, to its local branch when one
  exists. When that branch is strictly behind the upstream Git has
  configured for it (`main@{upstream}`), they now anchor on the upstream and
  print a note saying so, instead of refusing a task whose planning record
  landed upstream but was never pulled. When the two have diverged, they
  refuse and name both sides; reconcile the local branch, or name the ref
  with `codeflow work start --into`. A local branch that is equal, only
  ahead or has no configured upstream is still used, and another remote's
  branch of the same name, such as a fork's `origin/main`, never replaces
  it. Without a local branch, `origin/<name>` is used as before.

<!-- codeflow:release-impact minor -->
- **`human_authorization` is deprecated.** The top-level policy key accepted
  only `none` and changed nothing, so fresh installs no longer write it. A
  policy file that still has it loads, and validation, `codeflow ci` and the
  commit-msg hook print one line: `policy key human_authorization is
  deprecated and ignored`. `codeflow update` removes it and notes `removed
  deprecated key human_authorization`; no other value changes. The unwired
  v1 scanner modules behind no guard are removed too (ADR-0008 and ADR-0009
  amendments); nothing an adopter configures changes.

<!-- codeflow:release-impact patch -->
- **Quieter and narrower guards in a sandbox.** Hook entry points, `codeflow
  ci` and the read-only checks (`validate`, `work`, `estimate`) no longer
  record the repository in `~/.codeflow/registry.json`, and a registry
  this process may not write, as in a sandbox, is skipped without the
  `registry touch failed` warning. exec-guard allows recursive
  removal that really lands below a temp root (`/tmp`, `/var/tmp` and the
  macOS per-user `/var/folders/<xx>/<id>/T`, each also under `/private`),
  judged after following symlinks; the roots, the configured `$TMPDIR`
  itself, a link that leads out and every system directory stay blocked.

<!-- codeflow:release-impact major -->
- **Effort default on upgrade.** Version 2.1.0 set no reasoning effort, so
  Claude Code and Codex used their own defaults. After `codeflow update`,
  both start at high: update adds `"effortLevel": "high"` to
  `.claude/settings.json` when the key is absent, keeping any value you
  already set, and sets `model_reasoning_effort = "high"` in
  `.codex/config.toml`. To keep a lower default, set those two keys to the
  level you want after updating and commit both files; later updates keep
  your `effortLevel`, keep your `.codex/config.toml` whole when the shipped
  file is unchanged, and three-way merge your edit when it changed (on a
  conflict they write `.codex/config.toml.new` and leave yours). Two other
  changes are marked breaking in their commits but need no step from 2.1.0:
  the version 4 model ensemble file is new, and `codeflow update` installs
  it; the rule that rejects obsolete raw-text elements such as `<xmp>` in
  presentations applies to `codeflow present`, which is also new here.

<!-- codeflow:release-impact major -->
- **Pull request classification and light planning paths.** Every pull
  request names its work on a `Task:` line, and `codeflow ci` refuses one
  whose line is missing, empty, `none`, a template placeholder or a
  malformed id. With durable work tracking on, the line gives the pull
  request one class: tracked (`Task: TSK-NNN`), planning-only
  (`Task: EPC-NNN` with only records and plans in the range) or an epic's
  integration line landing on its target (`Task: EPC-NNN` on that verified
  `integration/` branch). A task branch names its own task. The range is
  read as one diff from the merge-base, and tracking is read at the target
  as well as the head, so a pull request cannot classify itself lighter. A
  task pull request may add one record, its own: a standalone task lands
  its record and its code in one reviewed pull request, and other new
  records go in the epic's planning pull request. A spike lands only
  `docs/research/` findings and its own record. With tracking off, the line
  names the harness's tracked unit, any name that is not a placeholder. A
  trusted automation profile's new `task` field supplies the line a bot's
  body leaves out. The planning anchor check runs at `work start` and in CI
  on every work prefix carrying a task id (`task/`, `fix/`, `feat/`,
  `spike/` and the rest), not on every commit. `git.product_paths` names
  the project's product code for the pull request checks: `init` writes a
  default for the detected stack and `update` adds it once, keeping any
  project value. `git.direct_changes` is retired: a policy that sets it
  still loads, and `doctor` names it deprecated. New
  `task new --follow-up-of`, `epic new --integration` and `adr new`
  (written `proposed`, the ADR template's new default). The shipped pull
  request template carries the `Task:` line with a hint. Migration: add a
  `Task:` line to every pull request body; a branch name alone no longer
  classifies a pull request, and `Task: none: <reason>` is refused. Work
  that had no task becomes a standalone task
  (`codeflow task new --standalone-reason <why>`), whose record lands with
  its code. Add a `task` to each `git.automation_profiles` entry, such as
  `"task": "dependency update"`, or that bot's pull requests are refused.
  No policy key restores the old behavior; classification is off only
  where durable work tracking is off.

<!-- codeflow:release-impact minor -->
- **A pre-push gate under a minute that blocks.** Public behaviour change:
  the pre-push hook no longer runs the test suite. It runs the push set and
  blocks on what it can see, leaving the rest to CI. `codeflow ci` runs on
  each pushed branch's range, leaving out only history known to be on the
  destination: the branch and tag tips the push location advertises now
  (one `git ls-remote`, which never prompts, gives up after 10 seconds and
  downloads nothing, even in a partial clone), plus the commit it
  advertises for an existing branch. A branch cut from an integration line,
  or rebased onto one and force-pushed, is checked for its own commits, and
  a rewrite notes how many commits are checked. For an existing branch,
  the work-record check reads `work_records_baseline` from that branch's
  current tip on the destination, not from the range's base, and prints
  the notice naming each entry a push introduces. When the destination
  cannot be asked, the hook says why: an existing branch is then bounded
  by its advertised commit alone, so after a rebase the range also holds
  the commits the rebase brought in, and a new branch by the tracking refs
  of its protected branches, which policy keeps from being rewritten, but
  only when the remote fetches from the location pushed to. Other tracking
  refs may be stale and never shrink the range. When nothing gives a base,
  the hook reports the range unresolved and never compares with a local
  branch. `codeflow validate
  --docs` and the `.codeflow/test-config.json` targets that define a `quick`
  mode run only when the pushed commit is the checked-out one, with no
  tracked changes, no sparse checkout and every submodule initialized at its
  recorded commit; otherwise the hook says they did not run and CI runs them.
  They read the working checkout, so untracked files there can influence a
  quick target, and the pass line says so. Pre-push no longer falls back to `essential`, so the suite
  stays in the full gate and CI. When the whole push set takes longer than 60
  seconds, the hook names its slowest step, and for a test-config target the
  `modes.quick` key that moves it out. The Rust and Go test-config templates
  add a lint target in the push set. Fresh installs at every tier set
  `git.test_gate_on_push` to `block`, and so does the built-in default, which
  applies to a policy file that omits the key. `codeflow update` keeps the
  value an existing install has and prints one line recommending `block`. To
  keep the advisory push, set `"test_gate_on_push": "warn"` under `git` in
  `.codeflow/policy.json`.

<!-- codeflow:release-impact minor -->
- **Every warning names the step that clears it.** Public behaviour change:
  with `git.test_gate_on_push` at `warn`, a push whose `codeflow ci` finds an
  always-blocking rule (such as the id registry) or a rule the project set
  to `block` is now stopped; before, the hook printed `BLOCKED` and let the
  push through. To let such a push through again, set that rule to `warn`
  in `.codeflow/policy.json`; an always-blocking rule stays blocking. Other
  findings print at the push gate's level, and the closing line of each
  hook says whether the commit or push was stopped.
  Each warning and note from `codeflow ci`, `validate --docs`, `doctor`, the
  git hooks and the session guards now names the step that clears it: a
  `codeflow` command, a named `git` command or a file edit; a test fails on
  one printed without. `validate --docs` no longer warns for an approved spec
  whose consumers are all accepted, its healthy derived `implemented` state,
  and prints no note for a layer the project's tier does not install.
  `doctor` reads Codex hook trust from `~/.codex/config.toml` (or
  `$CODEX_HOME`) and Grok folder trust from `~/.grok/trusted_folders.toml`
  (or `$GROK_HOME`). A static reading proves only that a hook does not run:
  a Codex hook that is untrusted, disabled or changed (hashed as Codex
  normalizes it), a folder Grok does not trust, or a Grok store it cannot
  read is a warning. A configuration that matches is a note, "configured;
  runtime not verified", naming the real hook event that verifies it;
  doctor never reports these hooks as running. Where doctor cannot
  reproduce the harness's decision (a matcher Codex rejects, an empty
  command, a linked worktree whose hooks Codex takes from the main
  checkout, Grok `version_overrides`, a relative `GROK_HOME`, a
  Grok-managed worktree) the note says it cannot verify it.
  `doctor --check hooks` warns when another hook manager's hook is missing,
  not executable or names no codeflow shim outside a comment; when every
  hook is executable and names its shim, it prints a note, "wiring not
  verified", naming the commit with a bad subject that confirms the calls
  run, since reading a hook cannot show that it runs the shim.
  `doctor --check delegates` gives what this machine installs its own step,
  apart from the Codex sign-in. The session summary names the path its
  ledger write failed on and the repair that path needs, and outside a git
  repository records nothing instead of warning. A guard input that is not
  a JSON hook payload, or whose `tool_name`, `tool_input`, `command` or
  `cwd` has the wrong type, names the field and the harness hook entry to
  repair. A commit on
  a `git.breaking_watch_paths` surface now prints a note, not a warning,
  pointing at the pull request's Release impact, and `codeflow ci` and
  `scripts/release.py` given a body that states `Breaking: no` with a
  `Rationale`, outside code and quotes, report nothing for it.
  `scripts/release.py` reads a pull request body through the `codeflow`
  binary its caller names (`--codeflow-bin` or `CODEFLOW_BIN`), with the
  parser `codeflow ci` uses, so the two cannot read a body differently; it
  never takes a `codeflow` from `PATH`. The pre-push preflight passes the
  `codeflow` running the hook, and the release impact job builds one from
  the checked-out tree. A project that runs `release.py check-pr` itself
  passes a `codeflow` built from its tree. The reader's answer carries a
  protocol version, and `release.py` refuses a binary that answers another.
  `codeflow ci` accepts the legacy `Contract` field as `release.py` does,
  alone or agreeing with `Breaking`, so the two no longer disagree on it.

<!-- codeflow:release-impact minor -->
- **A ceremony report with a recorded baseline.** `codeflow report ceremony
  --prs FIRST..LAST` (or `--since DATE [--until DATE]`) reports the process
  cost of the merged pull requests in a window: pull requests per logical
  change, where a task's pull requests count once and a pull request that
  only moves a record's status, acceptance evidence or closeout has its own
  row; review rounds per pull request; and refusals hit by the clone's hooks
  and guards. Review rounds are asked of the host through `gh`, the one read
  command that may use the network; when that call fails, or the host holds
  no submitted review, the report prints `unknown` and never estimates. Each
  operation a git hook or session guard stops now appends a `refusal` event
  to `.git/codeflow/ledger/refusals/`, naming the plane, the level and the
  rules, never the command; a warning is not recorded, and refusals from
  before a clone began recording print `unknown`. `codeflow status` and the
  work reads stay offline. Nothing to do on upgrade.

<!-- codeflow:release-impact minor -->
- **One full gate at a time, running the suite once.** Public behaviour
  change: `codeflow test --mode full` takes a gate lock before any target
  runs, and a second full gate on the machine refuses, naming the holder's
  pid, directory and start time. A killed gate's lock stays held while the
  targets it started are still running, and is reclaimed once they exit.
  The locks are `locks/full-gate.lock` under the CodeFlow home
  (`CODEFLOW_HOME`, else `~/.codeflow`), which spans the machine, and
  `codeflow/full-gate.lock` in the repository's git common directory, which
  spans its worktrees. A lock that cannot be opened or taken, as in a
  sandbox or a read-only home, refuses the run with `gate lock
  unavailable`, naming the path and the error; fix the permissions or the
  sandbox rather than running unguarded. Quick and essential runs take no
  lock. In `.codeflow/test-config.json` a target may declare `requires`
  (prerequisites that pass first), `outputs`, `narrow` (the inputs that
  select it) and `exclusive` (it runs alone), and `execution` gains
  `max_parallel` and `run_everything`. One coordinator runs targets with no
  prerequisite relation in parallel up to that bound and never starts a
  target whose prerequisite failed. `codeflow test --since <base>` skips a
  target only when its declared inputs are unchanged against a base with a
  recorded green full run under the same configuration; an unproven base, a
  rename or deletion, an input no target declares or a change under
  `run_everything` runs every target, and `--all` runs every target,
  including the binary determinism check at epic close. A full run keeps
  its evidence under `gate-runs/` in the CodeFlow home. A CI run that skipped
  a `ci_skip` target is recorded as incomplete, so a local `--since` run
  never takes it as a green base and runs those targets. In every mode, a
  gate that runs cargo warns when `CARGO_TARGET_DIR` points outside the
  worktree, naming the shared directory: builds in parallel worktrees can
  overwrite each other's binaries there. The gate still runs; a shared
  directory to save disk stays valid. Each target prints `[codeflow test]
  starting target '<name>' (<mode> mode)` on stderr as it starts, so a
  killed gate's log names the target it died in; stdout, the summary lines
  and the exit codes are unchanged, and targets skipped by `enabled` or
  `ci_skip` print nothing. CodeFlow's own full gate runs the Rust suite
  once, under coverage, and its journey check reads that run;
  `cargo test --workspace --doc` still runs, since coverage skips it.

<!-- codeflow:release-impact patch -->
- **Same-family workers run inside the host harness.** cf-model-orchestrator
  and its capability-routing resource tell a primary to run a same-family
  worker as a native subagent of its own session, in the desktop app and the
  CLI alike, and never as a separate CLI session or Herdr tab. A Claude host
  launches Fable or Opus through Claude Code's Agent tool. A Codex or Grok
  host follows the same rule once native evidence shows a subagent route for
  that model; until then the primary keeps the unit and records the missing
  route. The model evaluation suite adds requirement CF-MM-019, the
  `same-family-worker-runs-as-native-subagent` case for Claude hosts and the
  `same-family-worker-falls-back-without-native-route` case for Codex and
  Grok hosts. A case can now name the host lineages it applies to, and the
  canary and full suites select it only for those hosts.

<!-- codeflow:release-impact patch -->
- **Operating doctrine follow-through.** After opening a pull request,
  cf-ship polls its required checks at most once a minute for up to thirty
  minutes, repairs assertion-red checks without being asked, reports
  infrastructure-incomplete checks as missing evidence, never merges, and
  sends one readiness report with the printed PR URL. Operator replies carry
  a figure when the point is a relationship and give only printed or
  verified links; mannered prose and bare-identifier titles are editorial
  defects. The evaluation kit adds an `operating-doctrine` pack.

<!-- codeflow:release-impact patch -->
- **Summary shape and reply figures.** A pull request body, report or reply
  opens with a summary that anchors the reader in a few lines: what this is,
  why it matters and where it stands, with a key number, file name or caveat
  where it is part of that context. Every detail follows as bullets in a
  logical order. A reply figure matches its surface:
  an inline HTML figure where the harness renders one, a `cf-present` page
  when it needs a full page, fenced ASCII on a terminal or other plain-text
  surface, and never Mermaid. The operating-doctrine evaluation cases grade
  both with faulty controls.

<!-- codeflow:release-impact minor -->
- **Pull request template.** The shipped template opens with the `Task:`
  line (`TSK-NNN | EPC-NNN | <unit name>`) and has five sections (Summary,
  Changes, Testing, Reviews, Release impact) with short comments, and lists
  its conditional sections with the exact condition for each: Testing is
  left out of a range of only documentation files, and Release impact is
  required on a pull request into a protected branch or with a breaking
  commit. The Release impact block states `Breaking: yes | no` and always
  carries `Migration`. Existing policies are unchanged: the required
  headings are still the project's own list.

<!-- codeflow:release-impact patch -->
- **Claude Code preset prompts.** The shipped preset sets no permission mode
  in its default file and prompts for rooted or home-anchored recursive
  deletes and for force branch deletes spelled `-f`, `--force`, `-df`, `-fd`,
  `-Df`, `-fD`, `-qf` or `-fq` as the first option after `git branch`; other
  clusters and options placed before the delete flag are outside what prefix
  globs can express. The git-guard hook now recognizes a clustered delete flag
  such as `-Dq` on a protected branch, and a clustered in-place edit flag such
  as `sed -ni` on a policy or hook file, both of which it previously missed.
  Existing installations keep their permission entries and values, because
  update merges permission arrays as a union and leaves scalars alone.

<!-- codeflow:release-impact patch -->
- **Committed portal directory links.** Repository guides preserve relative
  links to committed directories as exact-source provider tree URLs while
  retaining visible provenance on unknown providers and fail-closed path rules.
<!-- codeflow:release-impact patch -->
- **Locally usable repository guide.** README and the adoption guide document
  the locked install, check, build, preview, and validate path for the derived
  `docs-portal/` guide from a clean checkout on Node 24.18.0, distinguish the
  pending 3.0.0 source from the published v2.1.0 release, and state the actual
  CI Node roles. The dogfood portal configuration replaces the pending release
  label with the exact built-from commit, and the release checklist gates
  advertising the private vulnerability-reporting channel on a visibly
  verified route.

<!-- codeflow:release-impact patch -->
- **Documentation portal dependency security.** The bundled portal pins
  devalue 5.9.4, removing GHSA-9rgm-9g3h-6x36 while preserving the existing
  starter identity, managed ownership contract and static-site behavior.

<!-- codeflow:release-impact patch -->
- **Responsible autonomy contract.** Managed instructions now bind
  consequential effects to explicit authority, minimize private data, preserve
  ordinary authorized work, and stop only the affected risky lane when
  authority or a required control is unclear. Detailed quality, security,
  delegation, customization, and exec-guard guidance distinguishes task
  authority from harness enforcement without changing default policy.

<!-- codeflow:release-impact patch -->
- **Workflow verification and guard boundaries.** Catalog and workflow checks
  cover the shipped instruction surface. Prepared reference transactions fail
  closed when an active local check cannot read or evaluate its input; docs
  distinguish installed local safeguards from actual remote enforcement and
  preserve the qualified interactive-only delegation policy. The README now
  states the proportional workflow and hook-conditional recall boundaries
  without unsupported comparisons to other development approaches.

<!-- codeflow:release-impact patch -->
- **Opt-in Claude context policy.** Repository-owned settings select a
  1M window and 50-percent compaction target. Customization explains effective
  window limits, consumer overrides and manual-versus-automatic evidence;
  generic settings presets and immutable delegated-run settings are unchanged.

<!-- codeflow:release-impact patch -->
- **Instruction workflow consistency.** Required stage references preserve
  planning, safety and review duties while clarifying mature-task reuse,
  docs-only recovery, qualified worker fallback and meaningful type/runtime
  boundary checks. Mandatory project gates remain required for every change.
  Lifecycle-tracked Claude launches keep child reviews synchronous through a
  process-local setting; ordinary sessions and stored run bindings are unchanged.

<!-- codeflow:release-impact patch -->
- **Release migration detection.** Historical `[Unreleased]` comparison links
  no longer cause existing pending notes to be counted as new PR changes.
  Migration handling still requires an actual unreleased section heading.

<!-- codeflow:release-impact minor -->
- **Same-work-PR release state (ADR-0062).** Normal work PRs now carry the
  curated pending note, impact annotation, and cumulative version stamps they
  require. Read-only host discovery advances from the latest verified public
  release, while cargo-dist remains the sole explicitly dispatched publisher;
  the candidate branch, follow-up release PR, and git-cliff authority are
  retired without moving the historical v2.1 tag.

<!-- codeflow:release-impact patch -->
- **Git-guard reads data as data.** The in-session git-guard no longer blocks
  reading the hook path (`git config core.hooksPath`, `--get`, `--list`,
  `--show-origin`); it still blocks every write form in every scope, and now
  also `git config --edit` and removing the `core` section. Heredoc bodies,
  comments and quoted text are no longer read as commands when only data
  tools such as `cat`, `git commit -F -` or `gh pr create` read them; a body
  or substitution that a shell or any other program can run is still checked.
  A `cd` or `-C` chain that switches to a new branch before committing is
  judged on that branch.

<!-- codeflow:release-impact patch -->
- **Delegate turns accept a pasted prompt.** Claude Code submits a long or
  multi-line pasted prompt inside a `<pasted_content id="N">` envelope with a
  per-session id of four lowercase hex digits, and tells the model to act on
  pasted text only where the user's own words say so. The delegate-turn hook
  used to reject that envelope as a digest mismatch, and a bare paste could be
  refused by the model. The delivering host now types one fixed sentence,
  `Carry out the pasted instructions.`, after the paste. The hook accepts the
  prompt when its bytes match exactly, or when it is exactly the envelope
  Claude Code submits around the armed bytes followed by that sentence, and
  `accepted.json` records which delivery matched. A bare envelope, another
  sentence, extra text, or any other prefix, id or shape still fails.

<!-- codeflow:release-impact patch -->
- **Portal writes work on Windows.** Every `codeflow portal` write on Windows
  failed with "The parameter is incorrect" (or a length error for
  one-character names) when it moved a staged file into place. The rename
  now goes straight to the kernel with the name resolved under the held
  destination directory and a correctly sized request, so it no longer
  depends on the process working directory. Reparse points are still
  refused and no path outside the portal root is opened.

<!-- codeflow:release-impact patch -->
- **Delegate turns survive backgrounded work.** When a delegated session
  backgrounds a Workflow or a Bash command, Claude Code reports its end with a
  task notice after the turn stops, and the delegate-turn hook used to block
  that notice as an unarmed prompt. The hook now admits exactly one notice
  envelope as a continuation of the current turn when the session transcript
  shows that turn launched the task, records it under the turn's
  `continuations/`, and closes it with the Stop that follows instead of
  poisoning the run. Unknown or earlier tasks, extra text and second
  envelopes are still blocked. That Stop first checks the session transcript:
  the notice must be recorded as a Claude Code task notice with the admitted
  bytes, and a typed copy or changed body poisons the run with no result.
  The model may still act on a forged notice within that turn; the check only
  keeps it from being recorded as a clean result.

<!-- codeflow:release-impact patch -->
- **Scaffolded CI leaves the Node 20 action runtime and pins its runner.**
  The GitHub workflow that `codeflow init` writes to
  `.github/workflows/codeflow-ci.yml` now uses `actions/checkout@v6`, which
  runs on Node 24, instead of `actions/checkout@v4`, which runs on the
  deprecated Node 20 runtime. Its four jobs run on `ubuntu-24.04` instead of
  `ubuntu-latest`, so GitHub moving that label to Ubuntu 26 on 2026-10-19
  does not change the tools under them; the jobs check the same things. An
  unmodified copy is replaced by `codeflow update`. An edited copy is merged
  three ways; on a conflict your file is left unchanged with the new version
  beside it as `codeflow-ci.yml.new`, so change each `actions/checkout@v4`
  to `@v6` and each `runs-on: ubuntu-latest` to `ubuntu-24.04` by hand. A
  self-hosted runner needs Actions runner 2.327.1 or later for Node 24
  actions.

<!-- codeflow:release-impact minor -->
- **Every writing surface states the plain-writing rule.** The managed
  `AGENTS.md` block gains a "Write plainly." always rule at every tier:
  everything an agent writes, replies and status updates included, is
  simple, straightforward and clear, with no mannered prose. The writing
  reference `.codeflow/rules/writing.md` leads with the same rule and a
  default of short prose and bullets, and names the figure form for each
  surface, including fenced ASCII in Markdown files. Each skill, the
  `cf-reviewer` agent, the git rules and the PR template state the rule
  once where they tell the agent to write, and the reply rule names
  Markdown files for fenced ASCII figures. The size guidelines `codeflow
  doctor` reports for five of those files rise by at most 768 bytes, and
  the map's rule count and rule line guidelines rise to 13 and 480 bytes. `codeflow update` brings the new text and leaves a project's own
  section of `AGENTS.md` untouched.

### Fixed

<!-- codeflow:release-impact patch -->
- **Evaluation trial repairs.** Repair fixtures and their planning, cleanup
  and PR-check guidance. The repository qualification runner now uses isolated trial
  homes, records native launch flags, guards prompt delivery and reports
  changes in declared directories with explicit observation limits.

<!-- codeflow:release-impact minor -->
- **Fix completed work in one PR.** A task can reopen with its old review
  preserved and a reason, carry the fix, and complete again with a review
  inside the same PR. The shared structural judge rejects copied or stale
  reviews, changed criteria and damaged reopen history. Clean task landings
  and verified release imports retain their source review; the separate
  planning-reopen path remains valid.

<!-- codeflow:release-impact patch -->
- **Agent-session refusal coverage.** Exec-guard and git-guard apply the
  existing action-family policy to parsed wrappers, leading flags,
  interpreter forms and tag-push spellings. Git-guard refuses covered
  discards of local-only work. Codex `apply_patch` and Grok `write` and
  `search_replace` pass enforcement-path edits through the new
  `codeflow hook edit-guard`; ordinary edits remain available. Refusals
  name the rule and the operator's route. These command and payload
  checks do not inspect opaque child programs.
  - Relief stays with the underlying action's policy level and the
    harness's independent native deny rules. `security.headless_peer_runs`
    is the sole policy-level relief for a headless peer run; the unread
    `security.headless_opt_in` structure remains accepted but ignored,
    with a warning and removal on `codeflow update`. The built-in
    catastrophic-command floor is unchanged.
  - ADR-0075 drops the proposed readiness receipts, external enforcement
    baseline, operator-actions queue and fixture-root admission. A
    primary's permitted sandbox retry is judged by native permissions
    and the guards; delegated seats deny the retry natively. Herdr's
    named list remains workspace practice. Landed-policy authority and
    fail-closed hooks are separate TSK-189 work.

<!-- codeflow:release-impact patch -->
- **A Codex seat under `cf-guard` can delete files and build.** The
  profile's secret-file denies (`.env`, `.env.*`, `*.pem`, `*.key`,
  `*.p12`, `*.pfx`, `.netrc`, `id_rsa*`, `id_ed25519*`) now apply at the
  workspace root only. The `**/` forms made Codex deny deleting and
  renaming every directory, so `rmdir`, `cargo build` and `npm` builds
  failed. Nested secret files, such as `sub/.env` or a linked worktree's
  `.env` under `.worktrees/`, are no longer denied, so a seat can read,
  change or delete them; the config comment records that gap. The network is unchanged: no unix sockets and no local
  binding. Reviewer seats launch with `--ask-for-approval never` and no
  `--sandbox` flag, which selects `cf-guard`; builder seats keep full
  access, because the `cf-builder` spike did not pass (a push to a remote
  outside the workspace root is denied). Tested on Codex 0.159.1. On macOS,
  Playwright's Chromium did not start under the sandboxed profiles probed
  (`cf-guard`, `cf-builder` and `:workspace`), since the sandbox denies its
  Mach port rendezvous; it starts unsandboxed, so a full-access builder can
  run browser tests (TSK-190).

<!-- codeflow:release-impact patch -->
- **`codeflow ci` accepts a workspace's root branch.** In workspace mode,
  at every tier, a range on the branch `git.root_branch` names, such as
  `integration/workspace`, is classified as the workspace root branch, the
  way a verified epic line is: it needs no `Task:` line and may change task
  criteria. Before, `ci` refused it as an unverified epic line and blocked
  any criteria change on it. The branch is read from the policy on the
  target. With durable tracking, a pull request from any other
  `integration/*` branch that is neither a verified epic line nor a release
  branch is refused whatever its `Task:` line, and also when the host
  supplies no body (a Bitbucket description `ci` cannot read); before,
  `Task: TSK-NNN` or a missing body let one through. A plain push with no
  pull request context, the minimal tier, and a range into a release
  branch, which the release checks judge, are not judged this way
  (TSK-190).

<!-- codeflow:release-impact patch -->
- **An approved spec is amended until it ships, and frozen after.** The
  lifecycle guidance, the spec template and the refusal of an approved spec
  moved back to `draft` now agree with how specs change in practice. While
  a spec is approved and not yet implemented, a change to it is amended in
  place through a reviewed planning change, with a dated note for each
  change of meaning and each bound consumer's disposition named; once
  implemented it is frozen and a change is a new spec. They no longer say
  that approval freezes the criteria. The documented freeze is now
  checked: `validate --docs --since`, `codeflow ci` and the pre-push hook
  refuse a change to the text of a spec that was ever implemented, also
  after a later supersession or consumer reopen.
  `codeflow spec status <id> draft` gives the refusal naming both routes
  instead of an argument error. `codeflow update` brings the changed
  guidance and template.

<!-- codeflow:release-impact patch -->
- **The secret scan reads the index a commit records (security).** `git
  commit -a` and `git commit <path>` record a temporary index that git names
  in `GIT_INDEX_FILE`. The pre-commit secret scan read the ordinary index
  instead, so a key in a changed tracked file that was not staged first was
  committed without a finding. The scan now reads the index the commit
  records, and fails closed when it cannot read it.

<!-- codeflow:release-impact patch -->
- **doctor claims the CI pin only for a shipped install.** The
  `ci-perimeter` check reported that CI installs the version the target
  pins, verified against its `sha256.sum`, for any CI file that mentioned
  `scaffold_version`, even in a comment. It now makes that claim only when
  the file carries a shipped template's target-pinned install unchanged; a
  comment, a pin read from the head, a removed checksum check or an edited
  install is reported as one doctor cannot verify.

<!-- codeflow:release-impact patch -->
- **Pre-push landing base.** Fast-forwards of protected and integration
  branches check from their advertised tip. Branches with a declared task
  target check from the merge base with that target's advertised tip and
  name the target, preserving every commit the destination lacks in the
  checked range. Branches without a declared target keep the existing
  advertised-history check. A landing from another line now checks that
  line's records against the receiving line's baseline. A legacy record
  missing from that baseline can block the push, as it would in PR CI.

<!-- codeflow:release-impact patch -->
- **Herdr delivery confirms a started turn.** `cf-herdr` delivers through
  `scripts/deliver.py`. For a Codex or Grok seat it sends one Enter and
  confirms within 20 s that the seat started working, and otherwise reports
  the turn not confirmed, naming the pane; it never sends a second Enter,
  since Herdr cannot tell a prompt in the input from one already submitted.
  It sends nothing to a seat that is working, blocked or of unknown status,
  or whose working folder is gone, where it names the relaunch step. The
  Claude lane adds the fold sentence only for a fold its own paste made.
  Cleanup keeps a worktree
  that a live seat uses. The Claude lane keeps its `accepted` wait. Run
  `codeflow update` to install the script.

<!-- codeflow:release-impact patch -->
- **The delegated Claude turn rules are read once, before launch.** On a
  Codex host, the `cf-delegate` lifecycle lane restated the turn adapter's
  launch sequence, turn detection and sibling Stop-hook preflight, and its
  copy ran the preflight before delivery, after the session had loaded its
  hooks. The lane now sends the reader to the adapter before launch, and
  the adapter states each rule once, with the preflight after `init` and
  before launch. A contract test fails when the lane states a rule again.
  `codeflow update` brings both files at the standard and full tiers.

<!-- codeflow:release-impact patch -->
- **The writing reference carries every reply duty.** The rule map sends an
  agent about to report to `.codeflow/rules/writing.md`, the only reply
  guidance a minimal-tier project installs. It now states each duty of the
  lifecycle reply rule: the running report on long work, labels forced onto
  a short answer, a summary that buries its anchor, a hard gate that waits
  on the operator while other work keeps moving, and no manufactured ask.
  A contract test fails when the two drift apart. `codeflow update` brings
  the reference at every tier.

<!-- codeflow:release-impact patch -->
- **Work reads survive a partial clone and refuse an oversized record.**
  `work next`, `work claim`, `work start`, `status` and `codeflow ci` read
  the records on a branch tip from `project-management/` only, so a clone
  that lacks unrelated trees (a treeless partial clone) still reads its
  backlog instead of failing. A record on a tip larger than 4 MiB is refused
  by path before it is read; the largest real record is about 100 KiB.

<!-- codeflow:release-impact patch -->
- **Work reads stay fast with thousands of stale branches.** `work next`,
  `status` and `orient` read each task record once, and rule out in-process
  the stale branches that cannot have landed, starting `git cherry` only for
  the rest; a landing is still proven by `git cherry` alone. On 10,000
  records with 3,000 stale task branches, `status` went from 358 s and 6,012
  git processes to 49 s and 500, and `orient` from 281 s to 42 s.

<!-- codeflow:release-impact patch -->
- **The guide reads every dependency form a task record accepts.** A task
  that depends on a research or decision input, written
  `{id: TSK-NNN, kind: research, pin: "<commit sha>"}` or
  `{id: TSK-NNN, kind: decision}`, builds as a normal portal page that shows
  the dependency, not a "Source unavailable" page. A task record that still
  uses the legacy `dependencies` key shows its dependencies too; any other
  page keeps its own meaning for that key. The portal generator and
  `codeflow validate --portal` read the same forms. Like `validate --docs`,
  both refuse a task record that carries `depends_on` and `dependencies`
  together, or whose dependency names something other than a task. Run
  `codeflow portal setup --path <dir>` to take the fix into an installed
  portal.

<!-- codeflow:release-impact patch -->
- **Faster `init` and `update` on macOS.** Scaffold writes no longer flush
  the whole disk cache for every file. Each file is still written to a temp
  copy, synced and renamed into place; on macOS the sync is a write barrier
  that keeps the data ahead of the rename, so an interrupted run or a power
  cut leaves every file whole. A directory is synced before any baseline,
  manifest or project state that records its files, and when those files
  sit on another disk than the record, that disk is flushed first, so a
  record never survives a crash ahead of them. With the project on one
  disk, the usual case, macOS gets one full disk flush per run instead of
  about four per file; a disk that gets new writes after its flush is
  flushed again before the next record on another disk. A symlinked
  manifest or `project.toml` is refused, as a symlinked baseline already
  was. A standard init made
  about 1,000 such flushes, most of its wall time. The installed files are
  unchanged.

<!-- codeflow:release-impact patch -->
- **`git gc` works in a hooked clone.** The reference-transaction hook no
  longer refuses `git pack-refs`, which `git gc` and auto gc run: moving a
  protected branch such as `main` from a loose ref into packed-refs, and
  pruning the loose copy, leave it on the same commit. A transaction line
  passes only when the ref keeps its current value; a prune passes only
  while packed-refs holds that value and is not being rewritten. A real
  move or deletion of a protected branch still blocks.

<!-- codeflow:release-impact patch -->
- **git-guard judges the repository a command targets.** A git command that
  reaches another repository through `cd`, `git -C`, `--git-dir`, a
  `GIT_DIR=` prefix or a path held in a variable set earlier on the same line
  is judged by that repository's branch and its own policy, so a commit on
  another repository's feature branch is no longer refused as a commit on the
  session's `main`. A repository without a CodeFlow policy keeps the default
  protection of `main` and `master`. `git -C <repo> --git-dir=<git dir>` is
  judged by the git dir it writes to, which closes a wrong allow. A `cd`
  that can fail proves its move only to commands chained with `&&`. When the
  guard cannot prove the target, for example an unset or escaped variable,
  a path built from a command substitution, a subshell, `pushd` or an `env`
  option, it blocks a commit, merge, push or other
  mutation and says how to name the repository: a literal path, or
  `cd <path> &&` first. A git command whose subcommand, global options or,
  for a commit, merge, push or other judged command, arguments come from a
  command substitution is refused too; generated text is accepted only in a
  quoted message such as `-m "$(…)"`. A read-only command keeps working
  with a substitution after its subcommand, as in `git show "$(…)"`, since
  its arguments cannot turn it into a mutation. An unclassifiable command is
  still judged by every other rule, such as the protected-commit check, and
  the strictest verdict wins. A git alias is judged by what it expands to,
  read with `git config` in the target repository (including `-c` and
  `include.path`); a `!` shell alias or one the guard cannot read blocks.
  `git rebase <upstream> <branch>` is judged by `<branch>`, which it
  rewrites. A checkout or rebase that can fail moves the branch only for
  commands after `&&`; after `;` or a newline the earlier branch still
  counts. An alias written earlier in the same command line is not read
  from disk: the command blocks. A git command inside `for`, `while`, `if`
  or `case` bodies is judged too. This also blocks a commit written inside a
  subshell such as `(cd <repo> && git commit)`.

<!-- codeflow:release-impact patch -->
- **exec-guard refuses a protected deletion however it is composed.** The
  catastrophic floor refused `rm -rf /` and `rm -rf ~` but let the same
  deletion through as `find / -delete`, `find / -exec rm -rf {} +`,
  `ls / | xargs rm -rf`, `rm -rf /Users/<name>`, `cd ~ && rm -rf *` or
  `D=/; rm -rf $D`, or inside a subshell, group, `if`, `for`, `while` or
  `case` body. The same holds for `rsync --delete` into a protected
  directory, `${HOME:-/}` and `~user`, a link that lands on one, and a
  deletion behind `command`, `env`, `nice` or a here-string. A `find` rooted
  at a protected directory with `-delete` or an `-exec` remover is refused
  whatever its tests, so `find ~ -name .DS_Store -delete` is now refused.
  Each is refused as its `rm -rf` equivalent is. A variable or directory
  that may hold a protected value on any path is refused too, and the
  message then says so. Functions, `local`, positional parameters, arrays,
  `read`, `IFS`, aliases, `eval`, `command cd` and `builtin cd` are
  followed as the shell runs them, and so are traps, zsh hook functions,
  `coproc`, arithmetic and a `cd` that may fail. exec-guard allows only the
  shell it models: after anything else, such as a sourced file,
  `declare -n`, `enable`, `emulate`, `set -k`, a trap it cannot read, an
  assignment to `CDPATH` or `BASH_ENV`, zsh-only syntax, or a command named
  by a value it cannot see, a deletion whose target depends on the state
  is refused as unproven, and the message asks for the project path written
  literally. A deletion inside the project, such as
  `find . -name '*.o' -delete`, still runs.

<!-- codeflow:release-impact patch -->
- **A reset trap no longer refuses a later cleanup.** exec-guard kept a
  `trap` action or zsh hook function feasible for the rest of the line
  after it was reset or removed, so
  `trap 'D=/' DEBUG; trap - DEBUG; D=build; rm -rf "$D"` was refused. A
  reset (`trap - SIG`, `trap '' SIG`, a new action), `unfunction`,
  `unset -f` or a new hook body now takes the action away where it runs on
  every path, outside any function call, and names the same signal or
  function. A reset inside an `if`, after `||`, in a subshell or a function,
  or of a signal the guard cannot resolve still leaves the action feasible.

<!-- codeflow:release-impact patch -->
- **A peer CLI's help no longer counts as a headless run.** `claude --help
  -p` and `codex exec --help` print help and exit, so exec-guard no longer
  reports them under `security.headless_peer_runs`; at the `block` level
  they were refused. The same word as a prompt, an option's value or after
  `--`, as in `codex exec -- --help`, is still reported.

<!-- codeflow:release-impact patch -->
- **A peer CLI started through a package runner counts as a headless run.**
  `npx @anthropic-ai/claude-code -p`, `bunx @openai/codex exec`, and the
  same through `npm exec`, `npm x`, `bun x`, `pnpm dlx`, `yarn dlx`, pnpm
  11's `pnx` and `pn dlx`, or an installed peer through `pnpm exec` or
  `pn exec`, are now judged as the direct
  `claude -p` or `codex exec` is, under `security.headless_peer_runs` at
  both `warn` and `block`; before, they passed unreported. The runner's own
  `--help` or `--version` runs nothing and passes.

<!-- codeflow:release-impact patch -->
- **A git hook runs the codeflow that started git.** When a `codeflow`
  command ran git, such as `task new` pushing its reservation to the
  registry, the hook git fired ran whichever `codeflow` was first on PATH;
  an older one there refused the registry push. codeflow now names its own
  binary in `CODEFLOW_HOOK_BINARY` for its git children only, and the hook
  shims run that binary, failing the hook when it is missing or not
  executable. Git run outside codeflow still uses the `codeflow` on PATH.
  Run `codeflow update` to install the new shims.

<!-- codeflow:release-impact patch -->
- **A killed full gate on Windows ends its targets.** When a
  `codeflow test --mode full` process was killed on Windows, its running
  target kept going while the gate lock was freed, so a second full gate
  could start beside it. Each target now runs in a job object that ends
  the target's whole process tree when the gate exits, however it exits.
  On Unix the lock still stays held until a killed gate's target exits.

<!-- codeflow:release-impact patch -->
- **Presentation cleanup on macOS no longer fails on a busy machine.**
  Closing a presentation lists processes with `ps` to prove its browser is
  gone. One process of any user caught mid-start could list bytes that are
  not UTF-8, and the whole listing was refused, so cleanup failed and left
  recovery evidence behind. Such a line is now read as it stands; it can
  neither hide an owned browser process nor match as one.

<!-- codeflow:release-impact patch -->
- **An unquoted numeric pin is refused for its quoting.** A `depends_on`
  pin such as `pin: 70283613`, which YAML reads as a number, was reported
  as not a commit id. The message now says the pin must be quoted, as in
  `pin: "70283613"`, since the text may well be a commit id.

### Changed

<!-- codeflow:release-impact patch -->
- **A change runs to its finish line.** A brief that asks for a change runs
  through to the readiness report. After two review rounds, a disagreement
  on a reversible choice inside the accepted outcome is settled by the
  judgment primary and recorded as settled dissent, never as approval; a
  dissent on safety, security or correctness keeps the gate closed. A seat
  lost mid-run moves to its next eligible alternative with reduced
  assurance. The primary merges a green, reviewed pull request into an
  integration branch that no protected-branch rule covers, with a no fast
  forward merge, and reruns the gate; every protected target stays a human
  merge. "Ready on local evidence" needs a completed green result for every
  owed check and names each hosted job that never ran.

<!-- codeflow:release-impact major -->
- **Model catalog schema 5 (ADR-0069).** The managed
  `current-ensemble.json` becomes a catalog of families, product lines,
  seats and duties carrying the 2026-09-23 roster, and
  `codeflow models resolve` returns each duty's pinned ids and efforts.
  The binary no longer reads schema 4: until you run `codeflow update`,
  `codeflow doctor --check model-bindings` fails and `codeflow models
  resolve` refuses on an older tree. After the update, the check reports
  a standing warning on every scaffold until each designated version has a
  full-suite qualification record at high effort on each of its harnesses.

<!-- codeflow:release-impact none -->
- **No native Windows build in 3.0.0.** This release publishes archives for
  macOS (arm64 and x86-64) and Linux x86-64, the shell installer and the
  source archive. It publishes no Windows archive, PowerShell installer or
  `codeflow.exe`, although a pre-policy entry below says CodeFlow adds them.
  2.1.0 had no Windows asset either, so no published asset is removed. The
  Windows CI job keeps running as an advisory check and does not gate the
  release. Native Windows support is planned for 3.0.1; until then, use the
  Linux build in WSL2.

<!-- codeflow:release-impact none -->
- **Reading the pre-policy entries below.** They are kept exactly as
  written before the release policy, so two of them describe what has since
  changed: ADR-0062 replaced the ADR-0061 release automation, and the Grok
  seat is `grok-primary` on `grok-4.7` in the schema 5 catalog.

<!-- codeflow:release-impact major legacy-group=pre-policy-v3 sha256=2e372b00f9ef20009024ba30733d75525345a0537bc419e2eb65a2b60aa59e9e -->

- **Optional agentic operating and estimation method (ADR-0057).** Standard and
  full projects receive cf-estimate: anchored four-grade classification,
  evidence-labelled scenarios, explicit resource/dependency allocation and
  prospective outcome learning. Planning and customization actively offer a
  project-specific preview, confirm adoption and respect existing authority or
  decline. No universal hour bands, forced profile, second tracker or claim of
  predictive calibration is installed.

- **Grok Build as a first-class host and catalog family (ADR-0054).** Interactive
  `grok` joins Claude Code and Codex as a CodeFlow host. The standing pair
  remains `claude-judgment-primary` and `codex-engineering-primary`;
  `grok-engineering-primary` is a catalog seat on `grok-4.6` at medium (ADR-0055). Extra-family
  review is named when a routing-policy trigger fires and the family is
  available; unavailable is an evidenced limitation, never a silent third vote.
  Claude produces design in its native session regardless of host. `grok-cli`
  is catalog-supported with a `grok-cli-version` doctor probe and a fifteenth
  doctor check for structural `.grok/hooks` wiring. Grok-hosted
  Claude/Codex lanes use Herdr; dated schema-v2 and Codex Herdr canaries
  live in `docs/verification/grok-host-duo-canary-2026-09-07.md` and are
  not a qualified binding. Headless `grok -p` is not a work-session lane.

- **Herdr-primary consult and delegate TTY overlay.** Inside Herdr, dual-lineage
  seats open in named tabs (`cf/<repo>/<work>/<kind>/<nn>`) without hijacking
  the caller pane. Resume requires the agent's cwd to match the intended
  worktree. Schema-v2 lifecycle still owns Claude turn completion; tmux remains
  the degraded host. Description-trigger checks live under `evals/skill-triggers/`
  and run in the test gate.

### Changed

- **Human-authorized release automation (ADR-0061).** CodeFlow PRs now declare
  reviewed release impact; git-cliff remains the sole bump calculator while a
  guarded workflow maintains one candidate PR and cargo-dist remains the sole
  publisher after a human merges the exact candidate. The v2.1 tag/source
  discrepancy is preserved as explicit provenance, not repaired by moving a
  tag. Consumer projects keep their own version and publication authorities;
  customization, shipping, and review route to one opt-in project policy with
  seven scoped model diagnostics recorded separately from deterministic tests.

- **Native evaluation executable binding.** Disposable trial receipts now
  record the resolved CodeFlow executable and SHA-256 and reject changed bytes
  during materialization. Launch guidance requires actual native command and
  hook attribution; a matching version or intended `PATH` is insufficient.
  Peer-dependent Claude turns require the peer's verified native result before
  the host returns. The strict delegate terminal state machine is unchanged.

- **Proportionate project organization.** New projects derive their repository
  shape from accepted ownership, native build, runtime, release, data, and trust
  boundaries; existing projects keep credible source layouts and work
  authorities. Onboarding explains current-directory init, fixed document
  destinations, tier history, and collision reconciliation. A foreign tasks
  folder alone no longer activates durable CodeFlow work gates, while full-tier
  and recognizable historical CodeFlow tasks retain their planning anchors.
  Relevant indeterminate tracking state now stops with a diagnostic.
  Living requirements remain distinct from frozen change specifications; an
  external tracker cannot waive active repository-execution gates.

- **Accountable worker execution (ADR-0060).** Plans now distinguish the
  responsible primary from the actual binding-or-route executor and record
  requested-versus-observed provenance plus scoped usage evidence. Natively
  proven candidates may perform bounded non-design work under primary
  inspection without a qualification or economy claim; scoped-qualified status
  remains limited to exact reviewed workload tuples and separate from full
  primary promotion. Claude owns design direction, real design execution, and
  fidelity unless an explicit task-specific operator override applies. This is
  portable orchestration policy, not a scheduler or runtime routing engine.

- **Portal customization examples.** Document the portal's repository-relative
  token path and accent-only light/dark format; do not borrow `cf-present`'s
  different import schema. Connect custom accents to the actual utility tokens
  and validate contrast against all reader-selectable skins. Regression checks
  reject schema mix-ups and verify rendered colors in light/dark appearances.

- **Qualified native transport fallback (ADR-0059).** Prefer the official Codex
  plugin, but allow verified official native clients when it is incompatible.
  Preserve safety, tools and independent-review evidence; distinguish product
  behavior from optional transport qualification instead of blocking unrelated
  delivery. No plugin fork, new broker or permission bypass is introduced.

- **Breaking: explicit portal runtime ownership (ADR-0058).** Managed portal
  updates replace unchanged files, repair missing managed files, and stop all
  portal writes on drift or collisions. They no longer line-merge runtime
  source/lockfiles, create conflict sidecars or retain pristine copies. Use
  supported configuration, restore reviewed managed bytes, or explicitly run
  `codeflow portal transfer --confirm` to preserve edits and intentional
  deletions under project maintenance. Adoption v2 records ownership and
  generator identity; transferred-from provenance stays frozen. Evidence v1
  remains strict, including legitimate renamed forks. Legacy journals recover
  first; unknown or altered baseline content blocks migration and transfer
  without deletion. See `docs/releasing.md` for safe manual recovery. General
  scaffold behavior and the portal's visual design are unchanged.

- **High default for development primaries (ADR-0056).** Claude, Codex, and
  catalog Grok primaries now start at high; bounded routine workers may use
  medium, while xhigh remains trigger-driven and owned by the receiving family.
  This supersedes the earlier medium-default entries below, not their transport
  or safety decisions. Regenerate managed settings on update; explicit local
  effort overrides remain deliberate project/operator choices.

- **Post-merge quality and runtime hardening.** Presentation selections retain
  the actual repeated-text occurrence; authored styles cannot hide review
  controls or create top-layer escapes. Utility font choices load bundled
  licensed faces offline in presentations and portals. Explanatory motion is
  permitted with static meaning and reduced-motion alternatives.

- **Review evidence and effort decisions.** PR guidance covers the full branch,
  revision-bound tests and independent review; executable documentation cannot
  bypass testing requirements. High-default primaries own proportionate
  same-family worker routing, including direct xhigh when justified; bounded
  routine workers may use medium. Adversarial evaluation scenarios distinguish
  those decisions from routine work and from observed native routing.

- **Mid-session effort and extra-family invoke.** The medium primary stays
  the orchestrator and spawns same-family high/xhigh workers when a
  complexity trigger fires. Extra catalog families are named when
  routing-policy triggers fire and the family is available; still never a
  silent third vote.

- **Codex Computer Use QA of changed UI (ADR-0043).** Claude still produces
  `DESIGN_INTENT` and the implementer check. Codex independently QAs the
  changed surface through Computer Use on the app-server. If Codex produced
  the UI, Claude QAs with Computer Use in Claude Code. Playwright stays
  the web driver. Adaptive viewports, accessibility, i18n/LTR-RTL, and
  system layers stay proportionate intent dimensions.

- **Gate redness is classified (ADR-0017, ADR-0038).** A gate is the
  verification check, not the CI job name. An assertion-red completed check
  still cannot be overridden. An infra-killed job (runner, memory, billing)
  is missing job evidence; a completed same-check in a sibling job or local
  `codeflow test` satisfies it. Agents still never merge.

- **Medium-default effort, Astra Codex primary, contained worktrees (ADR-0055).**
  All families default to medium and escalate to high/xhigh on complexity or
  difficulty (new or existing architecture, technical, planning, or design work
  — novelty is not a trigger). Codex primary is `gpt-6-astra` via the official
  app-server, with interactive CLI as the Grok-host/Codex-host fallback (the
  Claude Code lane stays plugin-only); no third-party Grok Codex plugins.
  Production host permissions are Claude `bypassPermissions` or
  `auto`, Codex full-access/always-approve, Grok `--always-approve`. Consult
  and no-edit review stay non-bypass. Linked checkouts live under
  `.worktrees/<slug>` (gitignored). `core.hooksPath` stays project-relative.
  Herdr/tmux cwd is the project being worked; reuse the same tab for the same
  topic and close it when that work is done.

- **Plan, develop, and consult craft.** After both seats settle Plan vN, planning
  synthesizes that ground and still asks live operator-owned questions. Develop
  tests through named interfaces first. Consult findings label `axis: standards`
  or `axis: spec` (both when both apply). An operator-unattended TTY overlay
  may skip routine approval clicks when the operator asks not to babysit,
  without amending harness-boundary ADRs or granting consults write access.

> **Breaking migrations.** Before installing v3:
>
> - replace every `changed_files` coverage scope with `per_file`, `per_package`,
>   `per_module`, or `global`;
> - rename or remove test modes outside `quick`, `essential`, and `full`;
> - set `security.dangerous_commands` to `block`, or remove the key to accept
>   the non-relaxable default; and
> - repair canonical work records reported by `codeflow validate --docs`,
>   including filename/ID mismatches, malformed or dangling relationships,
>   completed tasks with unchecked acceptance criteria, and approved or
>   implemented specs with unresolved open questions.
>
> Make those repairs on a planning branch and merge them before updating. These
> contract changes require a major bump from `v2.1.0`; feature additions do not
> reduce the release to a minor version.

### Fixed

- **Breaking: presentation build reproducibility.** Maintainer asset builds now require
  the official pinned Node distribution's compression libraries and explain
  incompatible system-library builds. Regenerated export bytes match that
  qualified toolchain; normal CLI use and consumer Rust builds still need no Node.

- **Reliable presentation selections.** Comment capture revalidates the live
  selection, rejects invalid or oversized anchors, distinguishes repeated words
  and preserves toolbar selection across focus changes. Exiting Comment clears
  capture state so the same anchor can be selected again. Embedded views no longer
  cover neighboring text with comment overlays; leaving Comment mode restores
  their pointer interaction without changing the surrounding layout.

- **Portal title rendering.** A leading title repeated with different initial
  capitalization now renders once, including record-ID prefixes. Other wording
  and internal capitalization remain source-owned; source documents are unchanged.

- **Breaking: portal dependency security updates.** The repository portal and distributed
  starter use Astro 7.2.8, Sharp 0.35.4, js-yaml 4.3.2 and SVGO 4.1.0 to address
  six reported advisories. Dependency lifecycle scripts remain disabled.
  The portal build requires Node 22.19.0 or newer to match the updated transitive
  HTTP library; the pinned Node 26.4.0 toolchain is unchanged.
  Existing adopters receive the managed-file reconciliation through
  `codeflow update` after installing the corrected CLI; resolve any reported
  local customization conflicts and rerun the portal's locked install and checks.

- **Presentation qualification now exercises the real owned runtime and offline
  export boundary.** A full-only gate drives the CLI, authenticated loopback
  service, isolated headless browser profiles, feedback/update/reopen flow,
  enhanced export, accessibility, network/console state, and exact bounded
  teardown. The real journey corrected strict-cookie bootstrap handoff and
  template payload extraction defects that raw HTTP and synthetic browser tests
  had missed. An injected failure proves the primary error survives exhaustive
  exact-owned cleanup, and runner children start from an explicit functional
  environment rather than inheriting unrelated host values. Executable child
  canaries cover the complete provider-secret deny set without leaking coverage
  profiles. Windows qualification now fails closed unless an externally
  provisioned disposable profile confines the actual Local AppData Known
  Folder; the outer native lane still owns profile or VM teardown proof.
  Greenfield/brownfield initialized-baseline parity, idempotent update, narrow
  cleanup, reproducible locked assets, exact CI Node/npm tooling, binary Git
  attributes, and isolated deterministic release payload-size deltas now have
  owning checks. Interrupted presentation creation is recoverable even before
  its transaction marker is written. Documentation-portal stale stubs retain
  only a bounded unavailable identity so current records can link to the safe
  diagnostic without republishing stale content, and a parity gate keeps the
  dogfood runtime identical to the shipped starter on every supported host.
  Producer and verifier now share one strict unavailable-identity grammar,
  while the local aggregate and CI both enforce the reciprocal documentation
  graph instead of leaving that check to a separate hosted step.
- **Claude's OS sandbox now keeps installed plugin code executable without
  exposing mutable plugin or private Claude state.** The broad `~/.claude`
  subprocess-read denial remains, while the higher-precedence filesystem
  carveout permits only `~/.claude/plugins/cache`; plugin job data, credentials,
  histories, sessions, settings, memory, and other private state remain denied.
  Updates retire superseded CodeFlow-managed sandbox array entries from the
  prior shipped baseline while preserving project-owned entries.
- **Delegate interruption coverage now waits for the process handler.** The
  SIGINT lifecycle test allows a bounded child-initialization window, avoiding
  a false failure under parallel or instrumented test startup while still
  proving exit 130 and run poisoning after prompt acceptance.
- **New durable records cannot start malformed.** Epic, spec, and task titles
  are encoded safely in YAML and must be non-empty single-line labels. Task
  allocation also proves that its declared integration target already resolves
  to a real local or remote-tracking branch instead of deferring a missing
  target to implementation time.
- **Work-start targets cannot be arbitrary Git revisions.** The stable-anchor
  preflight resolves only real local or remote-tracking non-task branches and
  record validator rejects `HEAD`, full object IDs, tags, and revision
  expressions before the ref lookup, preventing an implementation branch from
  authorizing itself through a revspec alias.
- **The stable-planning-anchor canary now tests its stated premise.** It uses a
  real declared target at the fixture's parent commit and a locally valid,
  approved workgraph added only on the planning branch, rather than sharing the
  intentionally orphaned/draft-spec fixture used by graph-rejection cases.
- **Partial epic and task updates preserve the complete planning record.**
  Status and delivery-field updates now patch the generic YAML frontmatter
  instead of re-serializing the narrower typed workgraph view. Capability,
  ADR, spec, graph, and future project-owned fields therefore remain intact,
  along with the markdown body and the template's `created` field shape.
- **Planning and implementation gates now enforce one workgraph contract.**
  Canonical task writers retain and require the stable integration target,
  blocked/closed tasks cannot start, spec linking preserves consumer comments
  and key order, and multiline template comments or explicit resolved markers
  no longer masquerade as unresolved spec questions. Duplicate identities
  across canonical and legacy layouts now fail normal validation, and anchored
  parse failures retain their specific graph error. `work start`, full-tier
  task-branch pre-commit, and full-tier task-branch CI all reject an invalid
  visible graph before checking the same anchored task snapshot; planning
  records created on a task branch cannot authorize their own implementation.
  Minimal and standard tiers retain `task/` as an ordinary documented branch
  prefix without requiring absent full-tier records. A focused lifecycle
  canary now exercises the complete brief-to-cleanup route plus material
  replanning, blocker strategy changes, cancellation honesty, and missing-seat
  degradation.

### Changed

- **Repository guide portals are now explicit, source-linked, and verifiable.**
  Standard/full projects receive the concise cross-harness `cf-docs-portal`
  workflow, while the exact-pinned Starlight/Pagefind starter remains absent
  until `codeflow portal setup --path <dir>` adopts it offline. Adopted files
  reconcile through ordinary never-clobber updates and opaque pristine
  baselines; `codeflow validate --portal <dir>` checks bounded evidence claims
  against repository/output bytes without executing project code. CodeFlow
  dogfoods the optional utility under `docs-portal/`. The pinned GFM adapter
  now requires one clean committed runtime/source/media snapshot, reads source
  claims from bounded Git blobs, detects index-masked worktree changes, rebuilds
  broken-source routes as bounded non-searchable current-source error pages
  without walking or republishing history, uses bounded batched Git reads,
  stable no-follow publication copies, raster-dimension limits, and
  locale-independent ordering, neutralizes derived metadata, and
  publishes under recoverable corpus and whole-workflow leases (ADR-0048,
  CAP-015). Canonical routes retain exact NFC case and punctuation while URL
  boundaries encode segments; shared JavaScript/Rust authority fixtures cover
  strict frontmatter, repository URLs, and SHA-1/SHA-256 object IDs. Committed
  non-reserved public files are also snapshot-authoritative. CodeFlow's full
  strict gate now blocks on locked portal tests/build/Rust validation on Ubuntu,
  with the authority/path suite repeated on Windows; generic consumer CI stays
  opt-in. Compressed release embedding keeps the offline starter within the
  ADR-0048 binary budget.

- **Multi-task epics now default to one topology-aware integration branch.**
  Planning creates the shared non-protected target from the intended protected
  branch before task allocation; independent nodes may branch concurrently
  within the host resource budget, dependent nodes branch from the updated
  integration tip only after predecessors land, and landings remain serialized.
  Aggregate verification and both-primary review run on the combined diff
  before one final human-reviewed PR. A different landing shape requires a
  recorded Plan vN rationale and dual approval rather than convenience.
- **Product language and appearance are now explicit, contextual design
  dimensions.** `cf-design` records language/voice and applicable appearance
  modes inside the proportionate `DESIGN_INTENT`, while `cf-editorial-review`
  remains the single shared authority for titles, emoji, fabricated
  personality, and language-quality judgment. New semantic cases protect
  distinct evidenced product voices, established-system collapse,
  localization honesty, preference/persistence verification, and the boundary
  that prevents CodeFlow utility defaults from becoming product design
  authority.
- **Design refinement and asset sourcing now stay bounded and attributable.**
  After a direction is selected, `cf-design` varies only a named unresolved
  material choice and stops when it is settled. An on-demand reference keeps
  inspiration distinct from user evidence, records proportionate rights,
  consent, transformation, and product-use provenance, refuses unauthorized
  private-data uploads, and binds material revision feedback to the exact
  reviewed version without adding a provider catalog or design database.
- **Durable work now has one explicit, adaptable authority and a stable start
  boundary (ADR-0046).** Full-tier scaffolds write independent `EPC-NNN`,
  `SPC-NNN`, and `TSK-NNN` records in one flat Git Markdown workgraph. Tasks
  declare an epic or standalone rationale, direct dependencies, consumed specs,
  and a non-task integration target; specs are allocated and linked by the CLI.
  Historical dual-identity, nested, and `TSK-NNN-NNN` records remain readable.
  `codeflow work start` rejects task-branch targets and proves from the
  merge-base that planning reached the declared target, the anchored record
  declares that same target, and parents, specs, and predecessors are ready; the
  full-tier pre-commit hook and detached CI reuse the same read-only core check,
  and a full-tier task branch with no visible durable record fails locally
  instead of falling through to ordinary commit checks. The
  planning method now makes the discussion → independent duo settlement →
  planning PR → anchored task → implementation/review/ship path and its
  failure routes explicit. Monorepo decomposition, serialized allocation,
  loose-link external trackers, bounded-deviation closeout, spec recall, and
  rebuildable local caches remain part of the authority contract.
- **End-to-end and concurrent UI verification now own their real boundaries
  (ADR-0044).** Material changed journeys map the affected frontend, service,
  state, external-seam, infrastructure, runtime, outcome, and recovery path,
  then prove one faithful vertical run instead of promoting disconnected unit
  and mocked layer tests to E2E. Concurrent Playwright work receives
  task-owned isolated browser state, non-overlapping endpoints where
  applicable, namespaced test data, run-scoped artifacts, and verified
  teardown; headed or Computer Use work cannot commandeer the operator's
  browser or active desktop. Consuming projects choose their allocator/ranges
  and cleanup commands during customization. Worktree closeout now inventories
  and promptly removes proven-landed entries while preserving active, dirty,
  and unproven work with ownership evidence; `codeflow status` classifies
  linked worktrees and unattached local branches from ancestry, squash-safe
  patch equivalence, and dirtiness without deleting them. Three behavioral
  canaries and deterministic contract tests pin the contract without adding a
  browser, port broker, or deletion command.
- **Product and interface design direction is now an explicit proportionate
  contract (ADR-0043).** Standard/full scaffolds gain a mirrored `cf-design`
  skill. Material new surfaces settle evidence-grounded `DESIGN_INTENT` inside
  Plan vN; cosmetic and established-system work can collapse without ceremony.
  The stable Claude judgment role leads intent, Codex challenges feasibility
  and fidelity, both approve the plan, and review distinguishes evidenced drift
  from taste. Behavioral and rendered cases exercise direction selection,
  operator precedence, evidence-grounded design-choice review, accessibility, and
  fidelity without tying the doctrine to a concrete model release.
- **Coverage now requires test integrity, not only a percentage.** The shared
  quality contract and independent reviewer reject tautologies, production
  logic copied into test oracles, mock-only wiring checks, weakened assertions,
  and production branches added only to manufacture coverage. `CF-QA-009` and a
  behavioral canary keep the 80/90 coverage policy from being gamed while
  preserving explicit contract fixtures and stable named invariants.
- **Performance, scale, and concurrency review now has an operating-shape
  canary.** Review checks complexity/N+1 access, bounded work and memory,
  backpressure/cancellation, async blocking, state synchronization and races,
  idempotency/retry amplification, and resource cleanup. Measurement,
  load/stress, or concurrency evidence is required when material risk earns it,
  without imposing ceremonial benchmarks on unaffected paths. A targeted
  native Fable/Sol
  [diagnostic](docs/verification/model-role-quality-diagnostic-2026-07-26.md)
  exercised this case together with managed-primary resolution and
  test-integrity review; it is retained as diagnostic evidence, not binding
  qualification.
- **Stable model roles now resolve through qualified project bindings
  (ADR-0041), and verification is explicitly layered (ADR-0042).** Durable
  orchestration names the Claude judgment and Codex engineering roles rather
  than a model release. The managed ensemble still selects the current Fable
  and Sol bindings, while standard/full projects may atomically reference
  locally approved binding IDs from `.codeflow/model-selection.json`.
  `codeflow doctor --check model-bindings` fails closed on invalid, drifted, or
  pseudo-duo selections without partial fallback. Role-tagged evals, a
  seat-collapse regression, deterministic-red verification, and bounded-history
  craftsmanship cases protect the boundary. CodeFlow will enable GitHub CodeQL
  default Rust analysis after public launch; consuming projects select SAST
  from stack/hosting evidence during customization, so no CodeQL workflow is
  imposed by the portable scaffold.
- **Model and harness evolution now has qualified bindings (ADR-0039).**
  Durable independent planning, Claude-led design, capability-routed
  production, cross-lineage review, integrated Claude judgment, and graceful
  degradation remain unchanged. One managed ensemble record now owns concrete
  model selectors, effort policy, worker classes, and escalation triggers; a
  universal native-harness capability catalog replaces the evaluator's
  hard-coded harness enum. Catalog entries mean capability support, not
  concrete model-binding qualification. Composable diagnostic packs select existing cases
  without weakening full promotion. Approved full-suite results can emit
  compact non-secret local binding records, and a fourteenth doctor check
  reports requested/observed contradictions plus observable harness-version and
  settings drift without launching, scraping, or automatically routing a
  model. Catalogs may name only code-allowlisted version probes; they cannot
  supply commands or arguments.
- **Multi-task plans now settle a durable, non-executable task graph
  (ADR-0040).** Both primary seats approve explicit nodes, dependencies, and
  genuine evidence-guarded decisions. Task records use canonical `depends_on`
  with a read-compatible `dependencies` alias, while `validate --docs` rejects
  malformed IDs, conflicting fields, dangling/self/duplicate edges, and cycles
  without becoming a scheduler. Material graph and cross-task contract changes
  require Plan vN+1; ordinary in-node detail does not. Verification planning
  may earn property tests, targeted mutation testing, or a project-owned
  architecture fitness check from explicit risk and oracle evidence, without
  imposing optional tools on consuming projects.
- **Critical-path focus now preserves bounded quality work (ADR-0038).**
  Materiality and the dependency presently controlling the accepted outcome are
  treated as related but distinct. Models protect all quality and safety gates,
  fix clear safe in-scope improvements when focused validation is bounded, and
  consolidate only genuinely uncertain secondary observations for one natural
  Claude+Codex checkpoint. Retained work is tracked once at an existing
  planning altitude with evidence and a deterministic revisit event; vague
  time-only deferral and issue-per-nit churn are rejected. Paired `CF-QA-005`
  fixtures guard both material-first execution and the no-reflexive-deferral
  case. Blocked work now advances only through changed evidence or a safe
  outcome-preserving strategy; the operator is asked only for a real external
  dependency or owner decision, while deterministic and safety gates remain
  non-bypassable.
- **Duo execution is now capability-routed per task (ADR-0035).** Independent
  Claude+Codex discovery, Claude-led design, versioned joint approval, native
  interactive transport, evidence gates, and integrated Claude judgment remain
  mandatory. The host now records each task's producer and cross-lineage
  reviewer from verified task fit, tools/context, independence, routing,
  resources, and observed native usage signals; a seat/lineage reassignment
  invalidates approval. Explicit host/peer/worker roles prevent nested duos,
  unverified workers are unavailable rather than guessed, and units authored
  by the Claude judgment primary receive independent Codex review without
  calling the primary's integrated judgment self-independent.
- **Reviews and proactive discovery now act by materiality (ADR-0034).**
  Models still search broadly, but substantiate candidates and lead with
  consequential strategic, architectural, structural, correctness, security,
  robustness, operability, and meaningful edge/error concerns. Severity stays
  separate from confidence and remediation effort; repeated small symptoms may
  form one systemic finding, while cosmetics remain minor and non-blocking.
  Evidenced out-of-scope material risk is escalated or routed to one tracked
  item without silent scope expansion or external mutation. Paired `CF-QA-005`
  cases guard ordering, approval with nits only, systemic diagnosis,
  effort-neutral severity, CVSS-aligned security vocabulary, and proactive
  routing without issue farming.
- **Web UI verification now routes browser mode and evidence by claim.**
  Routine deterministic Playwright E2E may run its browser headless while the
  Claude/Codex seats remain native interactive sessions. Behavioral assertions,
  same-environment visual comparisons, console/network evidence, and bounded
  failure traces are kept distinct; screenshots and automated accessibility
  checks are not treated as complete proof. Headed mode is reserved for claims
  that materially need live rendering, browser chrome, or debugging, and
  Computer Use remains a fallback outside the controlled page. A `CF-QA-002`
  canary guards this distinction across future model bindings.
- **Catastrophic-action protection is cross-platform and non-relaxable
  (ADR-0033).** The deterministic shell guard now covers Linux/WSL2, macOS, and
  native Windows Bash/PowerShell events; blocks protected-root deletion plus
  destructive disk, recovery, and system-permission operations; and cannot be
  downgraded through project policy. Common privilege, shell-command, and
  environment wrappers cannot bypass the classifier, while equivalent
  task-scoped project paths remain autonomous. Model agreement and automatic review are
  explicitly not human authorization. Secret-store denies now reach Claude's
  OS-sandboxed subprocesses. CodeFlow adds a native Windows x64 artifact,
  PowerShell installer, elevated Codex sandbox default, Windows build/test
  lane, and configurable test-command shell with dependency-free native
  defaults; WSL2 remains the required route
  for Claude work that needs OS-enforced containment. A release checklist now
  makes source/security gates, native platform and installer canaries,
  harness/model qualification, public-network installation, and rollback
  evidence explicit before downstream Agent OS work begins.
- **Adaptive test setup is safe and release-complete (ADR-0031).** Existing
  populated or malformed configs are preserved unless a reviewed template is
  explicitly applied with `--replace`; malformed configured pre-push gates now
  violate instead of skipping. `codeflow test setup` can list/apply templates
  embedded in the binary and append monorepo targets, while automatic detection
  remains root-only. Setup no longer emits dormant structural rules, doctor
  warns on legacy blocks without enabling the parked validator, and the schema
  now states the actual `CI` and quick/essential/full contracts; configs with
  empty mode maps or non-public mode names now fail validation.
- **Substantial prose now has a contextual editorial gate (ADR-0032).**
  Standard/full scaffolds gain a mirrored on-demand `cf-editorial-review` skill
  for documentation, ADRs, proposals, release notes, PR narratives, operator
  communications, and user-facing copy. It preserves technical meaning and the
  consuming project's documented voice, removes unsupported certainty,
  sycophancy, inflation, and obstructive formatting, and never invents a
  persona. The always-loaded contract gains only the voice hierarchy; detailed
  smells remain progressive disclosure. Six `CF-OUT-002` cases protect
  technical prose, operator updates, voice, PR formatting, contextual emoji,
  and legitimate punctuation/terms/lists. No Vale, AI detector, or lexical
  blacklist is added.
- **Right-sized design and code are now a blocking duo gate (ADR-0030).**
  Both seats must approve design proportionality; every routed producer
  first-verifies the smallest coherent, idiomatic change, the other lineage
  reviews it independently, and the directly invoked Claude judgment primary
  owns the integrated quality verdict. Speculative features, abstractions,
  configuration, dependencies, compatibility paths, dead code, and other
  complexity without a current requirement or evidenced risk yield
  `changes_requested` even when tests pass. The same gate rejects brittle
  under-design: duplicated business rules, unexplained hard-coding, swallowed
  errors, missing accepted edge cases, or one-off UI that bypasses an existing
  design system. Paired regression fixtures test both directions without
  treating raw line count or a framework pattern as a quality target. A context
  canary distinguishes disposable experiments from durable multi-team systems,
  requires clarification when that distinction is material, and uses safe
  reversible defaults rather than project size as an architecture rule.
- **Primary model seats now start at high and escalate by evidence (ADR-0028).**
  Cross-harness calls still target Fable and GPT-5.6 Sol (or their strongest
  supported successors) as the two independent reasoning seats. High is the
  default; xhigh is reserved for defined complexity, ambiguity, security,
  long-horizon, disagreement, or failed/stalled-high triggers. Fable owns
  Opus medium/high tool-operation routing, and Codex owns any verified native
  Sol-class medium/high worker routing without transferring either primary
  seat's judgment, approval, implementation, or verification responsibility.
  Codex project settings pin high as the fallback, while Claude-hosted plugin
  turns pass high/xhigh explicitly so a user-level default cannot silently
  change the observed peer binding.
- **Claude can use trusted native tools that require host state (ADR-0029).**
  The fail-closed sandbox remains the default, but Auto may classify one
  unsandboxed retry after a sandbox-boundary failure for a trusted installed
  tool such as the official Codex plugin. This avoids granting arbitrary Bash
  write access to `~/.codex`; destructive, privileged, secret, credential, and
  private-network controls remain in force, and arbitrary unsandboxed commands
  remain outside the workflow contract.
- **Model-effort comparisons now prove the binding and isolate the variable.**
  The evaluator can record harness-observed model/effort evidence and fixed peer
  seats, reject configuration drift outside the declared experiment variable,
  recompute raw results safely, report latency/token/cost distributions, and
  block promotion on any case regression, incomplete run, validity flag, or
  missing identified human approval. Existing protocol-conformant schema-version
  1 results remain valid when they are not used for strict promotion comparisons.
- **Coverage scope must have a measurable denominator.** Configuration now
  rejects the former `changed_files` scope because an invalid base ref could
  make it measure an empty file set and pass vacuously. Migrate affected rules
  to `per_file`, `per_package`, `per_module`, or `global`.
- **Harness settings now close the remaining credential and destructive-action
  gaps (ADR-0026).** Codex explicitly selects full public subprocess networking,
  denies high-confidence workspace key/certificate files plus its raw auth
  store to sandboxed subprocesses, and pins the built-in secret-bearing
  environment filter. Claude removes raw Anthropic, OpenAI, and AWS credentials
  from sandboxed Bash, protects transcripts, memory, session state, settings,
  and auth caches with narrow home-directory denies that leave the official
  plugin runtime readable, and asks before restore, common checkout-discard,
  force-push, flag-based remote-delete, prune/mirror, forced local-branch
  reset/move, and delete forms.
  Protected-branch hooks remain the backstop for deletion-refspec syntax that
  Claude's permission grammar cannot safely express. Public research, brokered
  tools/MCPs, Auto-at-CLI scope, and loopback UI testing remain available.
  Codex's automatic reviewer is a reviewer subagent, so consumers that require
  a human for every sandbox escalation must select `user` in the project or
  launch override and enforce it through managed requirements where available.
- **Linked worktrees no longer rewrite write-once document baselines.** Existing
  non-JSON user-owned files now keep their original shipped baseline and
  manifest hash, so a different worktree directory name cannot create unrelated
  scaffold drift. A missing baseline still self-heals from the current scaffold
  without touching the live file; newly adopted files and schema-versioned JSON
  keep their existing update behavior.
- **The Claude+Codex duo is host-neutral and evidence-gated (ADR-0023).**
  Claude Code hosts through the official Codex plugin; Codex App/interactive
  CLI hosts through a task-scoped interactive Claude CLI in tmux. Both models
  independently research/analyze/plan and Claude leads design. ADR-0035 now
  routes production and cross-lineage review per task while preserving the
  selected Claude primary's integrated judgment. A versioned
  dual-approved plan, reproducible evidence ledger, scenario-first tests, an
  80% coverage floor/90% target where measurable, UI-driven validation, and
  independent security review now form one shared quality contract.
- **Reverse-lane completion no longer relies on terminal stability.**
  Task-scoped Claude Stop/StopFailure hooks and `last_assistant_message` are
  the protocol signal. The new `codeflow hook delegate-turn` handler writes an
  owner-only terminal result exactly once, permits an exact retry to re-signal,
  rejects conflicting evidence, and releases only the matching tmux waiter;
  pane capture is limited to the dedicated task session after completion or
  bounded diagnosis. Doctor now checks both directions' inspectable
  prerequisites while requiring retained interactive canaries.
- **Batch automation no longer implies cross-vendor assurance.** The seeded
  Claude workflow replaces its misleading `duo` preset with an explicitly
  `single-vendor-assurance` preset and rejects old duo/plan-align semantics.
- **Codeflow's full local gate now enforces 90% aggregate line coverage.**
  `cargo llvm-cov --workspace --summary-only --fail-under-lines 90` runs as a
  full-mode target, matching the independent CI coverage threshold.
- **Security: four enforcement-bypass fixes from the pre-release review.**
  The destructive-command guard now tokenizes `rm` instead of pattern-matching,
  so `rm -r -f /`, `rm --recursive --force /`, `rm -rf -- /`, and `rm -rf $HOME`
  are blocked, not just the exact `rm -rf` spelling. The commit-standard merge
  exemption keys off a real merge (`MERGE_HEAD`), not the subject text, so a
  one-parent commit named `Merge ...` is fully checked. `codeflow update` rejects
  a manifest `dest` that is absolute or escapes the repo with `..`, closing an
  arbitrary out-of-repo file deletion via a tampered manifest. The pre-commit
  secret scan classifies each assignment by its own value, so a placeholder word
  in a comment no longer suppresses the first live quoted credential later on the
  line. Subsequent pre-release slices closed the additional destructive-command
  spellings and shell wrappers, scaffold symlink traversal, and vacuous
  coverage-exception handling found by the second pass. The shipped scaffold's
  accepted policy remains configurable and defaults dependency/security review
  to `warn` (ADR-0016); this repository now hardens both keys to `block` after
  triage. Only `secret_scan` is never relaxable in the shared product policy.
- **Codeflow's own Rust verification now includes format and documentation
  gates.** Local `codeflow test` and the independent CI Rust job both enforce
  `cargo fmt --check` and warning-free rustdoc alongside tests and clippy; the
  parity guard compares the complete Rust command set.
- **Dependency security is blocking in this repository.** The project-owned
  `git.security_review` and `git.dep_audit` levels are `block`, while the
  consumer scaffold keeps ADR-0016's configurable `warn` default.
- **Coverage thresholds now fail the test gate.** A configured per-file
  coverage threshold that a measured file misses fails `codeflow test --mode
  full` and the integrate gate, instead of being collected and silently
  ignored. A run with no coverage data recorded stays informational.
- **PR bodies now demand test evidence and digestible bullets.** The shipped
  PR template's `Verification` section becomes `## Testing` — required for any
  code change, carrying pasted test-summary output, the coverage number, the
  new tests added, manual/e2e evidence, and a plain "not tested" statement
  ("tests pass" as prose is a claim, not evidence). Every section is short
  one-line bullets — no paragraph-walls. The rule ships in the template, the
  AGENTS contracts (full and minimal), and `cf-ship`.
- **Enforcement is the floor; the tiers scale project-management (ADR-0019).**
  The `--minimal` tier now installs the complete four-plane enforcement floor,
  not just the pre-commit secret scan: the `commit-msg`, `pre-push`,
  `pre-merge-commit`, and `reference-transaction` git hooks, the scaffolded CI
  workflow (`codeflow-ci.yml`), the in-session `git-guard`/`exec-guard` +
  orient/summary hooks (`.claude/settings.json` and the `.codex/` starter), and a
  new lean `CLAUDE.md` all moved into the minimal tier alongside the armed policy
  it already shipped. `--standard` and `--full` are unchanged; they still add the
  method (cf-* skills, reviewer agents, the pipeline), the traceability spine, and
  project-management on top. The change is additive — nothing is removed from any
  tier.
- **Existing `--minimal` installs gain the enforcement floor automatically on
  their next `codeflow update`.** The update reconciliation installs manifest
  entries that are in-tier but missing on disk, so an old-minimal repo's next
  update adds the moved hooks, CI, settings, and Codex starter and records them —
  no re-init required.
- **The original commit standard is restored and block-enforced (ADR-0020).**
  The subject description is capped at 50 chars and the whole subject line at 72,
  and a commit body is again only `-` bullets (at most 3, each ≤ 72 chars) plus
  an optional `BREAKING CHANGE:` footer — a prose "story" body is now a blocked
  mistake. The rules ship armed at every tier via five new `git` policy keys
  (`commit_desc_max_len`, `commit_subject_max_len`, `commit_body`,
  `commit_body_max_bullets`, `commit_body_bullet_max_len`), added to an existing
  `policy.json` with their defaults on the next `codeflow update`.
- **A contract-surface tripwire nudges breaking-change discipline (ADR-0020).**
  A new `git.breaking_watch_paths` key (path globs, default empty) makes the
  commit-msg check — and `codeflow ci`, which reuses it — emit a WARN (never a
  block) when a commit touches a declared contract surface without a `type!:`
  subject marker or a `BREAKING CHANGE:` footer. Detection of a break stays a
  judgment call; the glob only prompts a confirm.

### Added

- **Bounded interactive review documents (ADR-0049, ADR-0050, ADR-0052).** Standard/full
  scaffolds gain the cross-harness `cf-present` skill and managed public
  document, primitive-token, and history schemas. The new `codeflow present`
  surface opens, updates, lists, resumes, exports, closes, and clears immutable
  local review sessions and delivers stable feedback envelopes at least once.
  One loopback-only authenticated service and a CodeFlow-owned isolated browser
  profile render a closed accessible block catalog with light/dark utility
  modes, inert HTML sandboxing, strict optional project primitive tokens, and
  self-contained read-only export. Versioned state is owner-private, project-
  keyed, and quota bounded; browser-owned profile/cache and small runtime
  controls use a separate derived owner-private root. Cleanup is identity-
  scoped; event recovery uses one bounded
  opened handle, diagram count/source/enhancement are capped, and native
  adapters use trusted platform paths, exact process identity, a shared
  allowlist-only child environment, creation-only Windows ACL hardening, and
  read-only owner/DACL verification. No daemon, remote viewer, product UI
  framework, or documentation portal is introduced. Native-path project keys,
  serialized creation quota enforcement, collection cardinality, versioned feedback
  resolution, exact re-anchoring/visible orphan states, and fail-closed orphan
  process recovery keep the bounded contract explicit. Native platform and
  browser qualification remains the explicit CAP-016/TSK-007 release boundary.
  All durable growth now reserves capacity before publication under one
  project-to-session-to-runtime-control lock order, while exact retries and
  cleanup remain usable for legacy over-quota state. Exact accepted and terminal
  feedback retries converge without growth; retention recomputes after each
  eviction; selected cleanup reports a retained named session while remaining
  isolated from unrelated state; interrupted creates, atomic temporaries, and
  trash recover only from exact names plus matching transaction proof; and relative Unicode export remains
  create-new and owner-private. One strict feedback ledger rejects impossible
  transitions; browser cleanup re-qualifies identity before forced escalation;
  exports are owner-private from creation; and browser limits mirror the
  server's note, text, selector, and payload bounds. Browser launch is serialized
  around one consumed record per attempt, reused PIDs take bounded exact-marker
  recovery, Windows proves real profile-resource release without assuming a
  POSIX lock file, every Windows append/lease file is private at creation, and
  Unix state-root environment paths cannot resolve into the worktree.

- **Transport-neutral durable delegate lifecycle (ADR-0036).** New
  `codeflow delegate init|arm|wait` commands and a schema-v2
  `hook delegate-turn --state-dir` mode drive a delegated harness turn through
  durable owner-only records — ready, armed, accepted, terminal — with SHA-256
  prompt binding, one outstanding turn, deterministic terminal correlation,
  and durable poisoning for session restarts, interrupts after acceptance,
  and ambiguous retries; a duplicate or digest-mismatched prompt submission
  is instead blocked (hook exit 2) with run state preserved. The binary
  never launches a harness or delivers a prompt; the host keeps transport,
  and the current event adapter is Claude hooks. Schema-v2 waiting is
  file-polled and tmux-free, while the legacy `--result` mode is
  byte-compatible and unchanged. State lives outside Git
  worktrees, carries digests instead of prompt text, caps raw hook input before
  parsing, and fails closed on native Windows (use WSL2). ADR-0037 narrows the
  transportable prompt boundary to non-empty canonical UTF-8 text with
  internal LF, no terminal line break, and no other control characters, rejected
  before turn creation, and
  pins a bounded paste-to-Enter settle with only one diagnosis-proven retry.
  `codeflow doctor`
  gains a thirteenth,
  Fail-severity `delegate-roundtrip` check that runs the installed binary
  through the full synthetic lifecycle — rebuild and reinstall the CLI
  (`cargo install --path crates/codeflow-cli`) before it can pass.
  The dated PR1 canary record covers the active Stop-hook set, a Unicode
  normalization case, live `prompt_id` binding, AskUserQuestion, and
  permission-response routing on the available macOS arm64 host. The reusable
  sibling-hook rejection procedure, full fake-TUI stress matrix, and broader
  native-platform evidence remain PR2/release gates.
- **Native-interactive model/harness qualification (ADR-0027).** Standard/full
  scaffolds gain `/cf-evaluate-model`: stable requirement-to-source-to-case
  traceability, balanced regression/capability cases, exact disposable fixture
  materialization, expected-versus-observed scoring, repeated full trials,
  baseline comparison, and marker+run-ID-gated cleanup. Subject models remain in
  supervised native Codex or Claude sessions with their actual tools; no
  headless model runner, CLI subcommand, CI model call, hard token-deletion
  gate, or generic cleanup surface is introduced.
- **A lazy PR body now fails CI mechanically.** When `codeflow ci` is given a
  PR/MR body, it checks the body's structure against three new `git` policy
  keys: `pr_sections` (level, default `block`) governs the check;
  `pr_required_sections` (default `["Summary", "Changes"]`) are headings every
  PR body must carry with real content — a section holding only template
  comments and bare `-` bullets counts as missing; `pr_code_sections` (default
  `["Testing"]`) are required only when the commit range touches non-docs
  files (docs-only = every changed path is `*.md`, `*.txt`, `LICENSE*`,
  `docs/**`, or a `.github` template — anything else, or a range whose files
  cannot be listed, counts as code). Leftover template placeholders — the
  paste-your-output stub, table rows of empty cells, bare `- CAP-`/`- EPC-`
  bullets — draw a warning naming their line, never a block. A run without a
  PR body skips the check, so local `codeflow ci` is unchanged; an existing
  `policy.json` gains the three keys with their defaults on the next
  `codeflow update`.
- **`codeflow policy explain` / `policy show` — the policy file is fully
  discoverable from the binary.** `explain` renders the complete
  `.codeflow/policy.json` key schema — every key's type, default (rendered live
  from the built-in defaults), valid values, purpose, and sharp edges (e.g.
  `allow` and `off` are both inactive levels) — grouped top-level/git/security;
  `show` prints the EFFECTIVE policy: each key's current value, whether it comes
  from the project file or the built-in default, and a loud flag on invalid
  values. Consumers get only the binary, so both need no source access; the
  schema registry is pinned to the policy struct's serde fields by a
  drift-guard test, so a new key cannot ship undocumented.
- **Strict `policy.json` validation — invalid config fails loudly, never a
  silent default-revert.** Malformed JSON, an unknown key, a wrong-typed value,
  an invalid enum value, or an unparseable `commit_ticket_pattern` regex is now
  a hard error naming every offending key, its value, and the valid set (e.g.
  `invalid value 'worn' for git.commit_ticket_required; expected one of: off,
  warn, allow, block`) — surfaced at the commit-msg git hook (exit 1),
  `codeflow ci` (exit 2, nothing verified), and `codeflow validate` (exit 1).
  Previously one invalid value made the whole file fail-parse and silently
  reverted EVERY key to the built-in defaults — including keys a consumer had
  hardened past them. The enforcement loaders keep their fail-safe fallback;
  the loud check is an explicit pre-check at those three surfaces.
- **Opt-in footer trailers and required footers, strict by default (ADR-0020).**
  The commit body stays bullets + a `BREAKING CHANGE:` footer only — every other
  trailer blocks — but a project can now open specific escape hatches via
  `policy.json`, all empty by default: `git.commit_footer_tokens` *allows* named
  trailers (e.g. `Signed-off-by`), and `git.commit_required_footers` *requires*
  them on every commit (e.g. DCO sign-off). Deliberately no populated default —
  in an agent-driven repo every default-allowed trailer is a slot an agent fills.
  Even when `Co-authored-by` is opted in, the `ai_attribution` rule still blocks
  an AI value; a human co-author passes only when the token is opted in.
- **Opt-in ticket references with an allow-vs-require split (ADR-0020).**
  `git.commit_ticket_keys` (default empty) *allows* ticket trailers (e.g. `Refs`,
  `Closes`); `git.commit_ticket_required` (default `off`; `warn`/`block`) makes a
  matching ticket *required*; `git.commit_ticket_pattern` (e.g. `^PROJ-\d+$`)
  constrains the value — a present-but-malformed reference blocks even when
  optional. Merge/revert/fixup commits are exempt; the git hook and `codeflow ci`
  enforce it identically.

- **Two working principles in both the minimal and full agent contracts.**
  *"Think independently — not a yes-man"*: a request, opinion, claim, or proposed
  approach — the operator's included — is owed analysis and evidence, not
  agreement; the operator still makes the final call, but agreement without
  examination is a failure mode, not deference. *"Think in depth, not at the
  surface"*: chase the implication chain ("and therefore? …") to the fundamental
  that decides the matter, and trace how each order ripples across the related
  domains, not just the immediate area.

### Fixed

- **Brownfield setup preserves the consuming project's decision history.**
  Init and update no longer add the starter stack `ADR-0001` when an existing
  repository already has ADRs. Test setup now installs the JSON schema beside
  generated `.codeflow/test-config.json` files, uses a correctly relative
  `$schema` reference, and repairs a missing schema without replacing a
  populated project configuration. Repositories initialized by an affected
  prerelease build should delete its duplicate starter ADR once; subsequent
  updates leave it deleted. Previously populated test configs are intentionally
  not rewritten; regenerate one to adopt the corrected `$schema` reference.
- **Public contribution and security guidance is release-neutral.** The
  contributor path now names the complete repository gate, including coverage
  and model-evaluation contracts, and the security policy supports the latest
  released major line without going stale at the v3 cut.
- **CI verifies security-tool downloads before executing them.** The shipped
  workflow and CodeFlow's own perimeter pin the official SHA-256 digests for
  Gitleaks and OSV-Scanner, fail closed on a mismatch, and retain the exact
  release versions already validated by the repository.
- **Auto-detected test configs now run with each stack's declared baseline
  tools.** Rust no longer assumes a project-defined nextest `full` profile;
  Node, Go, and Python no longer silently require optional report or coverage
  plugins that detection did not prove were installed. Generated targets use
  conservative native commands and leave richer reports and coverage to
  project customization. The empty-stack message also stops recommending an
  unimplemented `--add-target` flag, and coverage output now states honestly
  that configured thresholds affect the gate verdict.
- **Dependency advisories and maintenance debt.** `anyhow`, `git2`, and
  `quick-xml` move to versions that clear the active RustSec advisories; the
  unmaintained `serde_yaml` parser is replaced by the maintained
  `serde_yaml_ng` continuation behind the same source alias (ADR-0022).
- **Test code no longer mutates process-global environment variables from
  parallel unit tests.** CI detection is injected into the runner tests, and
  child Git-environment removal is verified through `Command` construction,
  eliminating the Rust-2024-unsafe `set_var` / `remove_var` calls and the race
  they represented.
- **Public API documentation builds warning-free.** Broken intra-doc links in
  the hook and policy modules and one redundant CLI link are corrected; the new
  rustdoc gate prevents regression.
- **Live reviewer definitions match the shipped scaffold sources.** The
  repository's `cf-reviewer` tier guidance and `cf-security-reviewer` CI posture
  now carry the same corrected doctrine as newly initialized projects.
- **The full-history secret scan ignores one reviewed synthetic fixture by its
  exact fingerprint.** The historical line demonstrated scanner behavior and
  contained no credential; `.gitleaksignore` suppresses only that immutable
  finding rather than weakening a rule.

<!-- codeflow:legacy-group-end -->

## [2.1.0] - 2026-07-12

### Added

- Cross-harness skills: the `cf-*` skills install to `.agents/skills/` (read by
  Codex) alongside `.claude/skills/`, so one skill set serves multiple coding
  CLIs. Claude Code merged custom commands into skills (v2.1.101); codeflow now
  ships skills only.
- The commit-msg gate flags a mis-cased `BREAKING CHANGE:` / `BREAKING-CHANGE:`
  footer, so a breaking change is never silently downgraded to a minor bump.

### Fixed

- `codeflow update` no longer silently discards merged-in user edits to a
  managed file on the *next* update. After a clean 3-way merge the manifest now
  records the pristine shipped hash (restoring the invariant `recorded ==
  hash(baseline)`), so a merged file stays classified user-modified and is
  re-merged rather than overwritten with the shipped version.
- `codeflow update` reconciles orphaned managed files: an artifact removed or
  renamed upstream (e.g. a command that became a skill), and its baseline and
  manifest record, is pruned when unmodified instead of lingering and colliding
  with its renamed replacement. User-modified and user-owned files are never
  deleted (ADR-0011).

### Changed

- Shipped scaffold content and documentation corrected for the commands→skills
  rename and for install/enforcement accuracy ahead of the public release.

## [2.0.0] - 2026-07-03

2.0.0 is a complete Rust rewrite of codeflow. The 1.x line (a shell/Node tooling
set) shares no code with it and is preserved at the `v1-final` tag.

### Added

- Single `codeflow` binary (crates `codeflow-core` + `codeflow-cli`) with an
  embedded scaffold, installed into any repo via `codeflow init`.
- Policy-driven git enforcement across four planes — git client hooks, an
  in-session PreToolUse guard, CI, and remote branch protection — all reading a
  single `.codeflow/policy.json`.
- Scaffolding with three ownership classes (managed, managed-region,
  user-owned), 3-way-merge `codeflow update`, and version-skew detection.
- Knowledge model: capabilities registry, ADRs, epics/specs, an automatic
  ledger, and `codeflow recall` full-text search.
- Composable pipeline workflow (build → independent review → verify) and
  cross-vendor delegation (`cf-delegate` / `cf-consult`).
- CLI surface: `init`, `update`, `test`, `validate [--docs]`, `status`,
  `recall`, `orient`, `doctor`, `integrate`, `remote`.
- cargo-dist release pipeline with prebuilt binaries for macOS (arm64/x64) and
  Linux (x64) and a shell installer.

[Unreleased]: https://github.com/sathyassn/codeflow/compare/v3.0.0...HEAD
[3.0.0]: https://github.com/sathyassn/codeflow/compare/v2.1.0...v3.0.0
[2.1.0]: https://github.com/sathyassn/codeflow/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/sathyassn/codeflow/releases/tag/v2.0.0
