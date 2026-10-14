# CodeFlow's push credential setting

Design note for TSK-291 (issue 151). It settles, before any code, where
the credential helper CodeFlow applies to its own pushes is read from, which
commands push, how the helper is applied, what git-guard sanctions and still
refuses, and whether a missing non-personal credential refuses under an
opt-in policy. Written 2026-10-14 against the EPC-022 line at `1e6e29e93`,
with the installed `codeflow 3.1.0` guard and git 2.53.0 for the live
checks. Revised the same day for the Codex design review at `2ffa0ad75`
(`target/review-design-TSK-291-codex-01.md`) and the operator's decisions.

Where it stands: both operator decisions are settled and recorded below.
The review's six findings are answered in the design; the disposition
table at the end names each one. The revision goes back to the reviewer.

## Summary

- CodeFlow pushes through three call sites (`ids/issue.rs:461`,
  `readiness.rs:1268`, `light_paths.rs:259`), reached from eleven CLI
  commands and one hook entry, the pre-push hook's pending-id sync.
- Settled: the helper is declared in `.codeflow/project.toml` under a
  `[push]` table and read only from the configured remote's default-branch
  tracking ref. A branch, an integration line, the working copy, `HEAD`,
  the environment and the repository's git config never supply it.
- The value is an absolute path to an executable outside every repository
  path, with literal arguments, resolved through symlinks and checked at
  push time; under `required` its SHA-256 is pinned in the same table. Bare
  helper names and `!` shell snippets are refused.
- CodeFlow applies it on its own push invocations only, as `-c` arguments
  with the helper list reset, `credential.useHttpPath=true`, askpass off and
  redirects off. A preflight at each command boundary resolves every
  effective push destination and refuses when another authentication
  source would answer first: a credential in the URL, an `http.extraHeader`,
  a netrc entry for the host, a cookie file, a client certificate, or TLS
  verification turned off. Destinations must be `https://`.
- Settled: `git.push_credential = "required"` makes every CodeFlow push
  refuse before any local write when no valid helper is configured at the
  authority, the operator's own terminal included. An invalid value or an
  unreadable authority refuses under both policy values.
- Doctor gains one check, `push-credential`. One short ADR is warranted.

## Credential handling rule for this task

No experiment, test or doctor check runs `git credential fill`,
`git credential approve`, or anything that asks the real system, keychain
or user credential helpers for a credential. Tests use isolated recording
helpers only, with `credential.helper=` reset first, inside a disposable
repository, with `GIT_CONFIG_NOSYSTEM=1` and a temporary `HOME`. The first
version of this note broke that rule once (experiment A below); the
operator has been asked to rotate the token it exposed.

## The problem at 3.1.1

Live checks with the installed `codeflow 3.1.0` guard in this worktree
(`codeflow hook git-guard` with a Bash payload), and git 2.53.0 with
recording helpers:

