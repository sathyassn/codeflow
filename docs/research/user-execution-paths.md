# Files that run code in a later shell or login: design note

Task: TSK-284. Issues: 87 (PATH directories), 90 (session environment
files), 91 (login items). Status: revision 2, after Codex's design review
of 2f73fd78d (`changes_requested`, five major findings and three nits);
no guard code exists yet. Written against the EPC-022 line at
`1e6e29e93`, PR 195 (TSK-283) at `6fd46ab21`, PR 187 (TSK-302) at
`9c2dadea6` and plan PR 162 (TSK-299) at `d9de22f2b`.

## Summary

Three kinds of file make code run later in the operator's own context,
outside any command the guards judge: a program placed in a directory on
`PATH`, a file that sets the session environment, and a login item. This
note settles which paths count on macOS, Linux and Windows, how each is
protected (the guards and the harness sandbox entries, stated separately),
what the backstop cannot see, and what fails closed.

The recommendation is unchanged in shape: a third class of the existing
startup machinery (`security/startup.rs`, the action table) under one new
always-blocking rule `security.persistence`, with the `PATH` set read from
the hook's own environment and a static table for the sandbox entries. No
new parser and no new ADR: an appended amendment to ADR-0075 and the
enforcement-planes section carry it.

Changed since revision 1: every `PATH` entry counts, with no permission
test (F2); the systemd, autostart and Windows Startup roots are resolved
from the environment the service managers use (F1); the unknown-directory
case is a bounded rule over the guards' own trust base and otherwise a
named residual (F3); Grok gets no read-blocking entry (F4); each operator
decision now states its guard effect and its sandbox effect separately,
and what changes if the operator chooses otherwise (F5).

Operator decisions D1 to D3 are pending. The design follows the
recommendation for each and says what changes under the other choice.

## The class and the threat

The guards are the honest-agent floor (ADR-0009); the harness sandboxes
are the containment; remote protection and CI are the boundary. What makes
issue 86's class severe is persistence into the operator's context: a
function in `~/.zshrc` runs in the operator's own terminal, where their
credentials and an unsandboxed shell are. The three classes here have the
same shape. A `~/.local/bin/git` runs when the operator types `git`; a
`~/.config/environment.d/x.conf` sets `PATH` or `BASH_ENV` in the
environment the systemd user manager exports to the services and session
processes it starts, which on a systemd desktop includes the terminals of
the next login (not an ssh login or any shell started outside that
manager); a `~/Library/LaunchAgents/x.plist` with `RunAtLoad` runs at the
next login with no `launchctl` call. None of them is a command the guards
see at the time it takes effect.

An in-session `PATH=./bin:$PATH git status` is a different thing: it runs
planted workspace code once, which an agent can do in a hundred ways the
guards never close (ADR-0009, interpreter indirection). This note protects
the persistent routes and leaves the one-call route where it is.

```text
                        agent session                  later, outside the session
   ┌──────────────────────────────────────┐     ┌──────────────────────────────────┐
   │ Bash / Edit tool call                │     │ operator's terminal, next login, │
   │   exec-guard, edit-guard judge it    │     │ a timer, a GUI session           │
   │   sandbox holds what they miss       │     │   no guard runs here             │
   └──────────────┬───────────────────────┘     └───────────────┬──────────────────┘
                  │ a write that lands in                       │ runs the planted
                  │   a PATH directory                          │ program, environment
                  │   a session environment file   ───────────► │ or login item
                  │   a login item
```

## What exists and what is in flight

