# Files that run code in a later shell or login: design note

Task: TSK-284. Issues: 87 (PATH directories), 90 (session environment
files), 91 (login items). Status: draft for the other lineage's review
(Codex, GPT-6 Astra at high); no guard code exists yet. Written against
the EPC-022 line at `1e6e29e93`.

## Summary

Three kinds of file make code run later in the operator's own context,
outside any command the guards judge: a program placed in a directory on
`PATH`, a file that sets the session environment, and a login item. This
note settles which paths count on macOS, Linux and Windows, how each is
protected (the guards, the harness sandbox entries), what the backstop
cannot see, and what fails closed.

The recommendation is to protect them as a third class of the existing
startup machinery (`security/startup.rs`, the action table), under one new
always-blocking rule `security.persistence`, with the `PATH` set read from
the hook's own environment and a static table for the sandbox entries. No
new parser and no new ADR: an appended amendment to ADR-0075 and the
existing enforcement-planes section carry the change.

Two choices change what adopters may write and go to the operator, with a
recommendation each: every user-writable `PATH` directory counts, not only
those before the system directories; and the Homebrew prefix counts whole.
A third, whether agent sessions may install programs at all, is a
follow-up the operator decides.

## The class and the threat

The guards are the honest-agent floor (ADR-0009); the harness sandboxes
are the containment; remote protection and CI are the boundary. What makes
issue 86's class severe is persistence into the operator's context: a
function in `~/.zshrc` runs in the operator's own terminal, where their
credentials and an unsandboxed shell are. The three classes here have the
same shape. A `~/.local/bin/git` runs when the operator types `git`; a
`~/.config/environment.d/x.conf` sets `PATH` or `BASH_ENV` for every shell
of the next login; a `~/Library/LaunchAgents/x.plist` with `RunAtLoad`
runs at the next login with no `launchctl` call. None of them is a
command the guards see at the time it takes effect.

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

The startup class is already built as one list and one judge, and the new
classes slot into it.

| Piece | Where | How the new classes use it |
|---|---|---|
| The path tables and their generated denies | `security/actions.json` (`startup_paths`, `git_config_paths`); `security/actions.rs:177-230` (`StartupPaths`, `claude_edit_denies`, `sandbox_write_denies`, `apply_to_claude_preset`, `codex_rules`) | A third table in the same shape generates the Claude `Edit` denies and `denyWrite` entries and the Codex read-only entries for the static paths |
| The resolved class and the write judge | `security/startup.rs`: `StartupEnv` (59), `Class::new` and `Class::git_config` (259, 275), `Class::target` (360), `Class::placement` (420), `evaluate_with` (1304), `class_target` (3179); the `Line` struct (725) carries `kind` and the line's directories | `Class::persistence(env)` is built by `from_paths` from the third table plus the dynamic `PATH` entries; a third `Kind` gives the rule id and the noun; every existing judgment (redirects, writers, copiers, placing programs, interpreter code, staged runs, the closed rule, symbolic links, globs, quoting) applies unchanged |
| The native edit check | `hooks/edit_guard.rs:259-300` (`evaluate` calls `class_target`) | One more call for the persistence class, same shape |
| The shell check | `hooks/exec_guard.rs:121-128` (`startup::evaluate_with`) | `evaluate_with` judges the third class in the same pass |
| The writer table, one judge for both guards | `security/unresolved.rs:36` (`WRITERS`), `writer_read` (1077), `judge_writer` (1380) | TSK-299 (plan PR 162) adds `sort -o`, `curl -o`, `wget -O` and the nested sed script to this table; the new classes get them for free, since the judge runs once per writer against every class |
| The line's directory set | TSK-302 (PR 187): `outward::line_directories` and `dir_changes`; today `git_guard::run_dirs_from_home` feeds `Line::dirs` | A relative write after `cd ~/.local/bin` is judged against the directory set; once TSK-302 lands, the persistence class takes its set from `line_directories` and the older reader is retired, not duplicated |
| The git configuration reader | TSK-283 (PR 195): `security/git_config.rs` (`read`, `get`, `boolean`, "cannot verify"; a `get_all` for multi-valued keys is being added) | Not used here: no decision in this task depends on git configuration. Its include-file diagnostic (`codeflow doctor --check git-config-includes`) is the model for the doctor rows below |
| The persistence command family | `security/actions.json` family `persistence`; exec-guard's `crontab` rule at `security/outward.rs:456` | The family gains the command forms in the next section; exec-guard's parse stays in `outward.rs` beside the existing rows |
| The startup doctor check | `doctor/startup_files.rs` | Gains the `PATH` rows below |

