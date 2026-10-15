# Package-manager script shell settings: design note

Task: TSK-285. Issue: 89. Status: revision 3, after Codex's round-2 review
of a5b164088 (`changes_requested`, R2-1 to R2-4, N1 to N3); no guard code
exists yet. Written against the EPC-022 line at `1e6e29e93`, PR 195
(TSK-283) at `6fd46ab21`, PR 187 (TSK-302) at `9c2dadea6` and plan PR 162
(TSK-299) at `d9de22f2b`.

## Summary

npm runs every package script through the program its `script-shell`
setting names, and pnpm and yarn 1 read the same key. The setting comes
from a project file, the user's file, a global file, an environment
variable or a command-line flag, so an agent that sets it makes every
later `npm test` run a chosen program while the guards see `npm test`.
This note lists the settings that choose a shell or script runner in npm,
yarn and pnpm, says how each is guarded, and names what the backstop
cannot see.

The recommendation: one new action-table family refuses the commands that
set, delete or point at a runner key in every accepted spelling, with each
manager's option and command namespace normalized before the key is
compared; edit-guard and exec-guard judge a write to a package-manager
file by the configuration it produces, comparing the runner keys before
and after and refusing when the result cannot be established; the rule is
`security.package_runner`, no key, always blocking. No value is exempt.
edit-guard is wired for Claude's edit tools, so the native refusal AC-3
requires exists on every harness. The file class is each manager's own
discovery list, taken from its installed source.

Changed since revision 2: the invocation check (operator decision D1,
pending) covers every package-manager command except a read-only set the
build proves, since `npm update` reifies and runs install scripts
(R2-1); its configuration reader takes the application's directory
options and reads every ancestor, a superset of npm's workspace-root lift
(R2-2); pnpm's `--config.<key>` namespace and yarn's `YARN_CONFIG`
pointer are in the command rules (R2-3); yarn's class is its `getRcPaths`
list, `~/.config/yarn/config` included (R2-4); `npm config delete` of a
runner key refuses like a set (N3); the reader's bounds replace a
constant cost (N1); sources are pinned (N2).

## The settings

Verified on this host unless marked: npm 10.9.2 (its installed
`docs/content` and `lib/`, and probes in a scratch `HOME` with
`NPM_CONFIG_USERCONFIG` and `NPM_CONFIG_GLOBALCONFIG` pointing at empty
scratch files); pnpm 10.30.1 (the bundle at
`~/Library/pnpm/.tools/pnpm/10.30.1/`, and probes with scratch `HOME` and
`XDG_CONFIG_HOME`); yarn 1.22.19 (`lib/cli.js`, and probes with a scratch
`HOME`). Codex's reviews probed Homebrew pnpm 10.30.0 in round 1 and the
10.30.1 artifact in round 2; where both were exercised the behavior
agreed. Yarn Berry and Bun are not installed here; their rows cite pinned
upstream sources.

| Tool | Setting | Read from | What it does | Guard |
|---|---|---|---|---|
| npm | `script-shell` | the files and forms below | the shell for `npm run`, `npm exec`, `npm init <spec>` and every lifecycle script (default `/bin/sh`, `cmd.exe` on Windows); probe: `npm test` from the project root and from a subdirectory ran the runner the project `.npmrc` named; review probe: `npm update` of a local dependency ran its install script through it | runner key: any change refuses |
| npm | `node-options` | same | passed to lifecycle scripts as `NODE_OPTIONS`; `--require`, `--import` and `--loader` load a file into every script's node | runner key |
| npm | `init-module` | same | a JavaScript module `npm init` loads (default `~/.npm-init.js`) | runner key |
| npm | `userconfig`, `globalconfig` | `NPM_CONFIG_USERCONFIG`, `NPM_CONFIG_GLOBALCONFIG`, `--userconfig`, `--globalconfig`, the files | point npm at another configuration file | pointer key: refused whatever the value |
| npm | `editor`, `shell`, `browser`, `viewer` | same | run a program for `npm edit`, `npm config edit`, `npm explore`, `npm docs`, `npm help`; probe: `npm config edit` ran the program `EDITOR` named | allowed, stated: each runs only on a command the agent spells; `npm config edit` is refused below |
| npm | `prefix` | same | where `-g` installs put binaries | allowed: the destination is a `PATH` directory TSK-284 protects (under its D1) |
| pnpm | `script-shell` (`scriptShell` in `pnpm-workspace.yaml`) | `.npmrc` (project, workspace root, user), the global rc, `pnpm-workspace.yaml`, `npm_config_*`, `--config.<key>`, `--<key>`; probe: `scriptShell: <path>` in `pnpm-workspace.yaml` ran the runner on `pnpm test`; review probe: `pnpm --config.script-shell=<path> run test` ran it | runner key |
| pnpm | `shell-emulator` (`shellEmulator`) | same | selects pnpm's own JavaScript shell | allowed: both values are pnpm's own code |
| pnpm | `node-options` (`nodeOptions`), `npm-path` (`npmPath`), `use-node-version`, `node-mirror:*` | same | as npm's; the npm binary pnpm runs; a node pnpm downloads from the named mirror and runs | runner keys |
| pnpm | `pnpmfile`, `global-pnpmfile`, `ignore-pnpmfile` | same | which hooks file pnpm runs at install (default `.pnpmfile.cjs`); `global-pnpmfile` loads a module on every install (`dist/pnpm.cjs`) | `pnpmfile` and `global-pnpmfile`: runner keys. The default file is repository code; see residuals |
| yarn 1 | `script-shell` | the `getRcPaths` list below, `yarn_<key>` variables, `--use-yarnrc FILE`, `YARN_CONFIG`; probes: a project `.yarnrc`, an ancestor's `.yarnrc` and `yarn_script_shell=` each ran the runner on `yarn test` | runner key |
| yarn 1 | `yarn-path` | same; review probes: `yarn --use-yarnrc alt.rc --version`, `YARN_CONFIG=alt.config yarn --version` and a `~/.config/yarn/config` entry each ran the file the configuration named | runner key: it runs on every yarn invocation; `yarn policies set-version` writes it |
| yarn Berry (upstream source, pinned, not run here) | `yarnPath`, `plugins`, `injectEnvironmentFiles` | `.yarnrc.yml` in the directory and every ancestor, `~/.yarnrc.yml`, the file `YARN_RC_FILENAME` names (`Configuration.ts` `getRcFilename`), `YARN_<KEY>` variables, `yarn config set`, `yarn set version`, `yarn plugin import` | runner keys; `YARN_RC_FILENAME` is a pointer. Berry has no script shell key |
| Bun (upstream documentation, not run) | `bunfig.toml` `[run] shell` | the project file (`bun run` loads the project `bunfig.toml`, not the global one) | chooses `system` or `bun`, both Bun's own code | out of scope with that evidence; not a runner path |

