# Package-manager script shell settings: design note

Task: TSK-285. Issue: 89. Status: draft for the other lineage's review
(Codex, GPT-6 Astra at high); no guard code exists yet. Written against
the EPC-022 line at `1e6e29e93`.

## Summary

npm runs every package script through the program its `script-shell`
setting names, and pnpm and yarn 1 read the same key. The setting comes
from a project `.npmrc`, the user's `~/.npmrc`, a global file, an
environment variable or a command-line flag, so an agent that sets it
makes every later `npm test` run a chosen program while the guards see
`npm test`. This note lists the settings that choose a shell or script
runner in npm, yarn and pnpm, says how each is guarded, and names what
the backstop cannot see.

The recommendation: one new action-table family refuses the commands that
set a runner key (`npm config set script-shell`, its yarn and pnpm
counterparts, `npm config edit`, the `--script-shell` and `--userconfig`
flags and the `npm_config_*` variable forms), under a new always-blocking
rule `security.package_runner`; edit-guard and exec-guard refuse a write
that adds a runner key to a package-manager file and allow one that leaves
the runner keys alone, by reading the text the write carries and refusing
when they cannot read it. A bare shell name as the value passes. No new
parser: the key check is a text predicate applied to text the existing
readers already extract. No new ADR.

One choice goes to the operator: whether the guards also refuse `npm run`,
`npm test` and their kin while the effective configuration names a runner
program, which is the route a malicious pull request takes.

## The settings

Verified on this host unless marked: npm 10.9.2 (`npm help config`,
`npm help npmrc`, the installed `docs/content`), pnpm 10.30.1 (the
setting names in its `dist/pnpm.cjs`), yarn 1.22.19 (the names in
`lib/cli.js`). Yarn Berry and Bun are not installed here; their rows say
so.

| Tool | Setting | Read from | What it does | Guard |
|---|---|---|---|---|
| npm | `script-shell` | project `.npmrc`, `~/.npmrc`, `$PREFIX/etc/npmrc`, the builtin `npmrc`, `npm_config_script_shell` in any case, `--script-shell` | the shell for `npm run`, `npm exec` and `npm init <spec>` (default `/bin/sh`, `cmd.exe` on Windows) | runner key: refused |
| npm | `node-options` | same | passed to lifecycle scripts as `NODE_OPTIONS`; `--require`, `--import` and `--loader` load a file into every script's node | runner key: refused |
| npm | `init-module` | same | a JavaScript module `npm init` loads (default `~/.npm-init.js`) | runner key: refused |
| npm | `userconfig`, `globalconfig` | `NPM_CONFIG_USERCONFIG`, `NPM_CONFIG_GLOBALCONFIG`, `--userconfig`, `--globalconfig` | point npm at another configuration file, which can hold any key above | pointer key: refused whatever the value, as `GIT_CONFIG_GLOBAL` is in the startup rules |
| npm | `editor`, `shell`, `browser`, `viewer` | same files | run a program for `npm edit`, `npm config edit`, `npm explore`, `npm docs`, `npm help` | allowed, stated: each runs only on a command the agent spells, and `npm config edit` is refused below |
| npm | `prefix` | same | where `-g` installs put binaries | allowed: the destination is a `PATH` directory, which TSK-284 protects |
| pnpm | `script-shell`, `shell-emulator` (`scriptShell`, `shellEmulator` in `pnpm-workspace.yaml`) | `.npmrc` (project, workspace root, user, global), `pnpm-workspace.yaml`, `npm_config_*`, flags | the shell for scripts; `shell-emulator` selects pnpm's own JavaScript shell | `script-shell`: refused. `shell-emulator`: allowed, since both values are pnpm's own code, never a planted program |
| pnpm | `node-options` (`nodeOptions`) | same | as npm's | refused |
| pnpm | `npm-path` (`npmPath`) | same | the npm binary pnpm runs for some operations | refused |
| pnpm | `use-node-version`, `node-mirror:*` | same | pnpm downloads and runs the named node from the mirror URL | refused: a planted mirror serves a planted node |
| pnpm | `pnpmfile`, `ignore-pnpmfile` | same | which hooks file pnpm runs at install (default `.pnpmfile.cjs`) | `pnpmfile`: refused (it points at code). The file itself is repository code by design; see residuals |
| yarn 1 | `script-shell` | `.yarnrc` in the directory and its ancestors, `~/.yarnrc`, `.npmrc` | the shell for scripts | refused |
| yarn 1 | `yarn-path` | same | a JavaScript file run in place of yarn on every yarn command; `yarn policies set-version` writes it | refused, including `yarn policies set-version` |
| yarn Berry (not installed; from the configuration reference, unverified here) | `yarnPath`, `plugins`, `injectEnvironmentFiles` | `.yarnrc.yml` in the directory and its ancestors, `~/.yarnrc.yml`, `YARN_<KEY>` variables, `yarn config set`, `yarn set version` | a file run as yarn, plugin files loaded into yarn, environment files loaded into scripts | refused. Berry has no script shell key: it ships its own shell |
| Bun (not installed; unverified) | `bunfig.toml` `[run] shell` | project and `~/.bunfig.toml` | the shell for `bun run` | named as a residual for the builder to confirm; add if the setting exists |