The hooks run `codeflow` by name (`assets/base/settings/default.json:535`,
`codex/hooks.json`, `grok/hooks.json`): the guard binary itself is found
on `PATH`. That is the strongest reason the whole `PATH` set counts.

## Which paths count

### Program search path

Rule: a directory on the `PATH` the hook process sees counts when it is
not an existing directory the user cannot write. A missing directory
counts, since a `mkdir` and a later write are two calls. An entry that
cannot be read counts. An entry inside the workspace (a repository root,
a registered worktree, the temporary directory) does not count; doctor
reports it. Order on `PATH` is irrelevant: a late user directory still
shadows every program that is not in the system directories, `codeflow`,
`cargo`, `node` and `python3` among them.

The dynamic set serves the guards. The sandboxes need static paths, so the
table below is generated into the presets as well, and the guards take
both. Entries are directories and everything below them.

| Platform | Home entries (`~/`) | Absolute entries | Why whole, not `bin` only |
|---|---|---|---|
| all | `.local/bin/`, `bin/`, `.cargo/bin/`, `.rustup/toolchains/`, `go/bin/`, `.npm-global/`, `.yarn/bin/`, `.bun/bin/`, `.deno/bin/`, `.volta/`, `.asdf/shims/`, `.asdf/installs/`, `.pyenv/shims/`, `.pyenv/versions/`, `.rbenv/shims/`, `.rbenv/versions/`, `.nvm/versions/`, `.local/share/mise/`, `.local/share/pnpm/`, `.fnm/` | | A shim or link in a `bin` runs a file under `installs/` or `versions/`; writing that target is a write through a link in a protected directory, which the text guard does not follow (policy reference, "the text guard does not inspect links inside a copied or moved tree"), so the tool manager's tree counts whole |
| macOS | `Library/pnpm/`, `Library/Python/` | `/opt/homebrew/`, `/usr/local/` | On Apple silicon `/opt/homebrew/` is owned by the user; `/opt/homebrew/bin/git` is a link into `Cellar/`, and `lib/` holds libraries every Homebrew program loads. On Intel the same holds for `/usr/local/`, which is admin-group writable. On Linux `/usr/local/` is root-owned, so the entry costs nothing there |
| Linux | as "all" | `/usr/local/` | `/snap/bin` and distribution directories are root-owned and already fail without the refused `sudo` |
| Windows | `AppData/Roaming/npm/`, `AppData/Local/Microsoft/WindowsApps/`, `AppData/Local/Programs/`, `AppData/Local/pnpm/`, `AppData/Local/Volta/`, `AppData/Local/Microsoft/WinGet/Links/`, `scoop/shims/`, `scoop/apps/`, `.cargo/bin/`, `go/bin/` | | `USERPROFILE` is the home the class reads (`StartupEnv::from_vars`), so home entries cover these |

The builder finalizes the table against the rule above; the dynamic read
covers a tool manager the table does not name.

### Session environment

| Platform | Files (class entries) | Commands (persistence family) |
|---|---|---|
| Linux | `~/.config/environment.d/`, `/etc/environment.d/`, `~/.pam_environment`, `/etc/security/pam_env.conf`, `~/.xprofile`, `~/.xsessionrc`, `~/.xsession`, `~/.xinitrc`, `~/.gnomerc`, `~/.config/plasma-workspace/env/` (`/etc/environment` is already in `startup_paths`) | `systemctl --user set-environment`, `systemctl --user import-environment`, `dbus-update-activation-environment` |
| macOS | none new (`/etc/paths` and `/etc/paths.d/` are already in the class; `~/.MacOSX/environment.plist` has not been read since 10.8) | `launchctl setenv`, `launchctl unsetenv`: they change the environment every later-launched process and terminal window gets for the rest of the login session |
| Windows | none (the user environment is the registry) | `setx` (any form, `/M` included), PowerShell `[Environment]::SetEnvironmentVariable(..., 'User')` and `'Machine'`, `Set-ItemProperty` and `New-ItemProperty` on `HKCU:\Environment`; `reg add HKCU*` is already denied |

### Login items and scheduled runs