**Key spelling and namespaces.** npm reads `npm_config_FOO` and
`NPM_CONFIG_FOO` alike and maps `_` to `-`; npm's ini reader accepts a
quoted key (`"script-shell"=x`, review probe) and expands unambiguous
option abbreviations (`--script-s=x`, probe); pnpm reads dashed and camel
forms and takes any setting as `--config.<key>=value` (review probe); YAML
decodes scalar escapes (`"\x73criptShell"`, review probe). The guard
therefore never matches spellings in text: it parses each file with that
file's grammar, decodes keys and values fully, strips each manager's
option namespace (`--config.` for pnpm, the `--` and `npm_config_` and
`yarn_` prefixes) before comparing, and compares normalized key names
(case-folded, `_` and `-` and the camel form as one). Anything the parser
cannot read is a refusal.

## How each form is guarded

```text
   form                                        layer                      rule
   ──────────────────────────────────────────  ─────────────────────────  ────────────────────────
   npm config|conf|c set|delete, npm set,      native deny + exec-guard   security.package_runner
     any pair form, any --location, -g
   npm config edit; yarn config set; yarn      native deny + exec-guard   security.package_runner
     policies set-version; yarn set version;
     yarn plugin import; pnpm config set
   --script-shell X and every abbreviation,    exec-guard parse           security.package_runner
     pnpm --config.<runner key>, --userconfig,
     --globalconfig, --use-yarnrc
   npm_config_*, NPM_CONFIG_*, yarn_*, YARN_*, exec-guard                 security.package_runner
     YARN_CONFIG, YARN_RC_FILENAME
   Edit/Write/apply_patch of a class file      edit-guard, before/after   security.package_runner
   >> .npmrc, tee, sed -i of a class file      exec-guard, before/after   security.package_runner
   any npm/pnpm/yarn command not proven        invocation check (D1)      security.package_runner
     read-only, under an effective runner
```

### Commands

A new family `runner_settings` in `security/actions.json`, decision
`deny`, generates the native text rules for the three harnesses (the
publish family is the template; its `codex_note` on leading flags applies)
and exec-guard parses the unwrapped argv in `security/outward.rs` beside
the publish rows, which already skip npm's, pnpm's and yarn's global
options (`outward.rs:495-505`):

- **Command words.** npm resolves aliases (`lib/utils/cmd-list.js:76-110`:
  `c` and `conf` for `config`, `i` and `add` for `install`, `up`, `rb`,
  `ln`, `ddp`, `it`, `cit`, `t`, `tst`, `run`, `create`, `un`, `rm`, `r`,
  `remove`, `unlink`, `clean-install`) and then unambiguous command
  abbreviations (`abbrev` at `:162`: `npm inst`, `npm upd`). exec-guard
  resolves the command word the same way before any rule reads it; an
  ambiguous abbreviation refuses.
- `npm config set`, `npm config delete` and `npm set`, in every alias,
  with a runner or pointer key in any pair form (`key value`, `key=value`,
  several pairs), at any location (`--location=project|user|global`, `-g`,
  `--global`, `--userconfig F`). Probes: `--location=project` writes the
  project `.npmrc`, the default writes the user file, `-g` the global
  file. Deletion refuses like a set (N3): it changes the ordered runner
  list, and the review probe showed a deleted project override exposing a
  global runner; without D1 that exposure would otherwise pass unnoticed.
  `npm config get` and `list` pass.
- `npm config edit` (and `conf`, `c` forms): refused, since the guard
  cannot see what the editor writes.
- Any npm, pnpm or yarn command carrying an option whose dashed name,
  after the manager's namespace is stripped, is a prefix of a runner or
  pointer key (`--script-shell`, `--script-s`, `--node-opt`,
  `--init-module`, `--userconfig`, `--userc`, `--globalconfig`, pnpm's
  `--config.script-shell`, `--config.node-options`), attached or separate;
  an ambiguous prefix refuses too. Words after `--` are not options and
  are not read. The build sweeps each manager's option grammar (nopt for
  npm, pnpm's `--config.` and `--<key>`, yarn 1's argument parser) with
  controls for ordinary `--config.*` and `--<key>` options.