| Line or situation | Today | Evidence |
|---|---|---|
| `GIT_CONFIG_COUNT=... codeflow task new x` (the adopter's route) | refused, `git.hook_integrity`: the line names a `GIT_CONFIG` variable and `codeflow` is a program the guard does not read | closed rule, `git_guard.rs:664-680`, `security/unresolved.rs:1513` |
| `GIT_CONFIG_PARAMETERS=... codeflow task new x`, `GIT_CONFIG_GLOBAL=... codeflow ...` | refused, same rule | live check |
| `GIT_CONFIG_* git push origin HEAD` | refused, `git.policy_authority`: command-local configuration can redirect the configured remote | `git_guard.rs:6474-6490` |
| `codeflow task new x` with no prefix | allowed; the registry reservation goes out with whatever credential git finds | `ids/git.rs:32-41` sets only `GIT_TERMINAL_PROMPT=0` and `LC_ALL=C` |
| A system or global keychain helper is present (the usual macOS or `gh` setup) | the personal credential answers before any repository helper is asked; git stops at the first helper that supplies both fields | experiment A below; gitcredentials(7) "each helper will be tried in turn" |
| Repository helper added without the empty reset | the earlier helpers still answer first | experiment C |
| Repository helper that answers only for a path, no `credential.useHttpPath` | the helper is called without `path=` and answers nothing; with the empty reset there is no fallback | experiment B2; the adopter's 2026-10-13 comment |
| The helper list is reset and only one helper is configured, and a `~/.netrc` names the host, or the URL carries `user:password@`, or an `http.<url>.extraHeader` holds an Authorization header | git authenticates from that source; the helper is asked and its answer is not needed, or never used | loopback experiments 1 to 4 below |
| `git -c credential.helper='!h' push ...`, `git -c core.askPass=/x push ...`, `GIT_ASKPASS=/x git push ...`, `GIT_ASKPASS=/x codeflow task new x` | allowed by git-guard and exec-guard | live check; `scan_git_globals` reads only `core.hooksPath` (`git_guard.rs:7782-7820`); `transport_config_environment` reads only `GIT_CONFIG*`, `HOME`, `XDG_CONFIG_HOME` (`git_guard.rs:919-926`) |
| `git config --local credential.helper '!h'`, `git config --worktree credential.helper ...` | allowed by both guards in this checkout; repository-scope writes pass by design (`git_guard.rs:12800-12817`). The issue's "refused or flagged" was not reproduced here; it may come from a native deny or another host | live check |
| `git config --local remote.origin.pushurl ...` | refused, `git.policy_authority` | live check |

The 3.1.1 upgrade note (`CHANGELOG.md:38-47`) tells the operator to add a
repository-scope helper after an empty value. That route is correct for
the agent's own `git push` lines and stays available. It omits
`credential.useHttpPath`, it is three hand-written settings on every
clone, nothing reports it, and the loopback experiments show that a helper
alone does not exclude the other authentication sources.

## Which CodeFlow commands push

Every push goes through one of three call sites. No libgit2 push exists in
the crates (no `remote.push` or `PushOptions` use), `codeflow integrate` is
local, and `codeflow remote protect` uses the GitHub API, not git.

| Entry | Kind | Call site | Path to the push | Writes before the push |
|---|---|---|---|---|
| `task new`, `epic new`, `spec new`, `adr new` | CLI | `crates/codeflow-core/src/ids/issue.rs:459-461` | `cmd/new.rs:616` calls `issue::reserve` (`issue.rs:80`); `reserve_locked` publishes pending entries first (`issue.rs:99`) then pushes the reservation (`issue.rs:126`) | the record file is written after the reservation; nothing before |
| `feedback new` | CLI | same | `cmd/feedback.rs:292` calls the same issuer | `FeedbackConfig::write_defaults` writes `[feedback]` into `project.toml` first (`feedback.rs:276-285`) |
| `epic new --integration` | CLI | `crates/codeflow-core/src/workgraph/light_paths.rs:259` | `create_integration_branch` pushes `integration/EPC-NNN-<slug>` with `-u` after the reservation push above | `git branch` at `light_paths.rs:253` |
| `work claim` | CLI | `crates/codeflow-core/src/workgraph/readiness.rs:1268-1280` | `claim_on` pushes `task/<id>-<slug>` create-only with `--force-with-lease=<ref>:` | `git branch` or `branch -m` at `readiness.rs:1260-1264` |
| `ids sync` | CLI | `issue.rs:461` | `issue.rs:590-603` then `publish_pending` (`issue.rs:664`, pushes at `690` and `727`) | none |
| `ids admit` | CLI | same | `issue.rs:858` publishes pending entries before admitting | none |
| `ids restore` | CLI | same | `issue.rs:963` | none |
| `ids retarget` | CLI | same | `seed.rs:588` calls `issue::reserve` | `issue::drop_pending` at `seed.rs:576` |
| `ids seed` | CLI | same | `seed.rs:158` | none |
| `codeflow git-hook pre-push` | hook, run by git inside any push to `origin` that pushes code while reservations are pending | same | `cmd/git_hook.rs:162` calls `sync_pending_ids`, which calls `issue::sync` at `git_hook.rs:219`; it never runs for a push of the registry itself, so CodeFlow's own registry push does not recurse (`git_hook.rs:180-193`) | none |

So eleven CLI entries and one hook entry, through three sites. The
registry site runs git through `ids/git.rs:32-41` (`GIT_TERMINAL_PROMPT=0`,
`LC_ALL=C`, stdin closed). The claim and integration-branch sites run
`crate::git::command()` (`git/mod.rs:41-59`) with no environment of their
own, so a missing credential there can hang on a prompt. All three build
on `crate::git::command()`, which is where one shared push builder fits.

Fetches and `ls-remote` calls (`issue.rs:380`, `remote_query.rs:28`,
`readiness.rs` fetches) stay on the ambient credential in this design: a
read carries no authorship, the push is what the host records under an
identity, and AC-2 and AC-6 speak of pushes. Extending the helper to every
CodeFlow transport call is a possible later setting.

## Design

### 1. Where the setting is read from (settled)

Operator decision, 2026-10-14: the helper is read from
`[push] credential_helper` in `.codeflow/project.toml` at the remote
default-branch authority.

Four sources were compared.

| Option | Where | A PR can change it for its own pushes? | An agent can change it in-session? | Reported by doctor? | Cost |
|---|---|---|---|---|---|
| A. `.codeflow/project.toml` `[push]` at the configured remote's default branch (settled) | the tracking ref `refs/remotes/origin/HEAD` (or `main`/`master` fallback), read as git data by `hooks/landed_policy.rs:219-229` | no: the branch's own copy is never read; the value takes effect once landed on the default branch, the same way policy levels do (ADR-0075 D5 amendment; `enforcement-planes.md:141-166`) | no: the file is an integrity path git-guard and edit-guard refuse to write (ADR-0009), and the working copy is not read anyway | yes, from the same ref as `policy-source` | one key in a file every adopter already has; the default branch must be fetched before a push |
| B. the operator's user-scope git config, `git config --global codeflow.pushCredentialHelper` | `~/.gitconfig` or `~/.config/git/config` | no | no: git-guard refuses user-scope writes of keys not known safe (`git_guard.rs:12755-12800`) and `GIT_CONFIG_GLOBAL` on a `codeflow` line; `HOME=` on a `codeflow` line passes (live check), so the file git reads can be redirected | yes | per machine and per operator; CI runners and every contributor repeat the provisioning; the project policy can still require a locally provisioned helper, so B does not stop a project from requiring a bot, its costs are provisioning and protecting the `HOME` and config resolution |
| C. the repository's git config (the 3.1.1 route) | `.git/config` | no | yes: repository-scope `credential.helper` writes pass both guards (live check) | not today | already documented; keeps working for the agent's own `git push` lines |
| D. an environment variable | the session | n/a | yes, in band | no | rejected: ADR-0009's reasoning, an agent shares every in-band credential |

A reuses the authority the guards already judge by. It is a local
enforcement assumption like the in-tree hooks, never an operating-system
boundary: the machine that trusts the repository's hooks trusts its
default branch to name a program. Neither A nor B proves that the token a
helper returns belongs to a bot; they decide who may name the helper.

Two narrowings of the existing reader apply to this key:

- Only the default sources contribute. `landed_policy::load` lets a declared
  target line's `project.toml` win over the default's
  (`landed_policy.rs:109`, `line_project.or(project)`). Integration lines
  are landed by the primary agent, so a line must never pick the program
  CodeFlow runs. The helper is read from `defaults` only
  (`landed_policy.rs:76-91`); with the `main` plus `master` fallback, a
  conflict between them refuses as it does for project settings today
  (`landed_policy.rs:87-89`). The merged `project` field the reader exposes
  today is not used for this key.
- Never the working copy or `HEAD` for the helper. Section 4 states what
  happens when the authority cannot be read; it is never a fallback to the
  checkout.

A no-remote repository never pushes (`issue.rs:91`, `light_paths.rs:255`,
`readiness.rs:1211`), so the setting is moot there.

### 2. The setting's shape and the executable trust contract

```toml
[push]
credential_helper = "/usr/local/libexec/codeflow-bot-credential --org acme"
credential_helper_sha256 = "3b7f...e1"   # pins the executable; required under git.push_credential = "required"
```

The review's R2 is right that trusted text does not establish a trusted
executable. The contract below says what is trusted and how it is checked
at every push, not only when the setting is read.

| Rule | Check | Why |
|---|---|---|
| The first word is an absolute path | at read and at push time | no `PATH` lookup, no `git credential-<name>` wrapper; a bare name is refused |
| The path resolves, through every symlink, to a regular executable file | at push time, with the resolved path reported | a dangling or replaced link refuses rather than runs something else |
| The resolved path lies outside every repository path: the worktree root, the common git directory, every registered worktree and the `.worktrees` directory, compared by real path | at push time | a pull request cannot place or change the program; a helper inside a checkout refuses even through an absolute path or a symlink |
| `credential_helper_sha256` pins the file's content | at push time; optional under `ambient`, required under `required`; doctor warns when unset under `ambient` | a file the operator's user can write is a file the agent can write; the pin turns substitution into a refusal. The operator updates the pin through a pull request when the helper changes |
| Arguments are literal words matching `[A-Za-z0-9_.,:=@+/-]+`, separated by single spaces; no quotes, no shell metacharacters, no `$`, no `!` | at read time | the helper string is executed by `/bin/sh` even for an absolute path (gitcredentials(7)), so CodeFlow composes the string itself, single-quoting each word, and the grammar guarantees the shell can only split it |
| An argument containing `/` is validated as a path by the rules above (absolute, resolved, outside every repository path) | at push time | an interpreter with a repository script argument has the same problem as a repository helper |
| An argument that names an existing file relative to the repository root is refused | at push time | git runs the helper with the repository as its working directory, so a relative file argument would resolve inside the checkout |
| Arguments must not contain secrets | documented; doctor prints the value | doctor and refusals show the full value |
| A value beginning with `!`, a relative path, or an empty string is refused | at read time | a shell snippet is unreadable to a reviewer; empty is git's reset spelling |
| No second key for `useHttpPath` | n/a | CodeFlow always passes `credential.useHttpPath=true`; a host-scoped helper ignores `path=` |

What the contract does not claim: the operator's user owns the machine, so
the operator and any process running as that user can change the helper.
The pin makes a change a refusal; it does not make the helper
agent-proof. git itself is found through `PATH` by every CodeFlow git
call (`git/mod.rs:41-59`), a residual shared with the whole binary and
outside this task.

TSK-283's reader applies the same contract, not string equality: a
repository-scope `credential.helper` on an agent's git line counts as the
sanctioned program only when its value passes every check above against
the authority's value, pin included. Otherwise it is an unknown program.
Both tasks share one validator module.

### 3. How the helper is applied

One shared builder, provisional name `push_credential::apply`, used by the
three call sites and by nothing else. For a push with a configured helper
`<h>` (the composed, quoted string) the invocation is:

```
git -C <root> \
  -c credential.helper= \
  -c credential.helper=<h> \
  -c credential.useHttpPath=true \
  -c core.askPass= \
  -c http.extraHeader= \
  -c http.followRedirects=false \
  push ...
```

with this environment on the child: `GIT_TERMINAL_PROMPT=0`; `GIT_ASKPASS`,
`SSH_ASKPASS`, `GIT_SSL_NO_VERIFY`, `GIT_SSL_CERT`, `GIT_SSL_KEY`,
`GIT_TRACE`, `GIT_TRACE_CURL`, `GIT_TRACE_CURL_NO_DATA` and
`GIT_CURL_VERBOSE` removed; everything else inherited, `GIT_CONFIG_PARAMETERS`
included, because the preflight in section 4 reads the effective
configuration under the same environment and the Claude sandbox relies on
its own entries there. Without a configured helper the builder adds
nothing, so the `ambient` behaviour is today's.

| Element | What it does | Evidence |
|---|---|---|
| `-c credential.helper=` | resets the helper list, dropping system, global, URL-scoped and repository helpers, and any `GIT_CONFIG_PARAMETERS` entries inherited from the environment, since `-c` values are appended after them | gitcredentials(7) "configured to the empty string, this resets the helper list"; experiments B and C |
| `-c credential.helper=<h>` | the only helper git asks | experiment B; loopback experiment 5 |
| `-c credential.useHttpPath=true` | the helper receives `path=<org>/<repo>.git`, so a helper scoped by repository can answer; a host-scoped helper ignores it | git-config(1) `credential.useHttpPath`; experiments B and B2 |
| `-c core.askPass=` plus the removed askpass variables and `GIT_TERMINAL_PROMPT=0` | when the helper supplies nothing, git fails instead of falling back to an ambient askpass program or a prompt | experiment D (an ambient `GIT_ASKPASS` answered after the helper gave nothing) and E (fatal, no fallback) |
| `-c http.extraHeader=` | resets the generic extra-header list; it does not reset a URL-scoped `http.<url>.extraHeader`, which the preflight refuses instead | git-config(1); loopback experiment 3 |
| `-c http.followRedirects=false` | a redirect is an error, so a push never re-authenticates with another host; a renamed repository refuses with git's message until the operator fixes the URL | git-config(1) `http.followRedirects` |
| removed TLS override variables | the preflight's TLS check cannot be undone by the environment | git-config(1): `GIT_SSL_NO_VERIFY` overrides `http.sslVerify` |
| removed trace variables | git's curl trace prints request headers to stderr, and CodeFlow folds stderr into its error text (`issue.rs:465-470`, `readiness.rs:1279`), which can land in a terminal or a record | the Authorization header carries the token |
| arguments, not `GIT_CONFIG_*` environment | nothing on the agent's line changes, so AC-3's refusal of hand-built environments stays meaningful; the child hooks see the helper string through `GIT_CONFIG_PARAMETERS`, which git exports for `-c` values, and never the secret | hook experiment below |

Why `-c` on CodeFlow's own calls and not a repository config write: a write
would be a standing setting an agent could later change, the guard would
have to learn it, and it would also govern the agent's own `git push`
lines, which this task leaves to the operator's 3.1.1 route.

What else changes at the call sites:

- The claim and integration-branch sites gain `GIT_TERMINAL_PROMPT=0` and
  stdin closed even under `ambient`, so a missing credential fails rather
  than hangs. This is a behaviour change worth one changelog line.
- `classify_push` (`issue.rs:478-525`) learns the shape of a helper that
  supplied nothing (`could not read Username`, `Authentication failed`) and
  reports it as a permission failure naming the helper and doctor, instead
  of `Unclear`, which today triggers a read-back.
- The helper and policy are resolved once per command boundary (section 4)
  and threaded to the push site, so one command never pushes under two
  identities. The push site itself refuses when it is handed no helper
  under `required`, as a second check.

### 4. The preflight: destinations, other authentication sources, and when it runs

The reset in section 3 governs only the helper list. The loopback
experiments show three other sources git uses before or instead of a
helper, and the git source the review cites names them: a credential in
the URL (git completes the credential from the URL and never needs the
helper's answer), an `http.extraHeader` carrying `Authorization`, and
curl's own netrc lookup, which git enables and does not expose a switch
for. Cookies (`http.cookieFile`), a client certificate (`http.sslCert`)
and `http.emptyAuth` (GSS negotiation under the caller's own identity) are
further identity sources. None can be reset from the command line in a way
the preflight can trust, so the design refuses when one would apply.

The preflight runs whenever a helper is configured at the authority or the
policy is `required`. It never runs the helper. Its steps, in order:

| Step | How | Refuses when |
|---|---|---|
| 1. Read the authority | the default-source refs only (section 1); when `origin` is reachable, refresh them first with a non-forced fetch of the existing default-source names (`refs/heads/<name>:refs/remotes/origin/<name>`); when unreachable, use the refs as last fetched, which are the landed authority as last seen | the authority cannot be read (section "Proven absence, invalid content, unreadable authority") |
| 2. Validate the helper | the contract in section 2, pin included | any rule fails; under `required`, also when the key is absent |
| 3. Resolve every effective push destination | `git remote get-url --push --all origin`, which expands `insteadOf` and `pushInsteadOf` and lists every push URL (git-remote(1)); run under the child environment of section 3 | the list is empty or unreadable |
| 4. Check each destination | scheme must be `https`; no userinfo (`user@` or `user:password@`); host non-empty | any destination fails; an `http://` destination is refused in production, with the test-only accommodation below |
| 5. Check the effective HTTP configuration per destination | `git config get --all --url=<dest> <key>` (git-config(1), the replacement for `--get-urlmatch`), run under the child environment so inherited `GIT_CONFIG_PARAMETERS` count, for `http.extraHeader`, `http.cookieFile`, `http.sslCert`, `http.sslKey`, `http.emptyAuth`, `http.sslVerify` | any `extraHeader`, `cookieFile`, `sslCert`, `sslKey` or `emptyAuth=true` is set for that destination, or `sslVerify` is `false`; the message names the key, its origin file (`--show-origin`) and that CodeFlow does not reset it |
| 6. Check netrc | the files curl reads: `$HOME/.netrc`, and `_netrc` on Windows; read only to match `machine <host>` or `default` entries against each destination host; never printed, never copied into messages | an entry matches; the message names the file, never its contents |
| 7. Hand the validated helper and policy to the command | one value per command, including the pre-push hook's sync | n/a |

Allowed and left alone: `http.proactiveAuth` (sends the helper's
credential without waiting for a 401; the helper is still the source),
`http.proxy` (authenticates to a proxy, not to the host; a credential in
its URL is the proxy's), the sandbox's `http.proxyAuthMethod`.

Scope note: the preflight checks the sources git reads on this machine.
It cannot see a credential a proxy injects on the way, or a netrc at a
path curl is built to read outside `HOME`; both are named as residuals.

**HTTPS and the test boundary.** Production pushes with a configured
helper require `https://`. The check is one function with an explicit
parameter that allows loopback plain HTTP; production callers pass
`false`, tests pass `true`. There is no environment variable or
configuration switch that disables the check. A loopback HTTP fixture
therefore proves helper and identity behaviour, and the HTTPS rule is
proven by its own unit test; the note discloses that no test pushes over
TLS.

**Where the preflight runs.** "Before any local write" means before the
first write the command makes, which differs per entry:

| Entry | Preflight placement |
|---|---|
| `task new`, `epic new`, `spec new`, `adr new`, `feedback new` | in `Issuer::new` (`cmd/new.rs:585-610`), which every `new` command calls before writing; `feedback new` moves its `write_defaults` call after `Issuer::new` |
| `epic new --integration` | the same `Issuer::new`; `create_integration_branch` receives the resolved value and runs no second preflight |
| `work claim` | at the top of `claim_on` (`readiness.rs:1201`), before `claim_target` and the branch creation |
| `ids sync`, `ids admit`, `ids restore`, `ids seed` | at the top of each `cmd/ids.rs` arm, before the core call |
| `ids retarget` | at the top of `seed::retarget` (`seed.rs:521`), before `drop_pending` at `seed.rs:576` |
| `codeflow git-hook pre-push` | in `sync_pending_ids` (`git_hook.rs:180`), after `has_pending` says there is something to publish and before `issue::sync`; a refusal is a blocking `registry.sync` finding, so an agent's code push stops with the configured route named rather than stranding reservations that a later sync would push under the wrong identity |

Each entry gets a no-mutation refusal test: the preflight refuses and the
test asserts that `project.toml`, the registry refs, the local branches
and pending state are byte-identical afterwards.

**Child hook processes.** CodeFlow's own push runs the repository's
pre-push hook, which runs `codeflow git-hook pre-push`. That nested
process never syncs for a registry push (`git_hook.rs:180-193`), and for a
claim or integration-branch push it resolves the helper from the
authority itself, exactly as a top-level command does; it does not read
the helper string out of its inherited `GIT_CONFIG_PARAMETERS`.

**Proven absence, invalid content, unreadable authority.** The three are
distinct results of step 1 and 2, and only the first ever leads to an
ambient push:

| Result | Meaning | `ambient` | `required` |
|---|---|---|---|
| Proven absent | the default-source refs were read, `project.toml` parsed, and no `[push] credential_helper` exists | push without a helper, as today | refuse: name the key, the ref read, `codeflow doctor --check push-credential` |
| Invalid | the key exists and fails a rule in section 2, or a destination or source check in this section fails | refuse: a configured helper that cannot be used never reverts to the personal identity (AC-2) | refuse |
| Unreadable | the tracking namespace is populated but the default-source refs are missing, dangling or unparsable (`landed_policy.rs:122-160`), or the fallback sources conflict | refuse, naming `git fetch origin` or `git remote set-head origin --auto`; policy and helper live in the same unreadable authority, so neither is known | refuse |
| Unpopulated | no tracking ref exists yet (`landed_policy.rs:58-72`), the state after `git init`, `git remote add` and before any fetch | the policy is read from `HEAD` with the existing residual; no helper is read from `HEAD`; if that policy is `required`, refuse naming `git fetch origin`; otherwise push without a helper and say so | as the previous cell: refuse |

The registry fetch at `issue.rs:380` fetches only `codeflow/registry` and
never refreshes the default branch; step 1's refresh is what keeps the
authority current, and the first fetch of a non-clone repository is the
operator's or agent's `git fetch origin`, which the refusal names. A
stale ref is at worst an older landed value, never a branch's.

### 5. What the guard sanctions and what it still refuses

The guard is a `PreToolUse` hook on the agent's shell line. It never sees
CodeFlow's internal git invocation. So the sanctioned form is the plain
command, which already passes, and no allowance keyed on the CodeFlow
executable's name or the helper string is added to git-guard.

| Line | Verdict | Change in this task |
|---|---|---|
| `codeflow task new ...`, `codeflow work claim ...`, `codeflow ids sync` and the other commands above, with no prefix | allowed; the helper is applied inside the binary | none |
| `GIT_CONFIG_COUNT=...`, `GIT_CONFIG_PARAMETERS=...`, `GIT_CONFIG_GLOBAL=...` on a `codeflow` line | refused, `git.hook_integrity`, the closed rule | none; the closed-rule tests (`security/unresolved.rs:1597` and `:1867`) stay, and AC-3 adds a `codeflow` line to them, since no existing test names one |
| `GIT_CONFIG_*` on `git fetch`, `pull`, `push` | refused, `git.policy_authority` | none |
| writes to `.codeflow/project.toml` or `.codeflow/policy.json` in-session, any tool | refused (integrity files, ADR-0009; edit-guard for native edits) | none |
| user or system `credential.helper` writes | refused (`security/git.rs:124-125` lists the key as a program) | none |
| repository-scope `credential.helper` writes | allowed | none; it is the operator's 3.1.1 route for the agent's own pushes. See the limits below |
| on a transport command (`fetch`, `pull`, `push`, `ls-remote`, remote-helper forms): `-c` or `--config-env` setting `credential.helper`, `credential.<url>.helper`, `core.askPass`, `core.sshCommand` to a non-empty value; the prefixes or earlier exports `GIT_ASKPASS=`, `SSH_ASKPASS=`, `GIT_SSH=`, `GIT_SSH_COMMAND=`; the same through a launcher or wrapper the guard parses | allowed today | new refusal under `git.hook_integrity`, "a command-local program on a transport command", covering the parsed forms the closed rule already reads. An empty value (`-c credential.helper=`) is a reset, introduces no program, and passes; the diagnostic says which of the two it saw. `GIT_SSH` and `GIT_SSH_COMMAND` are the environment forms of `core.sshCommand` and are refused with it |

Limits of the new rule, stated plainly: repository-scope program writes
stay permitted, so an agent can set a local `credential.helper` and then
run a plain `git push`. Refusing the command-local spelling does not close
that route; what it closes is the one-line form an agent reaches for once
the environment route is refused. Whether an agent's plain transport line
is judged by its effective configuration is TSK-283's decision, made with
the shared validator of section 2; this task does not broaden repository
write policy, which would need the operator's decision.

### 6. The opt-in policy (settled)

Operator decision, 2026-10-14: under `git.push_credential = "required"`, a
CodeFlow push with no valid helper refuses everywhere, the operator's own
terminal included.

`.codeflow/policy.json`: `git.push_credential`, enum `ambient | required`,
default `ambient`. `required` dominates: `stricter()`
(`landed_policy.rs:339-368`) merges only level enums, so this key gets its
own merge that keeps `required` when any contributing source says so, and
tests that a branch cannot relax it and that a fallback source can
tighten it.

Under `required`, every entry in the command table refuses before any
local write under the conditions of section 4. The operational consequence
is the operator's: CodeFlow cannot tell an agent session from a terminal,
caller markers are not authentication, so the policy means that
CodeFlow's own pushes in this project use the configured credential
regardless of who runs them. An operator who needs a personal workflow
uses a separately authorized route; there is no environment override.

### 7. Doctor

One check, `push-credential`, beside `policy-source` and `id-registry`
(`doctor/mod.rs:3891`, `3018`). It runs the preflight's read and checks
and reports configuration; it never runs the helper and never claims a
verified bot identity.

| Reports | Status |
|---|---|
| the default source actually read (ref name, fallback or not) and the authority state (read, unreadable with its reason, unpopulated) | pass, or warn with the remedy |
| the helper value, the resolved executable path, whether the pin is set and matches | pass; warn when unset under `ambient`; warn when invalid |
| that `credential.useHttpPath=true`, the askpass reset and `followRedirects=false` are passed with it | information |
| the entries that use it: the eleven CLI commands and the pre-push hook sync | information |
| every effective push destination and each check's result (scheme, userinfo, `http.*` keys with their origin files, netrc match by file name) | pass, or warn naming the failing check |
| `git.push_credential` level and the source it came from | information |
| the working copy's `[push]` differs from the authority | note, as `policy-source` reports drift |
| `ambient` and proven absent | pass: "CodeFlow pushes use the ambient credential" |

### 8. Docs and records

| Where | What |
|---|---|
| `docs/adoption.md`, the bots section | a short "CodeFlow's own pushes" paragraph: the `[push]` table with the pin, the policy key, the executable contract, and for the manual route the three lines with `credential.useHttpPath` |
| `docs/policy-reference.md` | the `git.push_credential` row; the `[push]` table under a project settings heading, since no project settings reference exists yet; the residuals (proxy-injected credentials, netrc outside `HOME`, repository-scope helpers on agent lines) |
| `changelog.d/TSK-291.md` | the setting, the policy key, doctor, the prompt-off change at the claim sites, and an amendment of the 3.1.1 upgrade step 3 to include `credential.useHttpPath` |
| the ADR | section "Is an ADR needed" |

## Failure modes and adversarial cases

| Case | What happens | Residual |
|---|---|---|
| A pull request sets `[push] credential_helper` to its own program | nothing until it lands on the default branch: the branch's copy is never read (AC-3 test). Once landed it has the authority the default branch already has over the in-tree hooks (`.codeflow/git-hooks`) and CI; review sees an absolute path and a pin, and the program must already exist outside every repository path | the default branch is trusted, as it is for hooks and workflows |
| A pull request ships `bin/git-credential-<name>` and the agent prefixes `PATH` | no effect: bare names are refused, the first word is an absolute path, and git is given the resolved path | git itself is found through `PATH` (shared residual) |
| The landed value points into a checkout, through an absolute path or a symlink | refused at push time: the resolved real path lies inside a repository path | none |
| The helper file is replaced by a process running as the operator's user | under `required` the pin mismatch refuses; under `ambient` without a pin the replaced program runs, which doctor warns about | the operator's user owns the machine |
| An interpreter helper with a repository script argument | an argument with `/` is validated as a path; a relative argument naming a file under the repository is refused | an argument the interpreter resolves some other way |
| Shell expansion or chaining in the value | the grammar admits no metacharacters and CodeFlow single-quotes each word | none |
| A change on an integration line | ignored: default sources only | none |
| An agent edits `project.toml` in-session | refused by git-guard and edit-guard; the working copy is not read anyway | none |
| `GIT_CONFIG_*` on a `codeflow` line | refused, unchanged | none |
| `GIT_ASKPASS=/x codeflow task new` | passes the guard today; CodeFlow's push removes the variable and sets `core.askPass=`; the new rule in section 5 refuses the spelling | none |
| A `~/.netrc` entry for the host | the preflight refuses and names the file; without the preflight, curl authenticates from it after the helper answers nothing (loopback experiment 1) | a netrc at a path curl reads outside `HOME` |
| `user:password@` in a push URL or an `insteadOf` rewrite to one | the preflight refuses the destination; without it git authenticates from the URL (loopback experiment 4) | none |
| `http.<url>.extraHeader` with an Authorization header | the preflight refuses and names the origin file; the generic reset does not remove it (loopback experiment 3) | none |
| `http.sslVerify=false` or `GIT_SSL_NO_VERIFY` | the preflight refuses the key; the variable is removed from the child | none |
| An `http://` push URL | refused in production; a test passes the loopback parameter | no test pushes over TLS; disclosed |
| A redirect to another host | the push fails with `followRedirects=false` | a renamed repository needs the operator's URL fix |
| The agent invokes the bot helper directly (`printf ... \| /usr/local/libexec/bot-helper get`) | the helper prints the bot token; this is inherent to any credential helper the agent's shell can run. The native deny list covers the `git credential*` spellings (`actions.json:313-315`), not a direct helper call | the setting keeps the personal credential out of CodeFlow's pushes and makes the identity auditable; it does not hide the bot credential from the agent. The token's scope is the control (ADR-0075 D9): a fine-grained token for this repository without workflow, administration or gist rights |
| A helper that supplies nothing | the push fails with no fallback (experiment E) and is reported as a permission failure naming the helper | none |
| A helper that answers only for a path | gets `path=` (experiment B) | none |
| An ambient `GIT_TRACE_CURL` or `GIT_CURL_VERBOSE` | removed from CodeFlow's push environment, so the Authorization header never reaches the error text | the agent's own git lines are outside this task |
| The child pre-push hook | sees `GIT_CONFIG_PARAMETERS` with the helper entries (hook experiment) and resolves its own helper from the authority; it never syncs for a registry push | the AC-5 journey confirms the hook plane accepts the push |
| The Claude sandbox injects `GIT_CONFIG_PARAMETERS` (`http.proxyAuthMethod=basic` and an empty helper for its localhost proxy) | the helper reset clears the helper list; the proxy method key is untouched and is not a refused key; the preflight reads the same environment | confirm in the AC-5 journey run inside the sandbox |
| `origin` is SSH | the helper is never consulted; a configured helper or `required` refuses the destination (scheme check) | an HTTPS `pushurl` set by the operator; an agent cannot set it, the guard refuses |
| Authority unreadable, or unpopulated with `required` at `HEAD` | refuse and name `git fetch origin` | none |
| Invalid value under `ambient` | refuse; never an ambient push | none |
| An offline reservation synced later | `ambient` with a helper: the sync pushes through the helper; `required`: the reservation was refused up front, and sync refuses under the same conditions | none |
| A plain agent `git push` with pending reservations | the pre-push hook's sync runs the preflight; under `required` with no helper the code push stops with the route named | none |
| An agent sets a repository-scope helper and runs a plain `git push` of its own branch | outside this task; TSK-283 decides whether the effective configuration of that line is judged, with the shared validator | stated in section 5 |
| The operator forgets `credential.useHttpPath` on the manual route | the 3.1.1 note is amended; doctor cannot see the manual route, which is in `.git/config` | a later check could read it |

## Evidence from experiments

All runs on 2026-10-14 with git 2.53.0 in throwaway repositories under
`$TMPDIR`, since removed.

First set. Two recording helpers logged the action and the fields git sent
them; a global file held a plain and a URL-scoped helper, and the
repository a local one. Experiments A and C consulted the machine's real
helpers through `git credential fill`; that is what the rule at the top of
this note now forbids, and they are not to be repeated. They are kept
because they established the leak.

| Experiment | Invocation | Result |
|---|---|---|
| A | `git credential fill` with only the ambient configuration | the operator's personal token was returned by a system-scope keychain helper; the recording helpers were never asked |
| B | `-c credential.helper= -c credential.helper=<bot> -c credential.useHttpPath=true credential fill` | only the bot helper was called, with `protocol`, `host` and `path=org/repo.git` |
| B2 | as B without `useHttpPath` | the bot helper was called without `path` |
| C | `-c credential.helper=<bot>` without the reset | the global, URL-scoped and local ambient helpers were each called before the bot helper |
| D | as B with an ambient `GIT_ASKPASS` and a helper that answers nothing | the askpass program supplied the username and password |
| E | as D with `GIT_ASKPASS` removed, `-c core.askPass=` and `GIT_TERMINAL_PROMPT=0` | `fatal: could not read Username ... terminal prompts disabled`; no fallback |
| Hook | `git -c credential.helper= -c credential.helper=/opt/bot -c credential.useHttpPath=true push` to a local bare remote with a pre-push hook printing its environment | the hook saw `GIT_CONFIG_PARAMETERS` holding the helper entries after the sandbox's own entries |

Second set, under the rule: `env -i` with a temporary `HOME`,
`GIT_CONFIG_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0`, the helper list reset and
one recording helper that answers nothing, against a loopback HTTP server
that answers 401 and logs the `Authorization` and `X-Probe` headers of
each request. `ls-remote` was used because it exercises the same
credential and HTTP path as a push without needing a receiving
repository.

| Experiment | Setup | Result |
|---|---|---|
| 1 | `~/.netrc` names the host; `-c credential.helper= -c credential.helper=<rec> -c credential.useHttpPath=true -c core.askPass=` | the recorder was asked once and answered nothing; the retry carried `Authorization: Basic` with the netrc user |
| 2 | a generic `http.extraHeader` and an `http.<url>.extraHeader` in the temporary global file | the URL-scoped header was sent; the generic one was not (the more specific match wins) |
| 3 | as 2 with `-c http.extraHeader=` | the URL-scoped header was still sent |
| 4 | `http://user:password@127.0.0.1:<port>/...` | the retry carried `Authorization: Basic` with the URL user |
| 5 | none of the above | the recorder was asked once; no `Authorization` header was sent |

Each run saw exactly one helper call. The experiments show the sources,
not a successful push: the server never served a repository.

## Test plan by criterion

Under the rule: synthetic helpers only, reset first, temporary `HOME`,
`GIT_CONFIG_NOSYSTEM=1`, no `credential fill` or `approve`, no real user
or system helper ever consulted.

The fixture for AC-2, AC-5 and AC-6 is a loopback HTTP server in the test
binary that fronts `git http-backend` for a disposable bare repository,
demands Basic authentication, and records the username of every
authenticated request. The helper under test is a script that answers
with the bot username for its path. The three call sites push to it
through CodeFlow, and the test asserts the recorded username, the
helper's recorded `path=`, and that the planted ambient sources (a
global helper, a URL-scoped helper, a repository helper, a `~/.netrc` for
the host in the temporary `HOME`) recorded no call and supplied no
`Authorization`. A separate credential probe is not accepted as evidence
of identity isolation. The loopback fixture is plain HTTP by the test-only
parameter; the HTTPS requirement is proven by its own unit test on the
preflight.

| Criterion | Test |
|---|---|
| AC-2 | one authenticated push per call site (registry push, claim push, integration-branch push) through the fixture, with every planted ambient source present and silent |
| AC-3 | a branch commits a different `[push]` value and a relaxed policy level; its own `task new` pushes through the authority's helper and the policy stays `required`; the closed-rule tests (`security/unresolved.rs:1597`, `:1867`) gain a `GIT_CONFIG_COUNT=... codeflow task new` line; the executable contract tests: a bare name, a relative path, a path inside the checkout, a symlink into the checkout, a shell metacharacter, a pin mismatch, a `PATH` prefix with a planted `git-credential-<name>` |
| AC-4 | doctor tests for each status row in section 7, including the hook entry and each authority state |
| AC-5 | a CLI journey: fresh project, the fixture as `origin`, configured helper and pin at the default branch, `codeflow task new` from a guarded session; the guard allows the line, the fixture recorded the bot username for the registry push, and the hook plane accepted it |
| AC-6 | a helper that answers only when `path=` names the repository; the push succeeds and the fixture recorded the bot username; the adoption text names `useHttpPath` |
| Section 4 | per entry, a no-mutation refusal test under `required` with the key absent, and under both policies with an invalid value, an unreadable authority, an `http://` destination, a userinfo destination, a URL-scoped `extraHeader`, a netrc match, and `sslVerify=false`; a policy test that `required` survives a branch's relaxation |

## Is an ADR needed

Yes, one short record, written in the code pull request. Two Tier-3
triggers apply (`cf-method` "When an ADR is warranted"): a schema change
(a `[push]` table in `project.toml` with the pin, and a `git.push_credential`
key in `policy.json`) and a boundary change (the credential CodeFlow's
pushes present moves from whatever git finds to a validated program named
at the policy authority, with a refusal gate that binds every caller).
Proposed title: "CodeFlow's own pushes take their credential helper from
the landed project settings". The record carries the operator's two
decisions of 2026-10-14 and the trust limits of sections 2 and 4.
`architecture_impact` names the policy authority paragraph of
`docs/architecture/enforcement-planes.md`, which gains one sentence, and
the decision map gets its row. ADR-0009, ADR-0072 and ADR-0075 stay as
they are; this record cites them.

## Operator decisions (settled 2026-10-14)

| Decision | Settled as |
|---|---|
| The source of the helper | `[push] credential_helper` in `.codeflow/project.toml` at the remote default-branch authority |
| Whether `git.push_credential = "required"` binds the operator's own terminal | yes: a CodeFlow push with no valid helper refuses everywhere, the operator's terminal included; no environment override |

A scope choice for the primary, not the operator: the new
`git.hook_integrity` refusal of command-local programs on agent transport
lines (section 5) lands in this task. Recommendation: yes, since it is the
spelling an agent reaches for once the environment route is refused, the
key classification exists, and the rule's limits are stated.

## Review disposition

| Reviewer | Scope | Verdict | Evidence |
|---|---|---|---|
| Codex, independent interactive design review | whole design at `50ea1174fdf2a3b6054f4999a454d0422685da7d` | changes_requested | `target/review-design-TSK-291-codex-02.md`: effective redirect settings, remaining ambient authentication sources, contradictory journey fixtures and unpopulated authority fallback remain unresolved. No implementation approval. |

Codex design review at `2ffa0ad75`, `changes_requested`.

| Finding | Disposition |
|---|---|
| R1, other authentication sources survive the reset | accepted and reproduced (loopback experiments 1, 3, 4). Section 4 resolves every effective push destination and refuses URL credentials, extra headers per destination, netrc matches, cookies, client certificates, `emptyAuth` and disabled TLS verification; the test plan requires an authenticated transport that records the receiving identity, with the planted sources silent |
| R2, trusted text is not a trusted executable | accepted. Section 2 requires an absolute path resolved through symlinks to an executable outside every repository path, a content pin (required under `required`), literal quoted arguments, path-shaped arguments validated as paths, and no bare names; TSK-283 shares the validator instead of string equality; the limits are stated |
| R3, `http://` under `required` | accepted. Section 4 requires `https://` for every effective destination whenever a helper is configured, refuses `sslVerify=false`, removes the TLS override variables, sets `followRedirects=false`, and confines plain HTTP to a test-only parameter with no environment or configuration switch |
| R4, invalid or unreadable settings restore personal authentication | accepted. Section 4's four-way table: an invalid value refuses under both policies, an unreadable authority refuses, only proven absence under `ambient` pushes ambiently; the registry fetch not refreshing the default branch is answered by the preflight's refresh and the named `git fetch origin` |
| R5, hook-triggered pushes and early writes | accepted. The command table separates eleven CLI entries from the pre-push hook sync (`git_hook.rs:162`, `:219`), names the early writes at `feedback.rs:276-285` and `seed.rs:576`, places the preflight per entry, and adds a no-mutation refusal test per entry; the nested hook process resolves its own value |
| R6, the journey permits a bypass | accepted. The file-remote plus probe alternative is withdrawn; the fixture is an authenticating loopback `http-backend` that records the identity during the push itself, under the credential rule |
| Default-branch source | agreed; the merged `project` field is not used for this key |
| New transport-line refusals | agreed; the rule now covers `--config-env`, URL-scoped helper keys, exports and wrappers, `GIT_SSH` and `GIT_SSH_COMMAND`, and distinguishes an empty reset from a program |
| Limits of the new guard rule | agreed and stated in section 5; no broadening of repository write policy |
| Policy ordering | agreed; a dedicated merge and branch-relaxation tests |
| Doctor | agreed; the hook entry, the actual source, authority state, destinations and the secrets note are added |
| TSK-283 coupling | agreed; its worktree's uncommitted file was read for direction only and is not treated as verified |
| ADR | agreed; the decisions are now settled and recorded |
| Nit, "three sources" | fixed: four |
| Nit, ADR-0075 labels | fixed: the token-scope decision is D9 (`ADR-0075:267`); D6 is the operator's removal of the `delete_repo` scope |

## Sources

- Issue 151 and its comments of 2026-10-12 and 2026-10-13.
- `project-management/tasks/TSK-291.md`; `project-management/epics/EPC-022.md` AC-9.
- `target/review-design-TSK-291-codex-01.md` (the Codex design review).
- `CHANGELOG.md:38-47` (3.1.1 upgrade step 3).
- ADR-0009 (integrity files, in-band credentials), ADR-0072 (the registry
  push), ADR-0075 (landed policy authority, D9 token scope);
  `docs/architecture/enforcement-planes.md:141-166`.
- `crates/codeflow-core/src/hooks/landed_policy.rs`, `hooks/git_guard.rs`,
  `security/unresolved.rs`, `security/git.rs`, `ids/issue.rs`, `ids/git.rs`,
  `ids/seed.rs`, `workgraph/readiness.rs`, `workgraph/light_paths.rs`,
  `git/mod.rs`, `hooks/policy_schema.rs`, `doctor/mod.rs`;
  `crates/codeflow-cli/src/cmd/new.rs`, `cmd/feedback.rs`, `cmd/git_hook.rs`.
- gitcredentials(7), git-config(1) and git-remote(1) from git 2.53.0 (the
  helper list, the empty-string reset, `credential.useHttpPath`,
  `http.extraHeader`, `http.followRedirects`, `http.sslVerify` and
  `GIT_SSL_NO_VERIFY`, `get-url --push --all`, `config get --url`).
- The review's git source citations for `http.c` (extra headers, netrc)
  and `credential.c` (complete credentials skip the helpers).