Key spelling: npm reads `npm_config_FOO` and `NPM_CONFIG_FOO` alike and
maps `_` to `-`; pnpm reads both the dashed and camel forms. The guard
matches a runner key case-insensitively with `_`, `-` and the camel form
as one name, a superset that fails closed.

## How each form is guarded

```text
   form                                   layer                        rule
   ─────────────────────────────────────  ───────────────────────────  ─────────────────────────
   npm config set script-shell X          native deny + exec-guard     security.package_runner
   npm set / npm c set, --location, -g    exec-guard parse             security.package_runner
   npm config edit, yarn config set,      native deny + exec-guard     security.package_runner
     pnpm config set, yarn set version
   --script-shell X, --userconfig F       exec-guard parse             security.package_runner
   npm_config_script_shell=X npm test     exec-guard (startup vars)    security.package_runner
   Edit/Write/apply_patch of .npmrc       edit-guard, text read        security.package_runner
   echo 'script-shell=X' >> .npmrc        exec-guard, text read        security.package_runner
   sed -i 's/.../' .npmrc                 exec-guard, sed reader       security.package_runner
   npm test under a planted .npmrc        (operator decision)          security.package_runner
```

### Commands

A new family `runner_settings` in `security/actions.json`, decision
`deny`, generates the native text rules for the three harnesses (the
publish family at `actions.json` is the template; its `codex_note` on
leading flags applies here too) and exec-guard parses the unwrapped argv
in `security/outward.rs` beside the publish rows, which already skip
npm's, pnpm's and yarn's global options (`outward.rs:495-505`):

- `npm config set|npm set|npm c set` with a runner or pointer key in any
  pair form (`key value`, `key=value`, several pairs), at any location
  (`--location=project|user|global`, `-g`, `--global`, `--userconfig F`);
  `npm config delete` and `get` pass.
- `npm config edit`: refused, since the guard cannot see what the editor
  writes.
- Any npm, pnpm or yarn command carrying `--script-shell`,
  `--node-options`, `--init-module`, `--userconfig` or `--globalconfig`
  (attached or separate), or an `npm_config_<runner key>` or
  `NPM_CONFIG_<RUNNER KEY>` assignment on the line, exported or set for
  the command. These join `STARTUP_VARS` in `security/startup.rs:1279`
  in spirit: variables that pick the program refuse whatever the value.
- `pnpm config set` with a runner key; `pnpm config set` writes the
  project `.npmrc` or, on pnpm 10, may write `pnpm-workspace.yaml`; the
  builder confirms which and the file class holds both.
- `yarn config set` with `script-shell`, `yarn-path` or `yarnPath`;
  `yarn policies set-version`; `yarn set version`; `yarn plugin import`.

The refusal names the rule and the sanctioned path: the operator sets a
script runner in their own terminal, outside the agent session; a bare
shell name (`sh`, `bash`, `zsh`, `pwsh`, `powershell`, `cmd`, `cmd.exe`)
passes, since the program it names is found on `PATH`, which TSK-284
protects.

### Writes to the files

The file class: `.npmrc`, `.yarnrc`, `.yarnrc.yml` and
`pnpm-workspace.yaml` in any directory (an `anywhere` list in the action
table, as `.envrc` is), plus `~/.npmrc`, `~/.yarnrc`, `~/.yarnrc.yml` and
`~/.bunfig.toml` as home entries. A write to one is judged by the text it
adds:

- **Native edits.** edit-guard's `EditRequest` carries paths only
  (`hooks/edit_guard.rs:26-30`: "content never participates"). For this
  class alone the hook reads the written text: `Write`'s `content`,
  `Edit`'s and `search_replace`'s new string, the `+` lines of an
  `apply_patch` hunk. A runner key on an added line refuses; a payload
  whose text the hook cannot read (a truncated input, a tool shape it
  does not parse) refuses. The enforcement-path check keeps its
  content-free contract; this is a second, class-scoped check in the same
  hook.
- **Shell writes.** The startup module already knows the text a line
  produces: `echo` and `printf` arguments, heredoc bodies, the line's
  literal assignments (`literal_assignments`, `produces_text`,
  `staged_run` in `security/startup.rs`), and TSK-290's sed reader
  (`security/sed_script.rs`) gives the replacement text of an `s`
  command. A redirect, `tee`, `sed -i` or interpreter literal that targets
  a class file is judged by that text: a runner key in it refuses; text
  the guard cannot read (a variable, command output, `sed -f`, a `w` or
  `e` command) refuses; a registry line, a comment or a `d` command
  passes. The writer table (`security/unresolved.rs`, which TSK-299
  extends with `sort -o`, `curl -o` and `wget -O`) supplies the targets:
  a download or copy into a class file carries text the guard cannot
  read, so it refuses.
- **Value check.** A key with a bare shell name passes; a path, an
  expansion or anything else refuses. `shell-emulator=true|false` passes.
- **Rule and relief.** `security.package_runner`, no policy key, always
  blocking, in the policy reference's "Rules with no key". A project that
  needs `script-shell=bash` for Windows scripts sets it in the operator's
  terminal, and the guard passes an edit that leaves the line unchanged.

### Sandbox entries

The home files get Claude `Edit` denies and sandbox `denyWrite` entries
and Codex read-only entries, generated from the table as the git
configuration files are; Grok gets none, since every npm command reads
`~/.npmrc` and a Grok deny blocks reads. The project files get no sandbox
entry: adopters edit them. The home is already unwritable in all three
sandboxes, so the entries change nothing in practice except where a
writable root contains the home; there the sandbox refuses a user-scope
registry edit the guard would allow, as it does for `git config --global
user.name` today (policy reference, the git configuration residual).
That is stated, not hidden. `~/.npmrc` also holds the registry token, so
a Claude read deny for it is worth adding in the same change.

## Composition with the in-flight guard work

| Work | How this task composes |
|---|---|
| TSK-283 (PR 195), the shared git configuration reader (`security/git_config.rs`: `read`, `get`, `boolean`, "cannot verify"; a `get_all` is being added) | Not used: npm's precedence is npm's, not git's. If the operator takes the `npm run` check below, a bounded `.npmrc` reader follows the same contract (`get(key) -> Result<Option<..>, String>`, "cannot verify" on anything it cannot read, no process started), kept to `key = value` lines, `;` and `#` comments and `${VAR}` refused as unread. It is a file reader, not a second command-line parser |
| TSK-302 (PR 187), interpreter operands and the line's directory set (`outward::line_directories`) | A relative `.npmrc` after `cd "$D"` is judged against the directory set; an unresolved set with a bare `.npmrc` tail refuses, as TSK-302's rule does for `settings.json` |
| TSK-299 (plan PR 162), writer output options | The new class gets every writer the table lists through the one judge; `sort -o .npmrc`, `curl -o .npmrc` refuse as unreadable text |
| TSK-284, files that run code later | `prefix`, `corepack enable` and `npm i -g` land in `PATH` directories TSK-284 protects; the two tables are siblings in `actions.json`; both touch `edit_guard.rs`, so they land in sequence |
| TSK-290 (landed in 3.1.1), the sed reader | Supplies the replacement text for the key check |

## Adversarial cases