- An assignment on the line, exported or set for the command, of
  `npm_config_<key>` or `NPM_CONFIG_<KEY>` for a runner or pointer key
  (probe: `yarn_script_shell=` selects the runner for yarn 1), of
  `yarn_<key>`, of `YARN_CONFIG` (yarn 1's configuration pointer,
  `lib/cli.js:97385`), or of `YARN_<KEY>` for a Berry runner key or
  `YARN_RC_FILENAME`. These join the command-valued variable check of
  `security/startup.rs` as variables that pick the program: refused
  whatever the value.
- `pnpm config set` and `delete` with a runner key at any location. Probes
  on 10.30.1: the default location is the global rc
  (`$XDG_CONFIG_HOME/pnpm/rc`), and `--location project` writes
  `pnpm-workspace.yaml` in camel case.
- `yarn config set` and `delete` with `script-shell`, `yarn-path`,
  `yarnPath`, `plugins` or `injectEnvironmentFiles` (probe: yarn 1 writes
  `~/.yarnrc`); `yarn policies set-version`; `yarn set version`; `yarn
  plugin import`; `yarn --use-yarnrc F` with any command.

The refusal names the rule and the sanctioned path: the operator sets or
removes a script runner in their own terminal, outside the agent session;
no policy key relaxes the rule.

### Writes to the files

**The file class**, each manager's own discovery list (R2-4):

| Tool | Project files | User and global files |
|---|---|---|
| npm (`@npmcli/config/lib/index.js:639-693`, `loadLocalPrefix`) | `.npmrc` in the local prefix: the nearest ancestor of the command's directory holding `package.json` or `node_modules`, lifted to an ancestor whose `package.json` declares a `workspaces` entry that contains it (review probe: a child package's `npm test` read the root `.npmrc`); `--prefix`, `-C`, `--workspace`, `-w`, `--workspaces` move it | `~/.npmrc`; `$PREFIX/etc/npmrc` (`/opt/homebrew/etc/npmrc`, `/usr/local/etc/npmrc`, `<tool-manager install>/etc/npmrc`, `%APPDATA%\npm\etc\npmrc`); the builtin `<npm>/npmrc`; the targets of `NPM_CONFIG_USERCONFIG` and `NPM_CONFIG_GLOBALCONFIG` when the hook's environment carries them |
| pnpm | `.npmrc` in the command's directory and the workspace root; `pnpm-workspace.yaml`; `--dir` and `-C` move the directory (review probe: `pnpm --dir child test` read the child's files) | `~/.npmrc`; `$XDG_CONFIG_HOME/pnpm/rc` (default `~/.config/pnpm/rc` on Linux, `~/Library/Preferences/pnpm/rc` on macOS, `$LOCALAPPDATA/pnpm/config/rc` on Windows; probe: the XDG file was read) |
| yarn 1 (`lib/cli.js:97331-97385`, `getRcPaths`) | `.yarnrc` and `.yarnrc.yml` in the command's directory and every ancestor; `--cwd` moves the directory (review probe) | `/etc/yarn/config`, `/etc/yarnrc` (not on Windows); the configuration directory (`CONFIG_DIRECTORY`: `~/.config/yarn` on Unix, `%LOCALAPPDATA%\Yarn\Config` on Windows); `~/.config/yarn/config`, `~/.config/yarn`, `~/.yarn/config`, `~/.yarnrc` and `~/.yarnrc.yml`; the file `--use-yarnrc` or `YARN_CONFIG` names. Yarn skips an absent path and a directory (`ENOENT`, `EISDIR`) and fails on any other read error |
| yarn Berry (`Configuration.ts` at the pinned revision) | `.yarnrc.yml` in the command's directory and every ancestor | `~/.yarnrc.yml`; the file `YARN_RC_FILENAME` names |

Table encoding (`package_manager_paths` in `actions.json`): `anywhere`
`.npmrc`, `.yarnrc`, `.yarnrc.yml`, `pnpm-workspace.yaml`; `home`
`.npmrc`, `.yarnrc`, `.yarnrc.yml`, `.yarn/config`, `.config/yarn/config`,
`.config/yarn` (judged as a file when it is one), `.config/pnpm/rc` (the
`.config/` entries relocate with `XDG_CONFIG_HOME` as the startup class's
do), `Library/Preferences/pnpm/rc`, `AppData/Roaming/npm/etc/npmrc`;
root-prefixed `$LOCALAPPDATA/pnpm/config/rc` and
`$LOCALAPPDATA/Yarn/Config/` (the environment-root mechanism TSK-284's
table introduces, so the Windows paths are no longer written as if they
were home-relative); `absolute` `/opt/homebrew/etc/npmrc`,
`/usr/local/etc/npmrc`, `/etc/yarnrc`, `/etc/yarn/config`; and a path-end
needle `etc/npmrc` (as `etc/gitconfig` and `git/config` are, `startup.rs`
`PATH_END_NEEDLES`) for the global file under any prefix. The pointer
targets from the hook's environment (`NPM_CONFIG_USERCONFIG`,
`NPM_CONFIG_GLOBALCONFIG`, `YARN_CONFIG`, `YARN_RC_FILENAME`) join at
evaluation. Absence is empty; a directory at a file's path is skipped as
yarn skips it; any other read failure is "cannot verify" and refuses.