| Platform | Files (class entries) | Commands (persistence family) |
|---|---|---|
| macOS | `~/Library/LaunchAgents/`, `/Library/LaunchAgents/`, `/Library/LaunchDaemons/` | `launchctl submit`, `launchctl load`, `bootstrap` and `enable` (already), `osascript` whose text contains `login item`, `at`, `batch` |
| Linux | `~/.config/systemd/user/`, `~/.local/share/systemd/user/`, `/etc/systemd/user/`, `/etc/systemd/system/`, `~/.config/autostart/`, `~/.config/autostart-scripts/`, `/etc/xdg/autostart/`, `/etc/crontab`, `/etc/cron.d/`, `/etc/cron.hourly/`, `/etc/cron.daily/`, `/etc/cron.weekly/`, `/etc/cron.monthly/` | `systemd-run` (a transient unit runs outside the session), `systemctl --user link` and `edit`, `loginctl enable-linger`, `at`, `batch`; `crontab` with anything but `-l` or help is already refused by exec-guard (`outward.rs:456`) and the text rule gains `crontab -` and `crontab <file>` |
| Windows | `AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup/` | `schtasks /create` and `/change`, `Register-ScheduledTask`, `Set-ScheduledTask`, `Set-ItemProperty` and `New-ItemProperty` on a `CurrentVersion\Run` or `RunOnce` key |