| Case | Guards | Sandbox | Open |
|---|---|---|---|
| `npm config set script-shell /tmp/x` (any pair form, `npm set`, `--location`, `-g`, through `bash -c`, `eval`, `env`, a launcher the reader unwraps) | refused | not held: `npm` writes `~/.npmrc` or the project file, which the sandbox allows inside the writable root; the user file is denied | a launcher the reader does not unwrap, which the closed rule refuses only when the line names a class file |
| `npm_config_script_shell=/tmp/x npm test`, `NPM_CONFIG_SCRIPT_SHELL=`, `export npm_config_script_shell=... ; npm test` | refused | not held | the variable exported in an earlier tool call, which the hook never sees; one call, not persistence |
| `npm test --script-shell=/tmp/x`, `npm run build --userconfig /tmp/evil.npmrc` | refused | not held | |
| `echo 'script-shell=/tmp/x' >> .npmrc`, `printf`, a heredoc, `tee -a`, `python3 -c "open('.npmrc','a').write('script-shell=...')"` | refused: the text names a runner key | the project file is writable | |
| `echo "$LINE" >> .npmrc`, `curl -o .npmrc`, `cp /tmp/rc .npmrc`, `sort -o .npmrc`, `sed -i -f s.sed .npmrc` | refused: text the guard cannot read into a class file | | |
| `sed -i 's/^registry=.*/registry=https:\/\/r.example/' .npmrc`, `echo 'registry=https://r.example' >> .npmrc`, an `Edit` of the registry line | allowed: the text names no runner key | | |
| `Write` of `.npmrc` with `script-shell=/tmp/x` on Codex, Grok or Claude | Codex and Grok: edit-guard refuses. Claude: no edit-guard runs on `Write`, and a project `.npmrc` has no `Edit` deny | | Claude's `Write` and `Edit` tools on a project file: a named residual, the same gap the startup class states for a moved `ZDOTDIR`; the `npm run` check below would catch the planted file on first use |
| `echo 'script_shell=/tmp/x' >> .npmrc`, `scriptShell:` in `pnpm-workspace.yaml`, `SCRIPT-SHELL` | refused: one key in every spelling | | |
| `yarn policies set-version 1.22.0` writing `yarn-path`; `yarn set version stable` writing `yarnPath` | refused | | an operator who pins yarn this way does it in their terminal |
| A malicious pull request adds `script-shell=./scripts/sh.sh` to the project `.npmrc`; the agent, CI or the operator runs `npm test` | not refused today; the operator decision below adds the check | | CI and the operator's terminal are outside every guard; `codeflow ci` reporting the key is the cover for them |
| A pull request adds `.pnpmfile.cjs`, a `preinstall` script in `package.json`, a `Makefile` with `SHELL := ./x`, a `.envrc` | repository code by design, except `.envrc`, which the startup class refuses an agent's write to | | stated residual: build files are code; `ignore-scripts` is the control npm offers |
| `NODE_OPTIONS=--require /tmp/x.js npm test` | not refused: a one-call variable with ordinary uses (`--max-old-space-size`) | | the one-call route; the persistent form (`node-options` in a file, or in a startup file) is refused |

## What fails closed

- A write to a class file whose text the guard cannot read refuses.
- A native edit payload for a class file whose content the hook cannot
  extract (truncated, an unparsed tool shape, a hunk without `+` lines it
  can attribute) refuses.
- A runner key whose value the guard cannot read (an expansion, a
  substitution) refuses; only a bare shell name passes.