**The judgment: before and after.** The inserted text does not establish
the resulting configuration: an `Edit` of `=sh` to `=./runner` carries no
key, deleting a later duplicate reveals an earlier runner, and a `Write`
that preserves an existing runner must stay allowed. So a write to a
class file is judged by the configuration it produces:

1. Read the current file. A path that is a symbolic link, not a regular
   file, unreadable or not valid UTF-8 refuses; an absent file is empty.
2. Compute the resulting text: `Write` and `write` give it whole; `Edit`
   and `search_replace` replace `old_string` with `new_string` once, or
   everywhere with `replace_all`, and refuse when the old string is absent
   or ambiguous; `apply_patch` applies its hunks and refuses when they do
   not apply. This is the hook's own application; the tests must show it
   produces the same bytes as the tool on each supported form, and any
   doubt is a refusal, never an allow.
3. Parse both texts with the file's grammar: npm's ini (quoted keys and
   values, `key[]` arrays, `;` and `#` comments, `${VAR}` references,
   which make a runner value unreadable and refuse); yarn 1's `key value`
   lines with optional quotes and `--flag` lines; YAML through the
   project's existing parser with full scalar decoding and nesting
   preserved (`pnpm-workspace.yaml`, `.yarnrc.yml`, including the
   `plugins` list entries). A parse failure of either text refuses.