| Piece | Where | How the new class uses it |
|---|---|---|
| The path tables and their generated denies | `security/actions.json` (`startup_paths`, `git_config_paths`); `security/actions.rs:177-230` | A third table `persistence_paths` in the same shape, with one addition: an entry may start with an environment root (`$XDG_CONFIG_HOME/`, `$XDG_DATA_HOME/`, `$XDG_RUNTIME_DIR/`, `$XDG_CONFIG_DIRS/`, `$XDG_DATA_DIRS/`, `$APPDATA/`, `$LOCALAPPDATA/`, `$ProgramData/`), resolved by the guard from its environment with the platform default when the variable is unset; the generated sandbox entries use the defaults only |
| The resolved class and the write judge | `security/startup.rs`: `StartupEnv` (59), `Class::from_paths` (281), `Class::target` (360), `Class::placement` (420), `judge_class`, the `Line` struct (725) | `Class::persistence(env)` adds the table entries and one `Entry::At { dir: true }` per `PATH` entry; `Kind::Persistence` gives the rule and the noun; every existing judgment applies unchanged |
| The order of classes in `evaluate_with` | `startup.rs:1304-1335`: the startup class first; the git class only when `git.hook_integrity` is active | The persistence class is judged with the startup class, before the integrity check, so it holds at `off`, `warn` and `block`; one test per level |
| The native edit check | `hooks/edit_guard.rs:259-300` (`evaluate` calls `class_target`) | One more call for the persistence class |
| The writer table, one judge for both guards | `security/unresolved.rs:36` (`WRITERS`, with `curl` and `wget` already there, not `sort`); `judge_writer` (1380); `ExecWriter::judge` delegates to it (`startup.rs:2773`) | TSK-299 is planned work (PR 162), not a landed writer; when it lands, the shared judge carries its options to this class too. Until then, `sort -o` into a class directory is a stated residual, not claimed coverage; the composed `sort`, `curl`, `wget` and nested-script tests are required once the work is present |
| The line's directory set | PR 187: `outward::DirSet { dirs, unknown }` (245), `line_directories` (691), `InterpreterCall` (121); today `git_guard::run_dirs_from_home` feeds `Line::dirs` | The class takes `DirSet` when PR 187 is on the line; otherwise a local shim of the same shape over `run_dirs_from_home`, replaced when PR 187 lands. Unknown state is preserved either way; the rule below is what the class does with it |
| The git configuration reader | PR 195: `security/git_config.rs` `read` (95), `get` (36), `get_all(key)` returning `(scope, origin, value)` per occurrence (47), "cannot verify" | Not used: no decision here depends on git configuration. Its reader runs git through `crate::git::command`, which resolves `git` on `PATH`, so a planted `git` in a `PATH` directory would also feed every guard that consults it: one more reason the whole `PATH` set counts. The include-file diagnostic is the model for the doctor rows |
| The persistence command family | `actions.json` family `persistence`; exec-guard's `crontab` row at `security/outward.rs:456`; family findings today carry `security.outward_actions` (`outward.rs:74`) | The family gains the commands below; its findings move to `security.persistence` |
| The startup doctor check | `doctor/startup_files.rs` | Gains the rows below |

The hooks run `codeflow` by name (`assets/base/settings/default.json:535`,
`codex/hooks.json`, `grok/hooks.json`): the guard binary itself is found
on `PATH`.

## Which paths count

### Program search path

**Rule.** Every entry of the `PATH` the hook process sees counts, as a
directory and everything below it, read lexically and through symbolic
links. There is no permission test: an existing root-owned directory
costs nothing to protect (a write there fails without the refused `sudo`),
and a directory the user cannot write can still hold a file the user can
overwrite, which a directory test would miss (F2). A missing entry counts,
since `mkdir` and a later write are two calls.

**Relative and empty entries.** `..`, `../tools/bin` and the empty entry
(the current directory) are resolved against the hook's working directory
and against every directory of the line's `DirSet`; each resolution is a
class directory. When the `DirSet` is unknown and `PATH` holds such an
entry, every write with a relative target refuses, since any directory
could be the one the entry names. Doctor reports relative and empty
entries; the cost falls only on an operator whose `PATH` has one.

**The workspace exclusion.** A dynamic entry inside the trusted root is
not protected. The trusted root is exactly the project root the hook
already receives for every other guard (the main checkout of the
repository the session started in, through its git common directory) and
`$TMPDIR`. It is never a linked worktree listed in repository metadata, a
directory named on the line, or the line's working directory, since an
agent can write all three. Static entries always count, inside the trusted
root included: a project checked out under `/opt/homebrew/` or
`~/.cargo/bin/` is an unsupported layout that doctor names. `direnv layout
node` and an activated virtualenv put `node_modules/.bin` or `.venv/bin`
on the operator's `PATH`; `npm install` and `pip install` write there on
every run, so this exclusion keeps ordinary work allowed and doctor names
the entry.

**The static table**, for the sandboxes and for a `PATH` the harness lacks
(a GUI-launched harness has a shorter `PATH` than the operator's
terminal). The guards take both sets.

