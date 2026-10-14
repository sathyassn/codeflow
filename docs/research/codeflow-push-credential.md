# CodeFlow's push credential setting

Design note for TSK-291 (issue 151). It settles, before any code, where
the credential helper CodeFlow applies to its own pushes is read from, which
commands push, how the helper is applied, what git-guard sanctions and still
refuses, and whether a missing non-personal credential refuses under an
opt-in policy. Written 2026-10-14 against the EPC-022 line at `1e6e29e93`,
with the installed `codeflow 3.1.0` guard and git 2.53.0 for the live checks.

Where it stands: this note recommends one design and names two operator
decisions. The other lineage reviews it before the first code commit (AC-1).

## Summary

- CodeFlow pushes from twelve commands, all through three call sites
  (`ids/issue.rs:461`, `readiness.rs:1268`, `light_paths.rs:259`).
- The helper is declared in `.codeflow/project.toml` under a `[push]` table
  and read only from the configured remote's default-branch tracking ref,
  through the landed-policy reader that already reads that file there. A
  branch, an integration line, the working copy, the environment and the
  repository's git config never supply it.
- CodeFlow applies it on its own push invocations only, as `-c` arguments
  (`credential.helper=` reset, the helper, `credential.useHttpPath=true`,
  `core.askPass=`) with the askpass and trace variables removed and
  terminal prompts off. Nothing on the agent's command line changes, so the
  guard needs no new allowance and keeps refusing hand-built environments.
- An opt-in policy key `git.push_credential = "required"` makes every
  CodeFlow push refuse before any local write when no valid helper is
  configured at the authority or the push URL is not HTTPS.