4. Compare the ordered list of `(key, value)` occurrences of every runner
   and pointer key, since last wins and order changes the effective
   value. Identical lists allow; any difference refuses, naming the key
   (never the line: `~/.npmrc` holds registry tokens, and the reader
   logs no values other than the runner key's).

Shell writes use the same comparison where the resulting text can be
computed, and refuse otherwise: a redirect or `tee` of literal text (an
`echo` or `printf` argument with no expansion, or a quoted-delimiter
heredoc) gives the appended or replacing text; `sed -i` gives it only when
TSK-290's reader (`security/sed_script.rs`) parses the whole script into
`s` commands with literal patterns (at most `^` and `$`), literal
replacements and the flags `g`, `i` or a number, and `d` commands with a
literal line or literal-pattern address, which a bounded evaluator in
this task applies line by line. The reader today exposes targets and
executed commands and keeps substitution text only for an `e` command
(`sed_script.rs:658`, `:682`); `produces_text` returns a Boolean
(`startup.rs:1730`). Neither reconstructs output, so this task owns the
evaluator and states its scope. Everything else into a class file refuses:
`cp`, `curl -o`, `sort -o`, an unquoted heredoc, a variable, `sed -f`, a
`w`, `r` or `e` command, an interpreter whose operand or supplier is a
class file (`python3 edit.py .npmrc`, `find -execdir`, `xargs`), read from
PR 187's `InterpreterCall` operands and suppliers. The cost: an agent
edits these files with the editor tools or by appending literal lines;
other shapes go to the operator.

**Sandbox entries.** The home and global files get Claude `Edit` denies
and sandbox `denyWrite` entries and Codex read-only entries, generated
from the table as the git configuration files are; `~/.npmrc` gets a
Claude read deny as well, since it holds the registry token. Grok gets
none: every npm command reads these files and a Grok deny blocks reads.
The project files get no sandbox entry. The home is already unwritable in
all three sandboxes, so the entries change nothing in practice except
where a writable root contains the home; there the sandbox refuses a
user-scope registry edit the guard would allow, as it does for `git
config --global user.name` today. Where the key-level allow holds: every
project file on every harness, and the home files through the guards; a
broader refusal applies to the home files under a sandbox whose writable
root contains them, and that is the stated contract.

**Native edits on Claude.** The Claude preset wires hooks for
`Bash|PowerShell` only (`assets/base/settings/default.json:531`), so no
edit-guard runs on Claude's `Edit` and `Write`, and a project file cannot
carry a native deny without blocking every edit to it. AC-3 requires the
refusal on every harness, so the preset gains a `PreToolUse` entry with
matcher `^(Edit|Write|MultiEdit|NotebookEdit)$` running `codeflow hook
edit-guard --contract 3` with the same fail-closed wrapper as the Bash
hooks, and `edit_guard::parse_payload` learns `MultiEdit`'s `edits` list
and `NotebookEdit`'s `notebook_path`. Cost: one hook start per Claude
edit, measured in the build against the hook budget; `codeflow update`
merges the entry as it merges the Bash hooks. This also closes TSK-284's
Claude `Write` gap for dynamic `PATH` entries and the startup class's
moved-`ZDOTDIR` gap. If the operator prefers not to wire it, AC-3 must be
narrowed to Codex and Grok through a planning amendment that states an
agent on Claude can plant a runner in a project file; that is not assumed
here.

**Rule.** `security.package_runner`, no policy key, always blocking, in
the policy reference's "Rules with no key". A project that needs a custom
runner sets it in the operator's terminal; the guard passes every edit
that leaves the runner keys unchanged.

### Why no value is exempt

Codex's round-1 probe showed `script-shell=sh` with an executable
`node_modules/.bin/sh` in the project ran that file on `npm test`: npm
prepends `node_modules/.bin` to the `PATH` it hands the shell (npm's
`scripts` documentation, "path"), so a bare name resolves inside the
checkout and a malicious pull request can supply it. TSK-284 protects the
user's `PATH` directories, not this package-local lookup. Any change to a
runner key refuses. The compatibility cost: a project that sets
`script-shell=bash` for Windows scripts keeps the line, since an unchanged
list passes; adding or removing it is the operator's act. The same fact
bears on D1 below.

## The invocation check (operator decision D1, pending)

The commands above stop an agent from planting a runner. They do nothing
about a runner that arrived in the checkout: a pull request adds
`script-shell=./scripts/sh.sh` to `.npmrc` and the agent's next `npm test`
runs it. The check:

- **Covered commands (R2-1).** The lifecycle page does not enumerate
  every command that installs or rebuilds dependencies: the review's
  offline probe showed `npm update` of a local dependency running its
  install script through the configured runner. A sweep of the installed
  `npm/lib/commands/` finds `Arborist` or `reify` in `audit`, `ci`,
  `dedupe`, `diff`, `explain`, `find-dupes`, `fund`, `init`, `install`,
  `link`, `ls`, `outdated`, `prune`, `query`, `rebuild`, `sbom`,
  `shrinkwrap`, `uninstall` and `update`, and script execution in `ci`,
  `explore`, `install`, `publish`, `restart`, `run-script`, `start`,
  `stop` and `test`; `exec`, `npx`, `version`, `pack`, `edit` and `init`
  run scripts through other paths. Some of the Arborist users only load
  the tree. So the check is conservative: every npm, pnpm and yarn command
  (after alias and abbreviation resolution) refuses under an effective
  runner, except a read-only set the build proves with the offline
  local-dependency fixture and an install-script marker (candidates:
  `view`, `search`, `ls`, `explain`, `outdated`, `query`, `sbom`, `fund`,
  `config get` and `list`, `help`, `ping`, `whoami`, `prefix`, `root`,
  `pkg get`, `cache ls`, `doctor`; a candidate that runs the marker is
  not exempt). yarn 1 and Berry: every invocation, since `yarn-path` and
  `yarnPath` run on `--version`. pnpm: the same conservative rule.
- **Configuration roots (R2-2).** Three things are kept apart: the
  shell's working directory, the manager's configuration root, and the
  packages whose scripts run. The start directory is the application's
  own directory option when given (`--prefix`, `-C`, `--workspace`, `-w`
  and `--workspaces` for npm; `--dir`, `-C` and `--filter` for pnpm;
  `--cwd` for yarn), resolved against the line's `DirSet`; an unresolvable
  option or an unknown `DirSet` refuses as "cannot verify". From each
  start directory the reader takes the class files of every ancestor up
  to the filesystem root, then the user, global and builtin files and the
  pointer targets. That is a superset of npm's `loadLocalPrefix` lift to
  the workspace root, of pnpm's cwd-plus-workspace-root read and of yarn's
  ancestor walk, so a parse of the wrong files cannot allow: the review's
  root-to-child and child-to-root probes both refuse because the root
  `.npmrc` is an ancestor file. Cost: an ancestor file outside the
  project with a runner refuses.
- **Precedence and environment.** The command line (namespace-stripped
  options), then the environment (the hook's own plus the line's
  assignments), then the files in each manager's order. The verdict
  needs no exact precedence: any runner key with a non-empty value
  anywhere in the chain refuses, naming the scope and origin, with no
  exempt value.
- **The reader.** A bounded file reader, never the package manager
  itself: running `yarn` or `npm` to ask for a setting would run the
  planted runner (the `--version` probes). It follows PR 195's contract in
  shape: `get_all(key)` returns every occurrence as `(scope, origin,
  value)` in precedence order, and anything it cannot read returns
  "cannot verify", which refuses. Bounds (N1): each file at most 256 KiB,
  the ancestor walk bounded by the path's depth, at most 64 files per
  judgment; exceeding a bound is "cannot verify". Timing is unmeasured;
  the build measures it against the 10 s hook budget.
- **Relief.** The operator removes the key, or runs the command in their
  own terminal. A per-project allowance would be a new policy key under
  TSK-280's placement rule and needs the operator's approval as process;
  it is not proposed here. Setting the runner by hand does not relieve
  this check; that relief belongs to the write rules only.
- **Reporting is separate.** `codeflow ci` reporting a tracked runner key
  in the pull request evidence is a second deliverable, for the reviewer
  and for CI, and is reporting, not enforcement; CI and the operator's
  terminal run outside every guard.

| Decision | Protection | Cost | If the operator declines |
|---|---|---|---|
| Add the invocation check to TSK-285, or as one follow-up task (recommended) | Planted checkout configuration cannot run scripts in an agent session through any command not proven read-only; the planted key is named at first use | Bounded reads per covered command (the bounds above; timing unmeasured); a project with a legitimate custom runner, an ancestor file carrying one, or an unreadable or over-bound configuration refuses until the operator acts; three small readers (ini, yarn 1, YAML) | Planted checkout configuration stays executable in agent sessions; the write rules and the reporting row stand, and this task's delivered protection is described as the no-D1 branch until a follow-up lands; the residual is stated in the policy reference |

## Composition with the in-flight guard work

| Work | How this task composes |
|---|---|
| TSK-283, PR 195 at `6fd46ab21` (`security/git_config.rs`: `read` runs a bounded git subprocess, `get`, `get_all(key)` with `(scope, origin, value)`, "cannot verify") | The result contract is reused in shape; the precedence and the subprocess are not: npm's precedence is npm's, and a package manager must never be run to read its configuration. `get_all` is the right shape because a last-value getter cannot show that a deletion exposed another value |
| TSK-302, PR 187 at `9c2dadea6` (`DirSet { dirs, unknown }`, `InterpreterCall` with operands, code, cwd and suppliers, `line_directories`) | The directory set resolves relative class files and the start directory of the invocation check; an unknown set with a bare `.npmrc` tail refuses, as PR 187's rule does for `settings.json`; interpreter operands and suppliers naming a class file refuse. The manager's own directory options (`--dir`, `--cwd`, `--prefix`) are read by this task on top of the `DirSet`, since the shell reader cannot know them. Landing: after PR 187, or with a local shim of the same shape that is replaced when it lands |
| TSK-299, plan PR 162 at `d9de22f2b` (planned: `sort -o`, `curl -o`, `wget -O`, nested sed bodies, in `hooks/git_guard.rs`) | Not landed; not credited. The class judges every writer in `security/unresolved.rs` through `judge_writer` today (`curl` and `wget` are in `WRITERS`, `sort` is not); `sort -o .npmrc` refuses as unreadable text once `sort` has an entry, and this task adds that entry itself if TSK-299 has not landed first. Tests for `sort -o`, `curl -o`, `wget -O` and an unresolved directory run through the package-file judge |
| TSK-290 (landed, `security/sed_script.rs`) | Supplies the parsed script; this task owns the bounded evaluator over it, with byte-equality tests against the platform `sed` on each supported form |
| TSK-284 | `prefix`, `corepack enable` and `npm i -g` land in `PATH` directories TSK-284 protects; the two tables are siblings and share the environment-root entry mechanism; the Claude edit-guard wiring here serves both; both touch `edit_guard.rs`, so they land in sequence |

## Adversarial cases

| Case | Guards | Sandbox | Open |
|---|---|---|---|
| `npm config set script-shell /tmp/x`, `npm conf set ... --location=project`, `npm set`, `npm c set`, `npm inst`-style abbreviations, through `bash -c`, `eval`, `env` or a launcher the reader unwraps | refused | the user file is denied; a project file is writable | a launcher the reader does not unwrap, which the closed rule refuses only when the line names a class file |
| `npm config delete script-shell` where a global runner lies beneath | refused: a deletion changes the ordered list | | |
| `npm --script-s=/tmp/x config get script-shell`, `npm test --script-shell=/tmp/x`, `pnpm --config.script-shell=/tmp/x run test`, `npm run build --userconfig /tmp/evil.npmrc`, `yarn --use-yarnrc /tmp/alt.rc test` | refused: namespace-stripped prefix matching and pointers | not held | |
| `npm_config_script_shell=/tmp/x npm test`, `NPM_CONFIG_SCRIPT_SHELL=`, `yarn_script_shell=`, `YARN_CONFIG=/tmp/alt.config yarn --version`, `YARN_RC_FILENAME=`, `export ... ; npm test` | refused | not held | a variable exported in an earlier tool call: one call, not persistence; under D1 the hook's own environment is read, so a pointer inherited at launch is read or "cannot verify" |
| `Edit` of `=sh` to `=./runner`; `sed -i 's|=sh|=./runner|' .npmrc`; deleting a later duplicate (`sed -i 2d`); a `Write` of the whole file with a new runner | refused: the runner list differs before and after | | |
| A `Write` that changes the registry and keeps an existing runner line verbatim; `Edit` of the registry line; `echo 'registry=https://r.example' >> .npmrc` | allowed: the runner list is unchanged | | |
| `echo "$LINE" >> .npmrc`, `cp /tmp/rc .npmrc`, `curl -o .npmrc`, `sort -o .npmrc`, `sed -i -f s.sed .npmrc`, `python3 edit.py .npmrc` | refused: the result cannot be computed | | |
| `.npmrc` with `"script-shell"=/tmp/x`; `pnpm-workspace.yaml` with `"\x73criptShell": ./runner`; `script_shell=`, `SCRIPT-SHELL` | refused: keys are parsed by grammar and normalized, not matched as text | | |
| A `yarn-path` line written to `~/.config/yarn/config`, `~/.yarn/config` or `/etc/yarnrc` | refused: the class is yarn's `getRcPaths` list | the home files are denied; `/etc` is root-owned | |
| `script-shell=sh` with a planted `node_modules/.bin/sh` | refused: no value is exempt | | a checkout that already carries both: D1 |
| `Write` of `.npmrc` with a runner on Claude | refused once the Claude edit-guard wiring lands; until then open | | the wiring is in this task's scope |
| `pnpm config set npm-path /tmp/npm` (writes the global rc by default), `--location project` (writes `pnpm-workspace.yaml`) | refused | | |
| `yarn policies set-version 1.22.0`; `yarn set version stable` | refused | | the operator pins yarn in their terminal |
| A pull request adds `script-shell=./scripts/sh.sh` to the root `.npmrc`; the agent runs `npm test`, `npm update`, `npm test --workspace=child`, or `npm test` from the child | refused under D1: every ancestor file is read and `update` is not read-only; open without it | | CI and the operator's terminal, outside every guard; the `codeflow ci` row reports the key |
| A pull request adds `yarn-path` to `.yarnrc`; the agent runs `yarn --version`, or `pnpm --dir child test` under a child's planted file | refused under D1 (every yarn invocation; pnpm's directory option is read); open without it | | as above |
| A pull request adds `.pnpmfile.cjs`, a `preinstall` script, a `Makefile` with `SHELL := ./x` | not refused: repository code by design | | `ignore-scripts` is the control npm offers |
| `NODE_OPTIONS=--require /tmp/x.js npm test` | not refused: a one-call variable with ordinary uses | | the one-call route; the persistent form (`node-options` in a file) is refused |
| `~/.npmrc` registry edit from Bash where the home is inside a writable root | the guard allows the edit; the sandbox refuses it | | stated contract |