| Platform | Home entries (`~/`) | Absolute entries | Why whole trees |
|---|---|---|---|
| all | `.local/bin/`, `bin/`, `.cargo/bin/`, `.rustup/toolchains/`, `go/bin/`, `.npm-global/`, `.yarn/bin/`, `.bun/bin/`, `.deno/bin/`, `.volta/`, `.asdf/shims/`, `.asdf/installs/`, `.pyenv/shims/`, `.pyenv/versions/`, `.rbenv/shims/`, `.rbenv/versions/`, `.nvm/versions/`, `.local/share/mise/`, `.local/share/pnpm/`, `.fnm/` | | A shim or link in a `bin` runs a file under `installs/` or `versions/`; the text guard does not follow link targets, so the tool manager's tree counts whole |
| macOS | `Library/pnpm/`, `Library/Python/` | `/opt/homebrew/`; under `/usr/local/`: `bin/`, `sbin/`, `lib/`, `libexec/`, `opt/`, `Cellar/`, `Homebrew/` | `/opt/homebrew/` is Homebrew's own prefix, user-owned on Apple silicon; `bin/git` links into `Cellar/` and `lib/` is loaded by every Homebrew program. `/usr/local/` is not taken whole: a project under `/usr/local/src` is ordinary |
| Linux | as "all" | the same `/usr/local/` subdirectories | root-owned on most systems, so the entries cost nothing there |
| Windows | `AppData/Roaming/npm/`, `AppData/Local/Microsoft/WindowsApps/`, `AppData/Local/Programs/`, `AppData/Local/pnpm/`, `AppData/Local/Volta/`, `AppData/Local/Microsoft/WinGet/Links/`, `scoop/shims/`, `scoop/apps/`, `.cargo/bin/`, `go/bin/` | | `USERPROFILE` is the home the class reads; `$APPDATA/` and `$LOCALAPPDATA/` roots cover a profile whose `AppData` is elsewhere |

### Session environment

| Platform | Files (class entries, roots from the environment with the platform default) | Commands (persistence family) |
|---|---|---|
| Linux | `$XDG_CONFIG_HOME/environment.d/` (default `~/.config`), `$XDG_CONFIG_DIRS/environment.d/` (each entry, default `/etc/xdg`), `/etc/environment.d/`, `$XDG_RUNTIME_DIR/environment.d/`, `~/.pam_environment`, `/etc/security/pam_env.conf`, `~/.xprofile`, `~/.xsessionrc`, `~/.xsession`, `~/.xinitrc`, `~/.gnomerc`, `$XDG_CONFIG_HOME/plasma-workspace/env/` (`/etc/environment` is already in `startup_paths`) | `systemctl --user set-environment`, `systemctl --user import-environment`, `dbus-update-activation-environment` |
| macOS | none new (`/etc/paths` and `/etc/paths.d/` are in the class; `~/.MacOSX/environment.plist` has not been read since OS X 10.8) | `launchctl setenv`, `launchctl unsetenv`: the environment every later-launched process and terminal window gets for the rest of the login session |
| Windows | none (the user environment is the registry) | `setx` (any form, `/M` included), PowerShell `[Environment]::SetEnvironmentVariable(..., 'User')` and `'Machine'`, `Set-ItemProperty` and `New-ItemProperty` on `HKCU:\Environment`; `reg add HKCU*` is already denied |

### Login items and scheduled runs

| Platform | Files (class entries) | Commands (persistence family) |
|---|---|---|
| macOS | `~/Library/LaunchAgents/`, `/Library/LaunchAgents/`, `/Library/LaunchDaemons/` | `launchctl submit`, `launchctl load`, `bootstrap` and `enable` (already), `osascript` whose text contains `login item`, `at`, `batch` |
| Linux, systemd user units (every directory of the user unit search path, `systemd.unit(5)`, drop-in `<unit>.d/` included) | `$XDG_CONFIG_HOME/systemd/user.control/`, `$XDG_RUNTIME_DIR/systemd/user.control/`, `$XDG_RUNTIME_DIR/systemd/transient/`, `$XDG_RUNTIME_DIR/systemd/generator.early/`, `$XDG_CONFIG_HOME/systemd/user/`, `$XDG_CONFIG_DIRS/systemd/user/`, `/etc/systemd/user/`, `$XDG_RUNTIME_DIR/systemd/user/`, `/run/systemd/user/`, `$XDG_RUNTIME_DIR/systemd/generator/`, `$XDG_DATA_HOME/systemd/user/` (default `~/.local/share`), `$XDG_DATA_DIRS/systemd/user/` (default `/usr/local/share:/usr/share`), `/usr/local/lib/systemd/user/`, `/usr/lib/systemd/user/`, `$XDG_RUNTIME_DIR/systemd/generator.late/`; and `/etc/systemd/system/` | `systemd-run` (a transient unit runs outside the session), `systemctl --user link` and `edit`, `loginctl enable-linger`, `at`, `batch`; `crontab` with anything but `-l` or help is already refused by exec-guard (`outward.rs:456`) and the text rule gains `crontab -` and `crontab <file>` |
| Linux, autostart and cron | `$XDG_CONFIG_HOME/autostart/`, `$XDG_CONFIG_DIRS/autostart/` (autostart specification), `~/.config/autostart-scripts/`, `/etc/crontab`, `/etc/cron.d/`, `/etc/cron.hourly/`, `/etc/cron.daily/`, `/etc/cron.weekly/`, `/etc/cron.monthly/` | as above |
| Windows | `$APPDATA/Microsoft/Windows/Start Menu/Programs/Startup/`, `$ProgramData/Microsoft/Windows/Start Menu/Programs/StartUp/` | `schtasks /create` and `/change`, `Register-ScheduledTask`, `Set-ScheduledTask`, `Set-ItemProperty` and `New-ItemProperty` on a `CurrentVersion\Run` or `RunOnce` key |