The PowerShell forms arrive as one `-Command` string, which a native prefix
rule cannot match (the family's existing `codex_note`); exec-guard carries
them, as it does `Set-ExecutionPolicy`.

## The rule, the refusal and the sandbox entries

- **Rule id:** `security.persistence`, no policy key, always blocking,
  like `security.shell_startup` (policy reference, "Rules with no key").
  `security.shell_startup` keeps its name and scope: it is shipped,
  documented and tested, and widening its meaning would make its name
  wrong. The persistence family's exec-guard findings move from
  `security.outward_actions` to this rule, so one rule names every
  persistent route.
- **Refusal text:** names the class and the entry (`cp` targets the
  program directory `~/.local/bin` on `PATH`; `tee` targets the login
  item directory `~/Library/LaunchAgents`) and the sanctioned path: the
  operator installs programs and login items from their own terminal; no
  policy key relaxes this rule; the harness sandbox denies these writes
  where one runs.
- **Claude:** `Edit(~/.local/bin/**)` and the rest from the table; sandbox
  `denyWrite` for the home and absolute entries. Claude's file tools run
  no edit-guard (the preset wires only `Bash|PowerShell`,
  `default.json:531`), so a `PATH` directory outside the static table has
  no native protection against the `Write` tool; stated below.
- **Codex:** `cf-guard` and `cf-builder` read-only entries for every
  static entry, as `config.toml:93-163` holds the startup ones.
- **Grok:** entries for the session environment and login item
  directories, which nothing in a session needs to read; none for `PATH`
  directories, since a Grok deny blocks reads and every command resolves
  programs there (the git configuration precedent). Grok's `workspace`
  sandbox already leaves the home and `/opt/homebrew` unwritable.
- **Doctor:** `codeflow doctor --check startup-files` gains rows for a
  `PATH` entry inside the workspace (writable by design, named), a
  user-writable `PATH` entry the static table does not cover (guards only,
  no sandbox entry), an entry that cannot be read, and an empty or
  relative entry (the current directory is on `PATH`; no class can hold
  that).

## Options compared

| Choice | Options | Recommendation and why |
|---|---|---|
| How the `PATH` set is known | (a) static table only; (b) the hook's `PATH` only; (c) both | (c). The sandboxes take static paths only; a static list misses the next tool manager; the hook's `PATH` is the one the harness's shells get, and the operator's terminal `PATH` can differ from it (a GUI-launched harness), which the static table covers for the common directories |
| Which `PATH` directories count | (a) those before the system directories, as the task text says; (b) every user-writable one | (b). Order decides which names are shadowed, never whether shadowing works: `codeflow`, `cargo`, `node` and `python3` live in user directories that commonly come after `/usr/bin`. Changes what adopters may write; operator decision 1 |
| The Homebrew prefix | (a) `bin/` and `sbin/` only; (b) the prefix whole | (b). `bin/` holds links into `Cellar/`, and `lib/` is loaded by every Homebrew program; the text guard does not follow link targets. Operator decision 2 |
| A `PATH` entry inside the workspace | (a) protected; (b) writable, reported by doctor | (b). `direnv layout node` and an activated virtualenv put `node_modules/.bin` or `.venv/bin` on the operator's `PATH`, and `npm install` and `pip install` write there on every run; refusing them would break ordinary work. The route is the operator's own `PATH` choice, so doctor names it. Decided here and disclosed; it keeps writes allowed |
| Rule id | (a) widen `security.shell_startup`; (b) a new no-key rule | (b), `security.persistence`, for the reasons above |
| Installer programs (`brew`, `cargo install`, `npm i -g`, `pipx`, `uv tool`, `go install`, `gem`, `rustup`, `asdf`) | (a) a command family refused in agent sessions; (b) left as opaque programs the sandbox holds | Not in this task: the criteria name the direct write forms. Record (b) as the residual and put (a) to the operator as decision 3, since it is the one route onto `PATH` an unsandboxed seat keeps |
| An ADR | (a) a new ADR; (b) an appended amendment to ADR-0075 plus the enforcement-planes section | (b). No boundary moves; the mechanism is the shipped class machinery, and ADR-0075's 2026-10-05 amendment already names issues 87 to 91 as the sibling classes. The amendment states the rule, the tables' homes and the residuals; `docs/architecture/enforcement-planes.md` "shell startup files" becomes "files that run code later" |

## Adversarial cases

Each row says which layer stops it and what stays open.

| Case | Guards | Sandbox | Open |
|---|---|---|---|
| `cp evil ~/.local/bin/git`, `tee`, `install`, `>`, `ln -s`, `mv`, `chmod +x` into a `PATH` directory, in any spelling the class expands (`~`, `$HOME`, globs, braces, links, case) | refused, `security.persistence` | denied on all three (home unwritable) | |
| `mkdir ~/bin && cp evil ~/bin/git` in two calls | the `mkdir` names the class directory and refuses; the copy refuses on its own | denied | |
| `cd ~/.local/bin && cp ../x git`, `cd "$D" && cp x git` | the directory set judges the first; the second refuses as unresolved near the class (closed rule) | denied | |
| `tar -xf kit.tgz -C ~/.local/bin`, `unzip -d ~/bin`, `rsync -a tools/ ~/.cargo/bin/`, `git clone x ~/bin` | placing programs run in or into the class refuse (`Class::placement`) | denied | an archive member with an absolute name under `tar -P` on a line that names no class place: the content residual the startup class already states |
| `python3 -c "open('/opt/homebrew/bin/git','w')"`, a heredoc that names a class path | refused (interpreter code names a class path) | denied | a path the code builds from pieces (policy reference, "a path assembled by a program") |
| `brew install x`, `cargo install --path ./evil` (a crate whose binary is named `git`), `npm i -g ./evil.tgz` with a `git` bin, `pipx install ./evil` | allowed: opaque programs that write on their own | denied: the home and `/opt/homebrew` are outside every writable root | an unsandboxed seat and Claude's unsandboxed retry; operator decision 3 |
| `printf 'PATH=/tmp/x:$PATH\n' > ~/.config/environment.d/p.conf` | refused: the file is in the class | denied | |
| `launchctl setenv PATH /tmp/x:/usr/bin`, `setx PATH ...`, `systemctl --user set-environment PATH=...` | refused: persistence family | the command itself needs no file write, so the sandbox does not hold it; the guard is the only layer | the PowerShell `-Command` string on a harness that passes it opaquely |
| `cat > ~/Library/LaunchAgents/x.plist <<EOF ... RunAtLoad ... EOF` | refused | denied | |
| `launchctl submit -l x -- /tmp/evil`, `systemd-run --user /tmp/evil`, `at now + 1 min <<< /tmp/evil`, `crontab -` with a piped line, `schtasks /create` | refused: persistence family (`crontab -` already is) | not held by any sandbox | a launcher the family does not list; ADR-0009's open-set caution applies, so the family is a floor |
| `osascript -e 'tell application "System Events" to make login item at end with properties {path:"/tmp/evil"}'` | refused: `osascript` text names a login item | not held | an AppleScript that builds the words at run time |
| A malicious pull request adds `bin/git` to the repository and the operator's `PATH` holds `$PWD/bin` through direnv | not refused: the workspace is writable by design; doctor names the entry | not held: the workspace is the writable root | stated residual; the operator's `PATH` choice |
| A pull request adds `.envrc` setting `PATH` | `.envrc` is in the startup class in every directory; the agent's write refuses; direnv runs only after `direnv allow`, which the startup class refuses | | the operator allowing it by hand |
| `PATH=$PWD/bin:$PATH git push --force origin main` on one line | git-guard judges the git command as spelled; the planted program is not persistence | the push needs the network and credentials the seat has anyway | the one-call route (ADR-0009): not this class |
| A planted link in a protected `bin` whose target is writable (`/opt/homebrew/bin/git` into `Cellar/`, `~/.asdf/shims/node` into `installs/`) | the whole prefix is in the class, so the target refuses too | denied | a link into a directory the table does not hold; the dynamic read protects `PATH` entries only, not their link targets |
| Native `Write` of `~/.local/bin/git` | Codex and Grok: edit-guard refuses. Claude: the `Edit` deny holds for a static entry | | Claude `Write` to a `PATH` directory outside the static table (no edit-guard on Claude); doctor names such an entry |

## What fails closed

- A `PATH` entry the hook cannot stat counts as protected.
- A write whose directory the guard cannot resolve, on a line that names
  the home, a class directory or a `PATH` directory, refuses (the existing
  closed rule and `placement_violation`).
- A placing program with an option the writer table does not list, on a
  line that names a class place, refuses (review round 19 of TSK-242).
- A native edit payload that is truncated, malformed or ambiguous refuses
  (`edit_guard::parse_payload`), as today.
- A class entry whose symbolic link cannot be followed is judged by its
  lexical reading and the link text it holds, as today.
- The `osascript` and PowerShell rules match the words in the text; a
  string the guard cannot read beside those words refuses rather than
  passes, following `Set-ExecutionPolicy`.
- `PATH` absent from the hook's environment: the static table applies and
  doctor warns; nothing is read as "no `PATH` directories".

## What the backstop cannot see

- A program that writes on its own: the installers above, `corepack
  enable` (shims into the node `bin`), `rustup`, `asdf install`, `mise
  install`. The sandboxes hold them where the target is outside the
  writable roots; an unsandboxed Codex seat under a `--sandbox` override, a
  Grok seat on Windows (stub sandbox), native Windows Claude (no Bash
  sandbox) and Claude's unsandboxed retry stay open, as the startup class
  already states.
- A `PATH` directory inside the workspace, and an empty or relative `PATH`
  entry (the current directory is searched): reported by doctor, never
  refused.
- A `PATH` directory on the operator's terminal `PATH` that the harness's
  `PATH` lacks and the static table does not name.
- Link targets outside the class: a `bin` entry that links into a
  directory neither the table nor `PATH` holds.
- Claude's file tools, which run no edit-guard, for an entry outside the
  static table.
- A path built at run time on a line that names no class place; a script
  written in one call and run in another; content a program supplies
  (archive members, diffs, server-chosen names).
- The PowerShell `-Command` string on a harness that hands it over
  opaquely; `cmd.exe` searching the current directory before `PATH`, so a
  `git.bat` in the workspace shadows `git` for every `cmd`-run command
  there, which includes npm scripts on Windows.
- Module search paths (`PYTHONPATH`, `NODE_PATH`, `PERL5LIB`, `RUBYLIB`,
  `GIT_EXEC_PATH`, `LD_LIBRARY_PATH`) set for one call: the one-call
  route, not persistence; their directories, when set in the hook's
  environment, are a possible later extension of the dynamic read.

## Operator decisions

| # | Decision | Protection | Cost |
|---|---|---|---|
| 1 | Every user-writable `PATH` directory counts (recommended), or only those searched before the system directories (the task's wording) | A planted `codeflow`, `cargo`, `node` or `python3` in a late user directory is refused; the guard binary stays the real one | Writes into `~/.cargo/bin`, `~/.local/bin`, `~/go/bin` and the tool managers' trees are refused in agent sessions; the sandboxes already deny them, so the visible change is a named refusal instead of a permission error |
| 2 | The Homebrew prefix counts whole (`/opt/homebrew/`, and `/usr/local/` where the user can write it), recommended, or its `bin` and `sbin` only | A planted library or Cellar binary that every Homebrew program loads is refused | `brew install`, `brew upgrade` and `brew link` are refused in agent sessions (the Claude and Codex sandboxes already deny them); the operator runs them |
| 3 | Follow-up: an installer command family (`brew install`, `cargo install`, `npm i -g`, `pipx`, `uv tool install`, `go install`, `gem install`, `rustup`, `asdf`, `mise`) refused in agent sessions, or left as opaque programs the sandboxes hold | Closes the one route onto `PATH` that an unsandboxed seat keeps, with a refusal that names the rule | Agents cannot set up a missing tool; the operator installs it. The set is open (ADR-0009), so it is a floor; one tracked item if approved |

The workspace `PATH` entry choice and the rule id are reversible choices
decided here and disclosed above.

## Siblings found during this design, not in scope

Each is issue 86's class through another reader of a user file. One tracked
item is recommended for the set; none blocks this task.

- `~/.ssh/config`: `ProxyCommand`, `LocalCommand` with `PermitLocalCommand`
  and `Match exec` run on every ssh, which includes `git push` over ssh.
  Claude denies reads of `~/.ssh/**`; nothing denies the write.
- `~/.cargo/config.toml`: `[alias]`, `runner`, `rustc-wrapper`,
  `[net] git-fetch-with-cli`; a key-level case like TSK-285's.
- `~/.config/gh/config.yml`: an alias with `!` runs a shell on every use.
- `~/.docker/config.json`: `credsStore` names a program found on `PATH`.
- `~/.vimrc`, `~/.config/nvim/`: run when `git commit` opens the editor.
- `~/.local/lib/python3.*/site-packages/usercustomize.py` and
  `sitecustomize.py`: run at every Python start when the user site is
  enabled.
- Editor and terminal configurations that set a shell or `PATH` for every
  terminal: VS Code `terminal.integrated.*`, iTerm2 and Windows Terminal
  profiles; and the repository forms (`.vscode/settings.json`), which a
  malicious pull request can carry.

## Build notes for the builder

- Table: a third `StartupPaths` in `actions.json`, `persistence_paths`,
  with a `why`, `home` and `absolute` lists; `anywhere` empty. The
  `manifest_consistency` and action-table tests cover its generation; the
  presets, installed copies and baseline resync in the same change.
- `StartupEnv` gains `path_dirs: Vec<PathBuf>` read from `PATH` in
  `from_vars` (the Windows branch already reads it for `shell_roots`),
  filtered by the rule above; `Class::persistence(env)` adds each as an
  `Entry::At { dir: true }`. `Kind::Persistence` carries the rule id and
  the noun; `Line::finding` maps it.
- Tests per AC-2: for each of the three classes and each write form
  (native edit, redirect, `cp`, `tee`, `install`), plus `mkdir`, `chmod`,
  a placing program and an interpreter literal; AC-3 allow tests: reads of
  each class, a write beside each class (`~/.local/share/x`,
  `~/Library/Application Support/x`, a workspace `bin/`), a `PATH` entry
  inside the workspace, and the ordinary corpus the startup tests keep.
- The journey (AC-5): a scratch `HOME` with `PATH` naming
  `$HOME/.local/bin`, writes of `$HOME/.local/bin/git`,
  `$HOME/.config/environment.d/p.conf` and
  `$HOME/Library/LaunchAgents/x.plist` through the installed build's
  exec-guard and edit-guard, each refused with the rule and the sanctioned
  path; a registry of allowed controls beside them.
- Docs: the ADR-0075 amendment, the enforcement-planes section, the policy
  reference's "Rules with no key" and its residual list,
  `docs/troubleshooting.md` for the doctor rows, `changelog.d/TSK-284.md`
  (minor: refusals adopters did not see before).
- Hotspots: `edit_guard.rs` and the tables with TSK-283 and TSK-285; land
  after PR 195 or merge it in first, and take TSK-302's directory reader
  when it is on the line.

## Review

| Reviewer | Scope | Verdict |
|---|---|---|
| Codex, GPT-6 Astra at high | this note, before any guard commit | pending |

## Sources

- The code and records cited by path and line above, at `1e6e29e93`.
- Issues 86, 87, 90 and 91 and ADR-0075's 2026-10-05 amendment.
- `npm` 10.9.2 on this host resolves `prefix` to the asdf install, so its
  global and builtin `npmrc` sit under `~/.asdf/installs/` (checked with
  `npm config ls -l`); the pnpm 10.30.1 home is `~/Library/pnpm/` here.
- Platform facts (systemd `environment.d`, `pam_env`, launchd
  `RunAtLoad` and `setenv`, `setx`, the Windows Startup folder,
  `cmd.exe`'s current-directory search) are from the platform manuals and
  were not re-verified on a live Linux or Windows host for this note; the
  builder's journey runs on macOS, and hosted CI runs the table tests on
  Linux and Windows.