## What fails closed

- A write to a class file whose resulting text cannot be computed, or
  whose current or resulting text cannot be parsed, refuses.
- A class file that is a symbolic link, not regular, unreadable or not
  UTF-8 refuses a write; a directory at a file's path is skipped, as
  yarn skips it.
- A native edit payload for a class file whose content the hook cannot
  extract, or whose edit does not apply, refuses.
- A runner value the parser cannot read (`${VAR}`, an unterminated quote)
  refuses.
- An option that is an ambiguous prefix of a runner or pointer key, or an
  ambiguous command abbreviation, refuses.
- A pointer flag or variable refuses whatever its value.
- Under D1: an unknown start directory, an unresolvable directory option,
  an unreadable file in the chain, an exceeded bound, or an inherited
  pointer whose target cannot be read refuses as "cannot verify"; a
  command not proven read-only refuses under an effective runner.

## What the backstop cannot see

- A variable exported in an earlier tool call; a script written in one
  call and run in another; a path built at run time on a line that names
  no class file.
- `NODE_OPTIONS` set for one call.
- A change to a class file between the hook's read and the tool's write:
  the hook is pre-tool, so a concurrent writer is a residual.
- Without D1: a runner the checkout already carries. With it: CI and the
  operator's terminal, where no guard runs.