- Doctor gains one check, `push-credential`. One short ADR is warranted.
- Operator decisions: the source of the setting (project file at the
  authority, recommended, or the operator's user-scope git config), and
  whether `required` may bind the operator's own terminal (recommended yes).

## The problem at 3.1.1

Live checks with the installed `codeflow 3.1.0` guard in this worktree
(`codeflow hook git-guard` with a Bash payload) and with git 2.53.0 and
recording credential helpers in a throwaway repository:

| Line or situation | Today | Evidence |
|---|---|---|
| `GIT_CONFIG_COUNT=... codeflow task new x` (the adopter's route) | refused, `git.hook_integrity`: the line names a `GIT_CONFIG` variable and `codeflow` is a program the guard does not read | closed rule, `git_guard.rs:664-680`, `security/unresolved.rs:1513` |
| `GIT_CONFIG_PARAMETERS=... codeflow task new x`, `GIT_CONFIG_GLOBAL=... codeflow ...` | refused, same rule | live check |
| `GIT_CONFIG_* git push origin HEAD` | refused, `git.policy_authority`: command-local configuration can redirect the configured remote | `git_guard.rs:6474-6490` |
| `codeflow task new x` with no prefix | allowed; the registry reservation goes out with whatever credential git finds | `ids/git.rs:32-41` sets only `GIT_TERMINAL_PROMPT=0` and `LC_ALL=C` |
| A system or global keychain helper is present (the usual macOS or `gh` setup) | `git credential fill` answers with the operator's personal token before any repository helper is asked; git stops at the first helper that supplies both fields | experiment A below; gitcredentials(7) "each helper will be tried in turn" |
| Repository helper added without the empty reset | the earlier helpers still answer first | experiment C |
| Repository helper that answers only for a path, no `credential.useHttpPath` | the helper is called without `path=` and answers nothing; with the empty reset there is no fallback | experiment B2; the adopter's 2026-10-13 comment |
| `git -c credential.helper='!h' push ...`, `git -c core.askPass=/x push ...`, `GIT_ASKPASS=/x git push ...`, `GIT_ASKPASS=/x codeflow task new x` | allowed by git-guard and exec-guard | live check; `scan_git_globals` reads only `core.hooksPath` (`git_guard.rs:7782-7820`); `transport_config_environment` reads only `GIT_CONFIG*`, `HOME`, `XDG_CONFIG_HOME` (`git_guard.rs:919-926`) |
| `git config --local credential.helper '!h'`, `git config --worktree credential.helper ...` | allowed by both guards in this checkout; repository-scope writes pass by design (`git_guard.rs:12800-12816`). The issue's "refused or flagged" was not reproduced here; it may come from a native deny or another host | live check |
| `git config --local remote.origin.pushurl ...` | refused, `git.policy_authority` | live check |

The 3.1.1 upgrade note (`CHANGELOG.md:38-47`) tells the operator to add a
repository-scope helper after an empty value. That route is correct for
the agent's own `git push` lines and stays available. It omits
`credential.useHttpPath`, it is three hand-written settings on every
clone, and nothing reports it. The adopter has stopped agent CodeFlow
pushes until a project setting exists.

## Which CodeFlow commands push

Every push goes through one of three call sites. No libgit2 push exists in
the crates (no `remote.push` or `PushOptions` use), `codeflow integrate` is
local, and `codeflow remote protect` uses the GitHub API, not git.

| Command | Call site | Path to the push |
|---|---|---|
| `task new`, `epic new`, `spec new`, `adr new`, `feedback new` | `crates/codeflow-core/src/ids/issue.rs:459-461` (`push`) | `cmd/new.rs:616` and `cmd/feedback.rs:292` call `issue::reserve` (`issue.rs:80`); `reserve_locked` publishes pending entries first (`issue.rs:99`) then pushes the reservation (`issue.rs:126`) |
| `ids sync` | same | `issue.rs:590-603` then `publish_pending` (`issue.rs:664`, pushes at `690` and `727`) |
| `ids admit` | same | `issue.rs:858` publishes pending entries before admitting |
| `ids restore` | same | `issue.rs:963` |
| `ids retarget` | same | `seed.rs:588` calls `issue::reserve` for the new number |
| `ids seed` | same | `seed.rs:158` |
| `work claim` | `crates/codeflow-core/src/workgraph/readiness.rs:1268-1280` | `claim_on` pushes `task/<id>-<slug>` create-only with `--force-with-lease=<ref>:` |
| `epic new --integration` | `crates/codeflow-core/src/workgraph/light_paths.rs:259` | `create_integration_branch` pushes `integration/EPC-NNN-<slug>` with `-u` |

The registry site runs git through `ids/git.rs:32-41` (`GIT_TERMINAL_PROMPT=0`,
`LC_ALL=C`, stdin closed). The claim and integration-branch sites run
`crate::git::command()` (`git/mod.rs:41-59`) with no environment of their
own, so a missing credential there can hang on a terminal prompt. All three
build on `crate::git::command()`, which is where one shared push builder
fits.

Fetches and `ls-remote` calls (`issue.rs:380`, `remote_query.rs:28`,
`readiness.rs` fetches) stay on the ambient credential in this design: a
read carries no authorship, the registry push is what the host records
under an identity, and AC-2 and AC-6 speak of pushes. Extending the helper
to every CodeFlow transport call is a possible later setting; it is not
needed to close issue 151.

## Design

### 1. Where the setting is read from

Requirement (AC-1, EPC-022 AC-9): a source a pull request cannot change for
its own pushes. Three sources were compared.

| Option | Where | A PR can change it for its own pushes? | An agent can change it in-session? | Reported by doctor? | Cost |
|---|---|---|---|---|---|
| A. `.codeflow/project.toml` `[push]` at the configured remote's default branch (recommended) | the tracking ref `refs/remotes/origin/HEAD` (or `main`/`master` fallback), read as git data by `hooks/landed_policy.rs:219-229` | no: the branch's own copy is never read; the value takes effect once landed on the default branch, the same way policy levels do (ADR-0075 D5 amendment; `enforcement-planes.md:141-150`) | no: the file is an integrity path git-guard and edit-guard refuse to write (ADR-0009), and the working copy is not read anyway | yes, from the same ref as `policy-source` | one key in a file every adopter already has; a fetch is needed before the first push, which the registry flow already does |
| B. the operator's user-scope git config, `git config --global codeflow.pushCredentialHelper` | `~/.gitconfig` or `~/.config/git/config` | no | no: git-guard refuses user-scope writes of keys not known safe (`git_guard.rs:12755-12800`), and `GIT_CONFIG_GLOBAL` on a `codeflow` line is refused; `HOME=` on a `codeflow` line is not (live check) | yes | per machine and per operator; CI runners and every contributor repeat the setup; the project cannot declare that it requires a bot identity |
| C. the repository's git config (the 3.1.1 route) | `.git/config` | no | yes: repository-scope `credential.helper` writes pass both guards (live check) | not today | already documented; keeps working for the agent's own `git push` lines |
| D. an environment variable | the session | n/a | yes, in band | no | rejected: ADR-0009's reasoning, an agent shares every in-band credential |

Recommendation: A. It is the one source that is project-declared, PR-proof,
agent-proof and already read at the authority by the reader every guard
and doctor use. C stays as documented for the agent's own pushes and is
unchanged by this task. B is the alternative if the operator prefers the
helper to be a per-machine secret rather than a value in the repository;
see "Operator decisions".

Two narrowings of the existing reader apply to this key:

- Only the default sources contribute. `landed_policy::load` lets a declared
  target line's `project.toml` win over the default's
  (`landed_policy.rs:109`, `line_project.or(project)`). Integration lines are
  landed by the primary agent, so a line must never pick the program
  CodeFlow runs. The helper is read from `defaults` only
  (`landed_policy.rs:76-91`); with the `main` plus `master` fallback, a
  conflict between them refuses as it does for project settings today
  (`landed_policy.rs:87-89`).
- Never the working copy or `HEAD`. When the tracking ref cannot be read
  (no remote, no fetch yet, or an unparsable file), the key is treated as
  absent. Under `ambient` the push proceeds without a helper and prints
  one line saying so; under `required` it refuses. The reader must not
  fall back to `read_project_toml(root)` for this key as `working()` does
  for policy (`landed_policy.rs:162-170`), or a branch could set the helper
  for its own pushes in a fresh clone before the first fetch.

A no-remote repository never pushes (`issue.rs:91`, `light_paths.rs:255`,
`readiness.rs:1211`), so the setting is moot there.

### 2. The setting's shape

```toml
[push]
credential_helper = "/usr/local/bin/codeflow-bot-credential --org acme"
```

| Rule | Why |
|---|---|
| One string, the helper as git's `credential.helper` would take it: an absolute path, or a bare name that git prefixes with `git credential-`, with optional arguments parsed by the shell (gitcredentials(7)) | the adopter's helper fits; doctor can name the program |
| A value beginning with `!` is refused by the reader and by doctor | a shell snippet on the default branch is unreadable to a reviewer and to doctor; an absolute path with arguments covers the same need |
| A relative path is refused | it would resolve inside the repository, where a pull request could change the program; the helper lives on the machine, outside the tree |
| Empty is refused | an empty value is git's reset spelling; "unset" is the absence of the key |
| No second key for `useHttpPath` | CodeFlow always passes `credential.useHttpPath=true` with the helper (section 3); a host-scoped helper ignores `path=`, so there is nothing to opt out of |

Validation runs where the key is read and in doctor. An invalid value under
`ambient` is reported and the push runs without a helper, never with a
malformed one; under `required` it refuses.

### 3. How the helper is applied

One shared builder, provisional name `push_credential::apply`, used by the
three call sites and by nothing else. For a push with a configured helper
`<h>` the invocation is:

```
git -C <root> \
  -c credential.helper= \
  -c credential.helper=<h> \
  -c credential.useHttpPath=true \
  -c core.askPass= \
  push ...
```

with this environment on the child: `GIT_TERMINAL_PROMPT=0`; `GIT_ASKPASS`,
`SSH_ASKPASS`, `GIT_TRACE`, `GIT_TRACE_CURL`, `GIT_TRACE_CURL_NO_DATA` and
`GIT_CURL_VERBOSE` removed; everything else inherited. Without a configured
helper the builder adds nothing, so the `ambient` behaviour is today's.

| Element | What it does | Evidence |
|---|---|---|
| `-c credential.helper=` | resets the helper list, dropping system, global, URL-scoped and repository helpers, and any `GIT_CONFIG_PARAMETERS` entries inherited from the environment, since `-c` values are appended after them | gitcredentials(7) "configured to the empty string, this resets the helper list"; experiment B vs C |
| `-c credential.helper=<h>` | the only helper git asks | experiment B |
| `-c credential.useHttpPath=true` | the helper receives `path=<org>/<repo>.git`, so a helper scoped by repository can answer; a host-scoped helper ignores it | git-config(1) `credential.useHttpPath`; experiments B and B2 |
| `-c core.askPass=` plus the removed askpass variables and `GIT_TERMINAL_PROMPT=0` | when the helper supplies nothing, git fails instead of falling back to an ambient askpass program or a prompt | experiment D (an ambient `GIT_ASKPASS` answered after the helper gave nothing) and E (fatal, no fallback) |
| removed trace variables | git's curl trace prints request headers to stderr, and CodeFlow folds stderr into its error text (`issue.rs:465-470`, `readiness.rs:1279`), which can land in a terminal or a record | the Authorization header carries the token |
| arguments, not `GIT_CONFIG_*` environment | nothing on the agent's line changes, so AC-3's refusal of hand-built environments stays meaningful; the child hooks still see the helper name through `GIT_CONFIG_PARAMETERS`, which git exports for `-c` values, and never the secret | hook experiment below |

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
- The helper is resolved once per command from the authority and threaded
  to the push, so one command never pushes under two identities.

### 4. What the guard sanctions and what it still refuses

The guard is a `PreToolUse` hook on the agent's shell line. It never sees
CodeFlow's internal git invocation. So the sanctioned form is the plain
command, which already passes, and no allowance keyed on the helper value
is needed in git-guard. That keeps the guard free of a second reader of the
project file.

| Line | Verdict | Change in this task |
|---|---|---|
| `codeflow task new ...`, `codeflow work claim ...`, `codeflow ids sync` and the other commands above, with no prefix | allowed; the helper is applied inside the binary | none |
| `GIT_CONFIG_COUNT=...`, `GIT_CONFIG_PARAMETERS=...`, `GIT_CONFIG_GLOBAL=...` on a `codeflow` line | refused, `git.hook_integrity`, the closed rule | none; the closed-rule tests (`security/unresolved.rs:1597` and `:1867`) stay, and AC-3 adds a `codeflow` line to them, since no existing test names one |
| `GIT_CONFIG_*` on `git fetch`, `pull`, `push` | refused, `git.policy_authority` | none |
| writes to `.codeflow/project.toml` or `.codeflow/policy.json` in-session, any tool | refused (integrity files, ADR-0009; edit-guard for native edits) | none |
| user or system `credential.helper` writes | refused (`security/git.rs:124-125` lists the key as a program) | none |
| repository-scope `credential.helper` writes | allowed | none; it is the operator's 3.1.1 route for the agent's own pushes, and repository scope is not an elevation for a program the agent could run directly |
| `git -c credential.helper=...`, `-c credential.<url>.helper=...`, `-c core.askPass=...`, `-c core.sshCommand=...` and `--config-env` forms on a transport command; `GIT_ASKPASS=`, `SSH_ASKPASS=` prefixes on a transport or `codeflow` line | allowed today | recommended new refusal under `git.hook_integrity`: "a command-local program on a transport command". These are the spellings an agent reaches for once the environment route is refused, and the same key classification already exists for config writes. Scope choice for the primary; it adds no process. `GIT_SSH_COMMAND` was not tested |

TSK-283's shared reader (`security/git_config.rs` in its worktree, which
reads the effective configuration of an agent's git line with
`git config list` under the command's globals and environment) judges only
agent lines. When it meets a repository-scope `credential.helper` whose
value equals the helper configured at the authority, it treats it as the
sanctioned program, not an unknown one; `credential.useHttpPath` is a
boolean switch beside it. CodeFlow's own push never passes through that
reader.

### 5. The opt-in policy

`.codeflow/policy.json`: `git.push_credential`, enum `ambient | required`,
default `ambient`. Read through the landed-policy reader like every other
key; `required` is the stricter value, so `stricter()`
(`landed_policy.rs:339`) must treat this enum as ordered or the key is
merged by hand, otherwise a line could not tighten it and a fallback
source could not either.

Under `required`, every push in the command table refuses before any local
write when any of these holds:

| Condition | Message names |
|---|---|
| no `[push] credential_helper` at the authority | the key, the ref read, `codeflow doctor --check push-credential` |
| the authority cannot be read (before the first fetch, or unparsable) | the ref and `git fetch origin` |
| the value is invalid (section 2) | the rule it breaks |
| the push URL of `origin` is not `http` or `https` | that a credential helper is never consulted for SSH, so CodeFlow cannot show the identity is non-personal; the operator sets an HTTPS `remote.origin.pushurl` (an agent cannot, the guard refuses it) |

"Before any local write" matters for the registry: today an offline
reservation is committed locally as pending and `ids sync` pushes it
later. Under `required`, `task new` refuses up front rather than leaving a
pending entry that a later sync would push, and `ids sync` refuses under
the same conditions.

CodeFlow cannot tell an agent session from the operator's terminal. The
refusal therefore binds every CodeFlow push in the project, the operator's
included, which is what "never a person's credential on the registry"
means. No override variable is added; the operator changes the policy
level through the ordinary landed route.

### 6. Doctor

One check, `push-credential`, beside `policy-source` and `id-registry`
(`doctor/mod.rs:3891`, `3018`):

| Reports | Status |
|---|---|
| the ref the setting was read from, the helper value, and that `credential.useHttpPath=true` is passed with it | pass |
| the commands that use it (the table above) | information |
| the `origin` push URL scheme; under SSH, that the helper is not consulted | note under `ambient`, warn under `required` |
| `git.push_credential` level | information |
| `required` with no helper, an invalid helper, or an unreadable authority | warn with the remedy that names the key |
| the working copy's `[push]` differs from the authority | note, as `policy-source` reports drift |
| `ambient` and no helper | pass: "CodeFlow pushes use the ambient credential" |

Doctor does not run the helper; it reports what would be passed.

### 7. Docs and records

| Where | What |
|---|---|
| `docs/adoption.md`, the bots section | a short "CodeFlow's own pushes" paragraph: the `[push]` table, the policy key, and for the manual route the three lines with `credential.useHttpPath` |
| `docs/policy-reference.md` | the `git.push_credential` row; the `[push]` table under a project settings heading, since no project settings reference exists yet |
| `changelog.d/TSK-291.md` | the setting, the policy key, doctor, the prompt-off change at the claim sites, and an amendment of the 3.1.1 upgrade step 3 to include `credential.useHttpPath` |
| the ADR | section "Is an ADR needed" |

## Failure modes and adversarial cases

| Case | What happens | Residual |
|---|---|---|
| A pull request sets `[push] credential_helper` to its own program | nothing until it lands on the default branch: the branch's copy is never read (AC-3 test). Once landed it has the authority the default branch already has over the in-tree hooks (`.codeflow/git-hooks`) and CI, which also run programs on every machine; review sees an absolute path, and `!` is refused | the default branch is trusted, as it is for hooks and workflows |
| A change on an integration line | ignored: default sources only | none |
| An agent edits `project.toml` in-session | refused by git-guard and edit-guard; the working copy is not read anyway | none |
| `GIT_CONFIG_*` on a `codeflow` line | refused, unchanged | none |
| `GIT_ASKPASS=/x codeflow task new` | passes the guard today; CodeFlow's push removes the variable and sets `core.askPass=`, so the helper stays the only source | the guard refusal in section 4 closes the spelling itself |
| `PATH=/x:$PATH codeflow task new` with a bare helper name | passes the guard; git would run `git-credential-<name>` from that `PATH` | an absolute path avoids it, doctor shows the value; an agent that can prefix `PATH` can run any program directly, so this is not an elevation |
| The agent invokes the bot helper directly (`printf ... \| /usr/local/bin/bot-helper get`) | the helper prints the bot token; this is inherent to any credential helper the agent's shell can run. The native deny list covers the `git credential*` spellings (`actions.json:313-315`), not a direct helper call | the setting keeps the operator's personal credential out of CodeFlow's pushes and makes the identity auditable; it does not hide the bot credential from the agent. The token's scope is the control (ADR-0075 D6): a push-only token for this repository without workflow rights |
| A helper that supplies nothing | the push fails with no fallback (experiment E) and is reported as a permission failure naming the helper | none |
| A helper that answers only for a path | gets `path=` (experiment B) | none |
| An ambient `GIT_TRACE_CURL` or `GIT_CURL_VERBOSE` | removed from CodeFlow's push environment, so the Authorization header never reaches the error text | the agent's own git lines are outside this task |
| The child pre-push hook | sees `GIT_CONFIG_PARAMETERS` with the helper entries (hook experiment); the hook runs `codeflow` git-hook checks, which read `GIT_CONFIG` only (`git_guard.rs:6633`) | the AC-5 journey confirms the hook plane accepts the push |
| The Claude sandbox injects `GIT_CONFIG_PARAMETERS` (`http.proxyAuthMethod=basic` and an empty helper for its localhost proxy) | the reset clears the helper list, the proxy method key is untouched; proxy authentication does not use credential helpers | confirm in the AC-5 journey run inside the sandbox |
| `origin` is SSH | the helper is never consulted; `ambient` notes it in doctor, `required` refuses | an HTTPS `pushurl` set by the operator |
| Authority unreadable before the first fetch in a fresh clone | `ambient`: push without a helper and say so; `required`: refuse and name `git fetch origin` | none |
| An offline reservation synced later | `ambient`: as today; `required`: refused up front and at sync | none |
| The operator forgets `credential.useHttpPath` on the manual route | the 3.1.1 note is amended; doctor cannot see the manual route, which is in `.git/config` | a later check could read it |

## Evidence from experiments

Run on 2026-10-14 with git 2.53.0 in a throwaway repository under
`$TMPDIR`, since removed. Two recording helpers logged the action and the
fields git sent them; a global file held a plain and a URL-scoped helper,
and the repository a local one. Letters match the references above.

| Experiment | Invocation | Result |
|---|---|---|
| A | `git credential fill` with only the ambient configuration | the operator's personal token was returned by a system-scope keychain helper; the recording helpers were never asked |
| B | `-c credential.helper= -c credential.helper=<bot> -c credential.useHttpPath=true credential fill` | only the bot helper was called, with `protocol`, `host` and `path=org/repo.git` |
| B2 | as B without `useHttpPath` | the bot helper was called without `path` |
| C | `-c credential.helper=<bot>` without the reset | the global, URL-scoped and local ambient helpers were each called before the bot helper |
| D | as B with an ambient `GIT_ASKPASS` and a helper that answers nothing | the askpass program supplied the username and password |
| E | as D with `GIT_ASKPASS` removed, `-c core.askPass=` and `GIT_TERMINAL_PROMPT=0` | `fatal: could not read Username ... terminal prompts disabled`; no fallback |
| Hook | `git -c credential.helper= -c credential.helper=/opt/bot -c credential.useHttpPath=true push` to a local bare remote with a pre-push hook printing its environment | the hook saw `GIT_CONFIG_PARAMETERS` holding the helper entries after the sandbox's own entries |

Experiment A reproduced the leak issue 151 describes on this machine: the
personal credential answered because a system-scope helper is asked
first. It is the reason the reset is part of every CodeFlow push.

## Test plan by criterion

| Criterion | Test |
|---|---|
| AC-2 | one test per call site (registry push, claim push, integration-branch push) against a local HTTP remote or a `credential fill` probe, with a recording helper configured at the authority and a planted ambient helper at global, URL and repository scope that must record no call |
| AC-3 | a branch commits a different `[push]` value and a different policy level; its own `task new` pushes through the authority's helper; the closed-rule tests (`security/unresolved.rs:1597`, `:1867`) gain a `GIT_CONFIG_COUNT=... codeflow task new` line, which today is covered only by the live check above |
| AC-4 | doctor tests for each status row in section 6 |
| AC-5 | a CLI journey: fresh project, local bare remote reached over a local HTTP transport or a path with the helper probe, configured helper, `codeflow task new` from a guarded session; the guard allows the line and the recording helper answered |
| AC-6 | a recording helper that answers only when `path=` names the repository; the push succeeds; the adoption text names `useHttpPath` |

## Is an ADR needed

Yes, one short record, written in the code pull request after this note's
review. Two Tier-3 triggers apply (`cf-method` "When an ADR is warranted"):
a schema change (a `[push]` table in `project.toml` and a `git.push_credential`
key in `policy.json`) and a boundary change (the credential CodeFlow's
pushes present moves from whatever git finds to a project setting read at
the policy authority, with a refusal gate). Proposed title: "CodeFlow's
own pushes take their credential helper from the landed project settings".
`architecture_impact` names the policy authority paragraph of
`docs/architecture/enforcement-planes.md`, which gains one sentence, and
the decision map gets its row. ADR-0009, ADR-0072 and ADR-0075 stay as
they are; this record cites them.

## Operator decisions

| Decision | Recommendation | Why it is the operator's |
|---|---|---|
| The source of the helper: the project file at the default-branch authority (A) or the operator's user-scope git config (B) | A | it decides who may change which program runs during a push: the people who can land on the default branch (A) or each machine's operator (B) |
| Whether `git.push_credential = "required"` may refuse the operator's own terminal as well as agents | yes; CodeFlow cannot tell callers apart, and an agent-only refusal would be a claim it cannot prove | it binds the operator's own workflow |

A scope choice for the primary, not the operator: whether the new
`git.hook_integrity` refusal of `-c credential.helper=`, `core.askPass=`,
`core.sshCommand=` and the askpass variables on agent transport lines
(section 4) lands in this task or is tracked once. Recommendation: in this
task, since it is the spelling an agent reaches for when the environment
route is refused, and the key classification already exists.

## Sources

- Issue 151 and its comments of 2026-10-12 and 2026-10-13.
- `project-management/tasks/TSK-291.md`; `project-management/epics/EPC-022.md` AC-9.
- `CHANGELOG.md:38-47` (3.1.1 upgrade step 3).
- ADR-0009 (integrity files, in-band credentials), ADR-0072 (the registry
  push), ADR-0075 (landed policy authority, D6 token scope);
  `docs/architecture/enforcement-planes.md:141-150`.
- `crates/codeflow-core/src/hooks/landed_policy.rs`, `hooks/git_guard.rs`,
  `security/unresolved.rs`, `security/git.rs`, `ids/issue.rs`, `ids/git.rs`,
  `ids/seed.rs`, `workgraph/readiness.rs`, `workgraph/light_paths.rs`,
  `git/mod.rs`, `hooks/policy_schema.rs`, `doctor/mod.rs`.
- gitcredentials(7) and git-config(1) from git 2.53.0 (the helper list,
  the empty-string reset, `credential.useHttpPath`, the askpass order).