- A `.npmrc` tail after a directory change the guard cannot resolve
  refuses (TSK-302's rule).
- A pointer flag or variable (`--userconfig`, `NPM_CONFIG_USERCONFIG`)
  refuses whatever its value.
- If the operator takes the `npm run` check: a configuration file the
  reader cannot read refuses the run with "cannot verify", as TSK-283's
  reader does.

## What the backstop cannot see

- Claude's `Write` and `Edit` tools on a project `.npmrc`: Claude runs no
  edit-guard, and a project file cannot carry a native deny without
  blocking every edit to it.
- A variable exported in an earlier tool call; a script written in one
  call and run in another; a path or text built at run time on a line
  that names no class file.
- `NODE_OPTIONS` set for one call.
- The configuration file a pull request carries, until the `npm run`
  check exists; CI and the operator's own terminal run it regardless.
- Files that are code by design: `package.json` scripts, `.pnpmfile.cjs`,
  `Makefile`, `tasks.json`, `mise.toml` tasks and `_.source`.
- `corepack` and the `packageManager` field, which download and run the
  named package-manager version; a hash in the field pins it, an absent
  hash trusts the registry.
- Bun's `bunfig.toml` until the builder confirms its setting.
- Other tools with the same shape of setting, out of scope and recorded
  for one tracked item: `~/.cargo/config.toml` (`runner`, `[alias]`,
  `rustc-wrapper`), `~/.docker/config.json` (`credsStore`), gh aliases.

## Operator decision

| # | Decision | Protection | Cost |
|---|---|---|---|
| 1 | Add to TSK-285 (or one follow-up task) a check that refuses `npm run`, `npm test`, `npm exec`, `npm init <spec>`, `npx`, and the pnpm and yarn 1 counterparts, while the effective configuration (project file, user file, global file, environment) names a runner program; a bare shell name passes; a file the reader cannot read refuses as "cannot verify". Recommended | Closes the malicious pull request route inside agent sessions, where the planted key is otherwise invisible until the script runs; the same reader lets `codeflow ci` report a tracked runner key to the reviewer | One bounded ini and YAML read per package-manager call inside the 10 s hook budget; a project with a legitimate runner path refuses until the operator sets it by hand; three small readers (ini, yarn 1's `key value`, YAML through the existing parser) |

Whether a bare shell name passes is decided here as a reversible choice:
refusing every value would block the one ordinary use (`bash` for
Windows projects) for no gain, since the program is found on a `PATH` that
TSK-284 protects.

## Build notes for the builder

- Table: a `runner_settings` family (commands) and a `package_manager_paths`
  table (files, `anywhere` and `home`) in `actions.json`; the runner key
  list lives in one Rust constant beside the table and is cited by the
  docs; a test fails when a key in the table has no row in the policy
  reference.
- exec-guard: rows in `outward.rs` for the command forms; the variable
  forms join the startup module's command-valued variable check; the
  class joins `startup::evaluate_with` as a third kind with its text
  predicate.
- edit-guard: a class-scoped content read in `hooks/edit_guard.rs`, fed
  by the parsed payload; the `EditRequest` contract for enforcement paths
  stays content-free.
- Tests per AC-2: exec-guard refusals per tool and per form, allow tests
  for `npm config set registry`, `npm config get script-shell`, `npm
  config delete script-shell`, `pnpm config set shell-emulator true`,
  `yarn config set registry`; per AC-3: planted tests per file and per
  write form (native edit, redirect, `tee`, `sed -i`, interpreter), an
  allow test for a registry line in each file and for an `Edit` that
  changes another line.
- The journey (AC-5): a fresh project with the installed build, `npm
  config set script-shell /tmp/x` refused, `script-shell=` appended to the
  project `.npmrc` refused through exec-guard and through Codex's or
  Grok's edit-guard, a registry line allowed both ways.
- Docs: an appended amendment to ADR-0075 (with TSK-284's, one note for
  the two classes), the enforcement-planes section, the policy reference's
  "Rules with no key" and residuals, `changelog.d/TSK-285.md` (minor).
- Hotspot: `edit_guard.rs` with TSK-283 and TSK-284; land after PR 195 and
  after or with TSK-284's table change.

## Review

| Reviewer | Scope | Verdict |
|---|---|---|
| Codex, GPT-6 Astra at high | this note, before any guard commit | pending |

## Sources

- npm 10.9.2: `docs/content/using-npm/config.md` (`script-shell`,
  `node-options`, `init-module`, `editor`, `shell`, `browser`, `viewer`,
  `userconfig`, `globalconfig`, "Environment Variables") and
  `configuring-npm/npmrc.md` ("Files"), read from the installed npm.
- pnpm 10.30.1: the setting names found in `dist/pnpm.cjs` on this host
  (`script-shell`, `shell-emulator`, `node-options`, `npm-path`,
  `use-node-version`, `pnpmfile`, `ignore-pnpmfile`, their camel forms,
  `pnpm-workspace.yaml`, `.pnpmfile.cjs`); their semantics are from the
  pnpm settings reference and were not exercised here.
- yarn 1.22.19: the names `script-shell`, `yarn-path`, `.yarnrc` and
  `.npmrc` found in `lib/cli.js` on this host.
- The code and records cited by path and line above, at `1e6e29e93`;
  issue 89; ADR-0075's 2026-10-05 amendment; TSK-283, TSK-299 and TSK-302
  as their pull requests stand on 2026-10-14.