- Files that are code by design: `package.json` scripts, `.pnpmfile.cjs`,
  `Makefile`, `tasks.json`, `mise.toml` tasks and `_.source`.
- `corepack` and the `packageManager` field, which download and run the
  named package-manager version; a hash in the field pins it, an absent
  hash trusts the registry.
- Yarn Berry and Bun runtime behavior: source and documentation only.
- Windows executable lookup (`PATHEXT`, `cmd.exe`'s current-directory
  search), drive and UNC paths, the relocation of `%LOCALAPPDATA%` and
  native harness behavior there: unverified.
- Other tools with the same shape of setting, out of scope and recorded
  for one tracked item: `~/.cargo/config.toml` (`runner`, `[alias]`,
  `rustc-wrapper`), `~/.docker/config.json` (`credsStore`), gh aliases.

## Build notes for the builder

- Table: a `runner_settings` family (commands) and a
  `package_manager_paths` table (files) in `actions.json`; the runner and
  pointer key list and each manager's namespace prefixes in one Rust
  constant beside the table, cited by the docs; a test fails when a
  listed key has no policy-reference row.
- exec-guard: command-word resolution (aliases and abbreviations) and
  rows in `outward.rs` for the command forms with namespace stripping and
  prefix handling; the variable forms join the command-valued variable
  check; the class joins `startup::evaluate_with` as a third kind, judged
  before the integrity check, with the before/after judgment as its text
  predicate and the bounded sed evaluator beside `sed_script.rs`.
- edit-guard: the class-scoped before/after judgment in
  `hooks/edit_guard.rs`, fed by the parsed payload; the enforcement-path
  `EditRequest` stays content-free. The Claude preset gains the edit hook
  entry; `parse_payload` gains `MultiEdit` and `NotebookEdit`.
- Tests per AC-2: refusals per tool, alias, abbreviation, namespace,
  location, pointer, variable and launcher form, including
  `pnpm --config.script-shell`, `YARN_CONFIG`, `npm config delete`;
  allow tests for `npm config set registry`, `npm config get
  script-shell`, `pnpm config set shell-emulator true`, `yarn config set
  registry`, ordinary `pnpm --config.registry=...`, and `npm run build --
  --script-shell=x` (after `--`). Per AC-3: the review's probes as
  regressions (`=sh` to `=./runner`, the duplicate deletion, the quoted
  key, the YAML escape, the preserved runner line, the yarn user files)
  per file and per write form, and an allow test for a registry line in
  each file. Under D1: the offline local-dependency fixture with an
  install-script marker across every command, the root-to-child and
  child-to-root workspace fixtures, `--dir`, `--cwd` and `--prefix`, and
  the read-only exemptions. The Claude hook payload shapes for `Edit`,
  `Write`, `MultiEdit` and `NotebookEdit` from native captures. Byte
  equality of the sed evaluator and the edit application against the
  platform tools.
- Journey (AC-5): a fresh project with the installed build and a scratch
  `HOME`; `npm config set script-shell /tmp/x` refused; `script-shell=`
  appended to the project `.npmrc` refused through exec-guard and through
  each harness's edit-guard, Claude's included; a registry line allowed
  both ways; under D1, `npm test` and `npm update` refused in a checkout
  that carries the key.
- Docs: an appended amendment to ADR-0075 shared with TSK-284, the
  enforcement-planes section, the policy reference's "Rules with no key"
  and residuals, `changelog.d/TSK-285.md` (minor).
- Hotspot: `edit_guard.rs` with TSK-283 and TSK-284; land after PR 195 and
  after or with TSK-284's table change.

## Changes for review round 2