**Relocation and its limits.** `StartupEnv` already reads
`XDG_CONFIG_HOME` and relocates `.config/` entries (`startup.rs:328`); it
gains `XDG_DATA_HOME`, `XDG_RUNTIME_DIR`, `XDG_CONFIG_DIRS`,
`XDG_DATA_DIRS`, `APPDATA`, `LOCALAPPDATA` and `ProgramData`, each with its
platform default when unset. The hook's environment is the harness's, and
the service managers may run with another: a user manager started by
`pam_systemd` sets `XDG_RUNTIME_DIR` itself, and a Windows Startup folder
redirected by policy (a known folder, `KNOWNFOLDERID`) is not in
`APPDATA`. Those are guard residuals: the guard protects the roots it can
resolve; doctor reports a redirected Startup folder by reading the `User
Shell Folders` registry value and a user manager environment that differs
from the hook's (`systemctl --user show-environment`, read-only), and the
sandbox entries name the defaults only.

The PowerShell forms arrive as one `-Command` string, which a native prefix
rule cannot match (the family's existing `codex_note`); exec-guard carries
them, as it does `Set-ExecutionPolicy`.

## The unknown-directory case

A write whose directory the guard cannot resolve and that names no class
place (`cd "$D" && cp x git`) is not refused by the shipped machinery:
`closed_rule` needs a class name on the line (`startup.rs:2206`), and
`placement_violation` needs a class-place cue (`startup.rs:3045`). PR 187
preserves the unknown state; it does not recover `D`. Revision 1 claimed
a refusal here that the cited code does not give (F3). The design now
has one bounded rule and a named residual:

- **Rule.** On an unknown `DirSet`, a write whose target has no directory
  part, or is relative, refuses when its final component is one of the
  programs the guard plane itself runs: `codeflow`, `git`, `gh`, `sh`,
  `bash`, `zsh`, `env` and `command`. A planted one of these defeats the
  hooks wherever it lands. This is TSK-302's trailing-components rule
  applied to the guards' trust base. Cost: `cd "$D" && cp x git` refuses;
  `cd "$D" && cp a b` passes (the required control).
- **Residual.** Any other basename under an unknown directory
  (`cd "$D" && cp x cargo`) is open to the text guard and held by the
  sandbox where `D` resolves outside the writable roots. ADR-0009's
  stance applies: an inventory of every executable name would be an open
  set and a false assurance.

## The rule, the refusal and the sandbox entries

- **Rule id:** `security.persistence`, no policy key, always blocking,
  like `security.shell_startup` (policy reference, "Rules with no key"),
  judged at every `git.hook_integrity` level. `security.shell_startup`
  keeps its name and scope.
- **Refusal text:** names the class and the entry (`cp` targets the
  program directory `~/.local/bin` on `PATH`; `tee` targets the login item
  directory `~/Library/LaunchAgents`) and the sanctioned path: the
  operator installs programs and login items from their own terminal; no
  policy key relaxes this rule; the harness sandbox denies these writes
  where one runs.
- **Claude:** `Edit(...)` denies and sandbox `denyWrite` entries for the
  static table at its default roots. Claude's file tools run no edit-guard
  today (`default.json:531` wires `Bash|PowerShell` only); TSK-285 wires
  edit-guard for Claude's `Edit`, `Write`, `MultiEdit` and `NotebookEdit`
  for its key-level rule, and this class is judged by the same hook once
  it exists, which closes the dynamic `PATH` and relocated-root gap on
  Claude. Until that lands, those are `Edit`-deny only.
- **Codex:** `cf-guard` and `cf-builder` read-only entries for every
  static entry, as `config.toml:93-163` holds the startup ones.
- **Grok:** no `deny` entry: a Grok deny blocks reads, and AC-3 keeps
  reads of every class allowed (an agent diagnosing a unit or a
  LaunchAgent reads it). `sandbox.toml:28` documents directory
  `read_only` entries; whether an absolute `read_only` path is honored is
  unverified, so the build probes it natively and adds read-preserving
  entries only on a positive result. Containment otherwise is the
  `workspace` write set, which leaves the home and `/opt/homebrew`
  unwritable.
- **Doctor:** `codeflow doctor --check startup-files` gains rows for a
  `PATH` entry inside the trusted root (writable by design), a `PATH` entry
  the static table does not cover (guards only), a relative or empty
  entry, a static entry inside the trusted root (unsupported layout), a
  user manager environment that differs from the hook's, and a redirected
  Windows Startup folder.

## Options compared

| Choice | Options | Recommendation and why |
|---|---|---|
| How the `PATH` set is known | (a) static table only; (b) the hook's `PATH` only; (c) both | (c). The sandboxes take static paths only; a static list misses the next tool manager; the hook's `PATH` is the one the harness's shells get |
| Which `PATH` entries count | (a) those before the system directories, as the task text says; (b) every entry | (b). Order decides which names are shadowed, never whether shadowing works; `codeflow` itself lives in a user directory. Operator decision D1 |
| The Homebrew prefix | (a) `bin/` and `sbin/` only; (b) the prefix whole | (b) for `/opt/homebrew/`; for `/usr/local/` the Homebrew subdirectories only. Operator decision D2 |
| A `PATH` entry inside the trusted root | (a) protected; (b) writable, reported by doctor | (b), with the trusted root defined narrowly above; decided here and disclosed |
| Unknown directory | (a) refuse every relative write; (b) the trust-base rule and a residual; (c) residual only | (b): (a) refuses ordinary `cd "$D" && cp a b`; (c) leaves `cd "$D" && cp x codeflow` open for no reason |
| Rule id | (a) widen `security.shell_startup`; (b) a new no-key rule | (b), `security.persistence` |
| Installer programs | (a) a command family refused in agent sessions; (b) left as opaque programs the sandboxes hold | Not in this task; operator decision D3 |
| An ADR | (a) a new ADR; (b) an appended amendment to ADR-0075 plus the enforcement-planes section | (b): no boundary moves; the mechanism is the shipped class machinery; ADR-0075's 2026-10-05 amendment names issues 87 to 91 |

## Adversarial cases

| Case | Guards | Sandbox | Open |
|---|---|---|---|
| `cp evil ~/.local/bin/git`, `tee`, `install`, `>`, `ln -s`, `mv`, `chmod +x` into a `PATH` directory, in any spelling the class expands | refused, `security.persistence` | denied on all three (home unwritable) | |
| `> /opt/team/bin/git` where the directory is `0555` and the file is user-writable | refused: every `PATH` entry counts | denied where the path is outside the writable roots | |
| `mkdir ~/bin && cp evil ~/bin/git` in two calls | both refuse: `~/bin` is a class entry whether or not it exists | denied | |
| `cd ~/.local/bin && cp ../x git` | refused: the directory set resolves the target | denied | |
| `cd "$D" && cp x git`, `cd "$D" && cp x codeflow` | refused: the trust-base rule | denied where `D` is outside the roots | |
| `cd "$D" && cp x cargo` | not refused: named residual | denied where `D` is outside the roots | an unsandboxed seat |
| `PATH=../tools/bin:$PATH` in the operator's environment, then `cp x /home/u/tools/bin/git` | refused: the relative entry resolves against the hook's cwd | denied | a relative entry that resolves differently in the operator's shell than in the hook's cwd |
| `tar -xf kit.tgz -C ~/.local/bin`, `unzip -d ~/bin`, `rsync -a tools/ ~/.cargo/bin/`, `git clone x ~/bin` | refused: placing programs run in or into the class | denied | an absolute member name under `tar -P` on a line that names no class place: the content residual the startup class states |
| `sort -o ~/.local/bin/git` | not refused until TSK-299's writer option lands; residual now | denied | |
| `python3 -c "open('/opt/homebrew/bin/git','w')"`, a heredoc that names a class path | refused (interpreter code names a class path) | denied | a path the code builds from pieces |
| `brew install x`, `cargo install --path ./evil` (a crate whose binary is named `git`), `npm i -g ./evil.tgz` with a `git` bin, `pipx install ./evil` | not refused by the guards: opaque programs | denied: the home and `/opt/homebrew` are outside every writable root on Claude (macOS), Codex (`workspace-write`) and Grok (`workspace`) | an unsandboxed seat, Claude's unsandboxed retry, a Codex `--sandbox` override, native Windows Claude; D3 |
| `printf 'PATH=/tmp/x:$PATH\n' > ~/.config/environment.d/p.conf`, the same under `$XDG_CONFIG_HOME` moved elsewhere | refused: the root is resolved from the environment | denied at the default root; a moved root has no sandbox entry | a root the hook's environment does not show |
| A drop-in `~/.config/systemd/user.control/foo.service.d/x.conf` or a unit under `$XDG_DATA_HOME/systemd/user/` | refused: every directory of the unit search path is in the class | denied at the default roots | |
| A file in a Startup folder redirected by policy | not refused; doctor reports the redirection | not held | residual |
| `launchctl setenv PATH /tmp/x:/usr/bin`, `setx PATH ...`, `systemctl --user set-environment PATH=...` | refused: persistence family | the command needs no file write, so no sandbox holds it | the PowerShell `-Command` string on a harness that passes it opaquely |
| `cat > ~/Library/LaunchAgents/x.plist <<EOF ... RunAtLoad ... EOF` | refused | denied | |
| `launchctl submit -l x -- /tmp/evil`, `systemd-run --user /tmp/evil`, `at now + 1 min <<< /tmp/evil`, `crontab -`, `schtasks /create` | refused: persistence family | not held by any sandbox | a launcher the family does not list: the family is a floor over its named spellings |
| `osascript -e 'tell application "System Events" to make login item at end with properties {path:"/tmp/evil"}'` | refused: `osascript` text names a login item | not held | an AppleScript that builds the words at run time |
| A malicious pull request adds `bin/git` and the operator's `PATH` holds `$PWD/bin` through direnv | not refused: the trusted root is writable by design; doctor names the entry | not held | the operator's `PATH` choice |
| A pull request adds `.envrc` setting `PATH` | the startup class refuses the agent's write; direnv runs only after `direnv allow`, which it also refuses | | the operator allowing it by hand |
| `PATH=$PWD/bin:$PATH git push --force origin main` on one line | git-guard judges the git command as spelled; not persistence | | the one-call route (ADR-0009) |
| A planted link in a protected `bin` whose target is writable (`/opt/homebrew/bin/git` into `Cellar/`, `~/.asdf/shims/node` into `installs/`) | the whole tree is in the class, so the target refuses | denied | a link into a directory neither the table nor `PATH` holds |
| Native `Write` of `~/.local/bin/git` | Codex and Grok: edit-guard refuses. Claude: the `Edit` deny holds for a static entry; a dynamic entry is refused once TSK-285's Claude edit-guard wiring lands | | until then, Claude `Write` to a dynamic entry |

## What fails closed

- A `PATH` entry the hook cannot stat counts as protected; every entry
  counts, so no permission result can exclude one.
- A relative or empty `PATH` entry on an unknown `DirSet` makes every
  relative write refuse.
- A write whose directory the guard cannot resolve, on a line that names
  the home, a class directory or a `PATH` directory, refuses (the closed
  rule and `placement_violation`); one whose target is a trust-base
  program name refuses on an unknown `DirSet`.
- A placing program with an option the writer table does not list, on a
  line that names a class place, refuses.
- A native edit payload that is truncated, malformed or ambiguous refuses
  (`edit_guard::parse_payload`).
- A class entry whose symbolic link cannot be followed is judged by its
  lexical reading and the link text it holds.
- The `osascript` and PowerShell rules match the words in the text; a
  string the guard cannot read beside those words refuses.
- An environment root whose variable holds a relative or empty value is
  read as its platform default and doctor warns; `PATH` absent from the
  hook's environment leaves the static table in force and doctor warns.

## What the backstop cannot see

- A program that writes on its own: the installers, `corepack enable`,
  `rustup`, `asdf install`, `mise install`. The sandboxes hold them where
  the target is outside the writable roots; an unsandboxed seat, Claude's
  unsandboxed retry, a Codex `--sandbox` override, Grok on Windows (stub
  sandbox) and native Windows Claude (no Bash sandbox) stay open.
- A `PATH` directory inside the trusted root, and a relative entry that
  resolves elsewhere in the operator's shell: reported by doctor, never
  refused.
- A `PATH` directory on the operator's terminal `PATH` that the harness's
  `PATH` lacks and the static table does not name.
- A basename other than the trust base written under an unknown
  directory.
- Link targets outside the class.
- Claude's file tools for a dynamic entry or a relocated root, until the
  Claude edit-guard wiring lands.
- A service-manager root the hook's environment does not show
  (`XDG_RUNTIME_DIR` of a manager started by `pam_systemd`, a Startup
  folder redirected by policy): doctor reports, the guard cannot judge.
- A path built at run time on a line that names no class place; a script
  written in one call and run in another; content a program supplies
  (archive members, diffs, server-chosen names).
- The PowerShell `-Command` string on a harness that hands it over
  opaquely; `cmd.exe` searching the current directory before `PATH`.
- Module search paths (`PYTHONPATH`, `NODE_PATH`, `PERL5LIB`, `RUBYLIB`,
  `GIT_EXEC_PATH`, `LD_LIBRARY_PATH`) set for one call; their directories,
  when set in the hook's environment, are a possible later extension of
  the dynamic read.

## Operator decisions (pending)

Each row states the guard effect and the sandbox effect separately, the
cost, and what changes if the operator chooses the other way. The design
follows the recommendation until the operator decides.

| # | Decision and recommendation | Guard effect | Sandbox effect | Cost | If the operator chooses otherwise |
|---|---|---|---|---|---|
| D1 | Every `PATH` entry counts (recommended), or only entries searched before the system directories (the task's wording) | Direct writes (redirect, `cp`, `tee`, `install`, `ln`, `mv`, `chmod`, placing programs, interpreter literals) into any `PATH` entry refuse; installers are not commands the guard refuses | The static entries get `denyWrite` and Codex read-only entries; the sandboxes already deny these writes where the entry is outside the writable roots, so on a sandboxed seat the change is a named refusal in place of a permission error. On an unsandboxed seat, or where a writable root contains a static entry, these are new refusals | Writes into `~/.cargo/bin`, `~/.local/bin`, `~/go/bin` and the tool-manager trees are refused in agent sessions | The dynamic set keeps only entries before the first of `/usr/bin`, `/bin`, `/usr/sbin`, `/sbin`; `codeflow`, `cargo`, `node` and `python3` in a later user directory stay unprotected; the static table and the sandbox entries are unchanged |
| D2 | `/opt/homebrew/` counts whole and `/usr/local/` by its Homebrew subdirectories (recommended), or `bin/` and `sbin/` only | Direct writes anywhere under the prefix refuse; `brew install`, `brew upgrade` and `brew link` are opaque installers the guard does not refuse | `/opt/homebrew/` and the `/usr/local/` subdirectories get `denyWrite` and Codex read-only entries; on Claude (macOS), Codex and Grok the prefix is already outside the writable roots, so `brew install` already fails there; an unsandboxed seat stays open until D3 | A project checked out under the prefix is unsupported (doctor names it); a direct write to a Homebrew library from a session refuses | Only `bin/` and `sbin/` are entries; a planted `lib/` library or Cellar binary reached through a link stays open to the text guard and is held by the sandbox alone |
| D3 | Follow-up (one tracked item if approved): an installer command family (`brew install`, `cargo install`, `npm i -g`, `pipx`, `uv tool install`, `go install`, `gem install`, `rustup`, `asdf`, `mise`) refused in agent sessions, or left as opaque programs | The listed spellings refuse with a named rule; the set is open (ADR-0009), so it closes only what it names, never assembled paths, separate scripts or link targets | None; the family adds native text denies, which hold on every seat including unsandboxed ones | Agents cannot set up a missing tool; the operator installs it | Installers stay open on unsandboxed seats and under Claude's unsandboxed retry; the sandboxes keep denying them elsewhere |

## Siblings found during this design, not in scope

Each is issue 86's class through another reader of a user file. One tracked
item is recommended for the set; none blocks this task.

- `~/.ssh/config`: `ProxyCommand`, `LocalCommand` with `PermitLocalCommand`
  and `Match exec` run on every ssh, which includes `git push` over ssh.
- `~/.cargo/config.toml`: `[alias]`, `runner`, `rustc-wrapper`,
  `[net] git-fetch-with-cli`; a key-level case like TSK-285's.
- `~/.config/gh/config.yml`: an alias with `!` runs a shell on every use.
- `~/.docker/config.json`: `credsStore` names a program found on `PATH`.
- `~/.vimrc`, `~/.config/nvim/`: run when `git commit` opens the editor.
- `~/.local/lib/python3.*/site-packages/usercustomize.py` and
  `sitecustomize.py`.
- Editor and terminal configurations that set a shell or `PATH` for every
  terminal: VS Code `terminal.integrated.*`, iTerm2 and Windows Terminal
  profiles, and the repository forms (`.vscode/settings.json`).

## Build notes for the builder

- Table: `persistence_paths` in `actions.json`, `home`, `absolute` and
  root-prefixed entries; `anywhere` empty. The generator emits sandbox
  entries for the default roots only. The `manifest_consistency` and
  action-table tests cover generation; presets, installed copies and
  baseline resync in the same change.
- `StartupEnv` gains `path_dirs` and the XDG, `APPDATA`, `LOCALAPPDATA`
  and `ProgramData` roots; `Class::persistence(env)`; `Kind::Persistence`;
  the class is judged in `evaluate_with` before the integrity check.
- Tests per AC-2: per class and per write form (native edit, redirect,
  `cp`, `tee`, `install`), plus `mkdir`, `chmod`, a placing program, an
  interpreter literal, a relocated root, a systemd drop-in, a relative
  `PATH` entry, the trust-base rule and its control, each
  `git.hook_integrity` level. Per AC-3: reads of each class, writes beside
  each class, an entry inside the trusted root, the ordinary corpus. The
  composed `sort`, `curl`, `wget` and nested-script tests once TSK-299 is
  present.
- Journey (AC-5): a scratch `HOME` and `XDG_CONFIG_HOME` with `PATH`
  naming `$HOME/.local/bin`; writes of `$HOME/.local/bin/git`,
  `$XDG_CONFIG_HOME/environment.d/p.conf` and
  `$HOME/Library/LaunchAgents/x.plist` through the installed build, each
  refused; allowed controls beside them. Table tests on Linux and Windows
  show the class, not which directories the service managers consume;
  that is stated in the record.
- Docs: the ADR-0075 amendment, the enforcement-planes section, the policy
  reference's "Rules with no key" and residuals, `docs/troubleshooting.md`
  for the doctor rows, `changelog.d/TSK-284.md` (minor).
- Landing: after PR 195; the `DirSet` shim is replaced when PR 187 lands;
  the Claude edit-guard wiring is TSK-285's and is not required here.

## Changes for review round 1

| Finding | Disposition |
|---|---|
| F1 user persistence locations | Changed: every systemd user unit search directory, the autostart roots, `environment.d` under every root, and the Windows Startup folders are entries resolved from environment roots with platform defaults; a redirected folder and a manager environment the hook cannot see are named residuals with doctor rows |
| F2 PATH exclusion predicate | Changed: no permission test; every entry counts; relative and empty entries are resolved against the hook's cwd and the `DirSet`, and refuse relative writes on an unknown set |
| F3 unknown-directory refusal | Changed: the claim is withdrawn; one bounded rule over the guards' trust base, the rest a named residual; the TSK-302 landing condition is stated |
| F4 Grok read denies | Changed: no Grok deny; a native probe of absolute `read_only` entries in the build; reads stay allowed |
| F5 decision table | Changed: guard and sandbox effects separated, installer claims corrected, D1 cost restated for unsandboxed seats, the trusted root defined narrowly, `/usr/local/` narrowed, "if otherwise" column added |
| N1 environment.d scope | Fixed in the threat section |
| N2 evidence citation | Fixed: the `npm config ls -l` citation is replaced by npm's documented default and a scratch-HOME probe |
| N3 ADR | Dropped as the reviewer proposed; the appended amendment stands |

## Review

| Reviewer | Scope | Verdict |
|---|---|---|
| Codex, GPT-6 Astra at high | revision 1 at 2f73fd78d | changes_requested (F1 to F5, N1 to N3) |
| Codex, GPT-6 Astra at high | revision 2, this commit | pending |

## Sources

- The code and records cited by path and line above, at `1e6e29e93`;
  PR 195 at `6fd46ab21`; PR 187 at `9c2dadea6`; plan PR 162 at
  `d9de22f2b`.
- Issues 86, 87, 90 and 91 and ADR-0075's 2026-10-05 amendment.
- systemd: `systemd.unit(5)` (the user unit search path) and
  `environment.d(5)` (the scope of the user manager's environment), from
  the systemd manual sources; the freedesktop autostart specification
  (`$XDG_CONFIG_DIRS/autostart`); Microsoft's known-folder documentation
  (`KNOWNFOLDERID`, folder redirection); Apple's archived launchd guide
  (home `LaunchAgents` loading). These are documentary sources; no Linux
  or Windows runtime was exercised for this note, and the builder's
  journey runs on macOS with hosted CI running the table tests elsewhere.
- npm's documented global file is `$PREFIX/etc/npmrc` (`npm help npmrc`);
  on a tool-manager node install that prefix sits under the manager's tree
  (checked in a scratch `HOME` with `NPM_CONFIG_GLOBALCONFIG` set, no user
  configuration read).