| Finding | Disposition |
|---|---|
| R2-1 trigger list omits `npm update` | Changed: the check covers every command not proven read-only, after alias and abbreviation resolution; the installed-command sweep (Arborist, reify, script execution) is cited; the read-only set is proven in the build with the offline fixture |
| R2-2 configuration root confused with cwd | Changed: shell cwd, configuration root and selected packages are separated; the manager's directory options are read; every ancestor's files are read from the start directory, a superset of npm's workspace lift and yarn's walk; unresolved selection is "cannot verify" |
| R2-3 option and pointer forms missed | Changed: each manager's option namespace is stripped before prefix matching (`pnpm --config.<key>`); `YARN_CONFIG` joins the pointers and the inherited-target discovery; a per-manager grammar sweep with controls is required |
| R2-4 yarn user configuration outside the class | Changed: the class is yarn's `getRcPaths` list at the cited lines, `~/.config/yarn/config`, `~/.yarn/config`, `/etc/yarnrc` and `/etc/yarn/config` included; absence, directory and unreadable cases are defined; the Windows pnpm and yarn paths use the environment-root encoding |
| N1 "six reads" | Fixed: explicit bounds (256 KiB per file, depth-bounded ancestors, 64 files) with "cannot verify" beyond them; timing stays unmeasured |
| N2 moving references | Fixed: Berry pinned to `yarnpkg/berry` master `e4e423a1e` (2026-09-24), `Configuration.ts` blob `9a7fe2456`; pnpm cited by its installed bundle first |
| N3 `npm config delete` | Fixed: deletion of a runner key refuses like a set, with the exposure consequence named |

## Changes for review round 1

| Finding | Disposition |
|---|---|
| F1 inserted text | Changed: the judgment compares the runner keys of the configuration before and after the write, parsed by each file's grammar with full decoding; the sed evaluator's scope is explicit; unreadable results refuse; a preserved runner line passes |
| F2 bare shell name | Changed: the exception is dropped with the `node_modules/.bin` evidence; no value is exempt; the cost is stated |
| F3 command matrix | Changed: aliases, option abbreviations by prefix, `--` handling, `--use-yarnrc`, `yarn_*` and `YARN_*` variables, `YARN_RC_FILENAME`, `global-pnpmfile` and `pnpm config set` locations are in the matrix (closed further in round 2) |
| F4 file discovery | Changed: the class holds the files the tools load (completed in round 2 with yarn's list) |
| F5 invocation check | Changed: triggers, the effective-configuration model, the no-process reader, the relief and the reporting row are separated (completed in round 2) |
| F6 Claude native edits | Changed: edit-guard is wired for Claude's edit tools in the preset; the planning-amendment alternative is stated, not assumed |
| N1 pnpm version | Fixed: 10.30.1 inspected and probed here, 10.30.0 in the round-1 review |
| N2 links and SHAs | Fixed in round 2 |

## Review

| Reviewer | Scope | Verdict |
|---|---|---|
| Codex, GPT-6 Astra at high | revision 1 at 7ccade0b0 | changes_requested (F1 to F6, N1, N2) |
| Codex, GPT-6 Astra at high | revision 2 at a5b164088 | changes_requested (R2-1 to R2-4, N1 to N3) |
| Codex, GPT-6 Astra at high | revision 3, this commit | pending |

## Sources

- npm 10.9.2, installed under `~/.asdf/installs/nodejs/23.10.0/lib/node_modules/npm/`:
  `docs/content/using-npm/config.md`, `configuring-npm/npmrc.md`,
  `using-npm/scripts.md`; `lib/utils/cmd-list.js:76-175` (aliases and
  `abbrev`); `lib/commands/*.js` (the Arborist, reify and script sweep);
  `node_modules/@npmcli/config/lib/index.js:639-693` (`loadLocalPrefix`,
  per the round-2 review); probes in a scratch `HOME` on 2026-10-14.
- pnpm 10.30.1: the bundle under `~/Library/pnpm/.tools/pnpm/10.30.1/`
  (`dist/pnpm.cjs`, setting names and `global-pnpmfile`), probes in a
  scratch `HOME` and `XDG_CONFIG_HOME`, and the round-2 review's
  `--config.`, `--dir` and `--version` probes of the same artifact; the
  pnpm settings reference <https://pnpm.io/settings> as a secondary,
  unversioned description.
- yarn 1.22.19: `~/.asdf/installs/yarn/1.22.19/lib/cli.js:97331-97385`
  (`getRcPaths`), probes in a scratch `HOME`, and the round-2 review's
  `YARN_CONFIG`, `--cwd` and `~/.config/yarn/config` probes.
- Yarn Berry: `packages/yarnpkg-core/sources/Configuration.ts` in
  `yarnpkg/berry` at master `e4e423a1eb117b5129f20ac626a03eb7a97aedff`
  (2026-09-24), blob `9a7fe2456637a81e4640995781928b86c5080653`
  (`getRcFilename`, `yarnPath`, `plugins`, `injectEnvironmentFiles`);
  source inspection only.
- Bun: <https://bun.sh/docs/runtime/bunfig#run-shell-use-the-system-shell-or-buns-shell>
  (`run.shell` is `system` or `bun`; `bun run` loads the project file).
- The code and records cited by path and line above, at `1e6e29e93`;
  PR 195 at `6fd46ab21`; PR 187 at `9c2dadea6`; plan PR 162 at
  `d9de22f2b`; issue 89; ADR-0075's 2026-10-05 amendment.
