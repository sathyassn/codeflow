# Harness posture

<!-- HOW layer. Split out of docs/adoption.md: the runtime posture each harness
     gets. Adoption keeps install, tiers, ownership, and the daily loop; the
     planes that block, and how far each reaches, live in
     docs/architecture/enforcement-planes.md. -->

## Concept

**Autonomy is fail-closed by default and widened only by the operator.** Each
supported harness ships with a preset that grants the access ordinary work
needs and refuses the rest; widening it is an operator act, and the enforcement
floor stays underneath either way.

The starters are executable policy, not prompt-only guidance: the preset is
enabled in runtime settings as well as described in the skills (architecture
decision record ADR-0025). Approval policy and sandbox authority are separate
controls ([Codex security](https://learn.chatgpt.com/docs/security)).

## Architecture

Each harness reads its own configuration files, and all three hand the same
payload to the same two guards. Each table row is the boundary for one harness.
The middle column is what the scaffold turns on, and the right column is what
an operator still has to decide.

| Harness | What the shipped preset enables | What stays gated |
|---|---|---|
| Claude Code | Fail-closed OS sandbox on macOS, Linux and WSL2, sandbox-contained Bash with raw model and cloud credentials removed, web search/fetch, sandbox egress limited to the development hosts listed below (TSK-1070), local port binding for dev/UI tests, ask rules for `gh api` writes, `Edit` denies and sandbox write denies for the shell startup files; a status line showing model, effort, version, output style, folder, branch, worktree, pull request, context use and plan usage, and no `effortLevel` (ADR-0087); cross-family seats pass the current ensemble's model and effort explicitly | Private, link-local, and internal-name destinations; destructive, privileged, publish, and secret-read boundaries, including the token stores of `gh`, git, Docker, gcloud, Azure, npm and the macOS keychains; no unsandboxed retry (`allowUnsandboxedCommands: false`), so a command the sandbox refuses fails in every mode, and only the commands that need host state run outside it by name (`excludedCommands`: the `gh` subcommands CodeFlow uses and a plain `git push`), still judged by the guards and the deny rules; auto and bypass mode and classifier policy are never taken from the repository, so the mode is the person's (ADR-0026, ADR-0029, ADR-0093) |
| Codex | Guarded workspace permission profile (no `sandbox_mode` key), live search, reviewer-subagent escalation review, workspace key-file denies, the current primary seat's configured fallback, the default secret-bearing environment filter, `approval_policy = "never"`, no `model_reasoning_effort` (effort is each person's, ADR-0087), shell startup files read only; a builder seat runs the `cf-builder` profile and a reviewer the `cf-guard` profile, so no seat runs without the OS sandbox (ADR-0075 D1 as amended on 2026-10-05) | Catastrophic work still stops for the operator; git-guard, exec-guard, git hooks, and CI remain the floor; `never` grants no access of its own, so operations outside the effective sandbox fail instead of asking |
| Grok Build | Project hooks wired in `.grok/hooks/codeflow.json`, the same `git-guard` and `exec-guard` payload; Grok's `workspace` sandbox leaves the home, and so its shell startup files, unwritable | The hooks load only after the one-time `/hooks-trust` (or `--trust`); the doctor check reports structural wiring, not trust state (ADR-0054) |

Commands outside the Claude Code sandbox work as follows (ADR-0093, which
amends ADR-0029's retry).

- The shipped presets set `sandbox.allowUnsandboxedCommands: false`. Claude
  Code then ignores a request to rerun a refused command outside the
  sandbox, so a refusal holds in every permission mode, bypass included.
- `sandbox.excludedCommands` names the commands that need host state.
  `gh` reads its token from `~/.config/gh`, which the sandbox denies, and
  fails TLS verification under the macOS sandbox, so the subcommands
  CodeFlow uses run outside it: `gh pr` view, list, checks, diff, status,
  create, comment, edit, ready, review and merge; `gh run` view, list,
  watch, rerun and cancel; `gh issue` view, list, status, create, comment,
  edit, close and reopen; `gh api`, `gh repo view`, `gh release view`,
  `gh release list`, `gh search` and `gh auth status`. `gh run download`,
  which extracts an archive, and `gh pr checkout` and `gh issue develop`,
  which run git, stay sandboxed (TSK-1070 security review). The excluded
  verbs still act on this machine: `gh pr create` can push a branch and,
  with `--editor`, start the editor; `gh pr merge --delete-branch` deletes
  the local branch. The git that `gh` starts is not judged by git-guard.
  `git push *` covers a plain push, which asks `gh` for the HTTPS
  credential and runs the pre-push hook.
- An entry matches only when every command in the call matches it. A pipe,
  a file redirect, a `cd`, `git -C <path>`, an environment prefix or a
  `$(...)` keeps the whole call sandboxed (Claude Code sandboxing docs and
  the TSK-1070 canary). So an agent pushes with a plain `git push` from the
  session's working directory, which git-guard resolves as the repository,
  and filters `gh` output with `--jq`. A script that calls `git` or `gh`
  stays sandboxed too, and an App token passed as `GH_TOKEN=$(...) gh ...`
  never matches an entry.
- The guards, the deny rules and the ask rules judge an excluded command
  like any other. The `forge_exec` denies refuse the `gh` and push forms
  that would run code outside the sandbox, print the token or reach
  another host: extensions, aliases, `gh config set`, `gh auth status
  --show-token`, codespaces, keys, secrets, variables, `gh repo clone`,
  `gh browse`, `--receive-pack`, `--exec` and `ext::` remotes.
- A list of spellings never closes, and neither does a model of gh's
  option grammar (TSK-1070 security review round 3), so exec-guard judges
  `gh api` and `gh auth` against one narrow allowlist and asks or refuses
  everything else:
  - A `gh api` call runs without a prompt only in the plain read form:
    `api` is the first word after `gh`, one endpoint that is not GraphQL
    or an absolute URL, an explicit method only as `-X GET`, `--method
    GET` or `--method=GET`, and otherwise only `-q`/`--jq`,
    `-t`/`--template`, `--cache`, `-i`/`--include`, `--paginate`,
    `--slurp` and `--silent`, each as a separate word or a long `=` form.
  - Every other `gh api` form gets a prompt (the `ask` decision, which
    Claude Code shows in bypass mode too): a write method, a field or
    input body (with `-X GET` too), any header, `--help`, `--input`,
    `--hostname`, GraphQL, an absolute URL, an attached or bundled short
    option, an option before `api`, and any option outside the list.
  - A `gh api` call with a word naming DELETE anywhere (the method word,
    an option with it attached, or a header value, in any case) is
    refused, and so is a `gh auth` call with the token flag anywhere
    (`--show-token` or a short option word holding `t`) or with an option
    before its subcommand.
  - A word the shell fills in is refused, in a `gh api` or `gh auth` call
    or as the `gh` subcommand.
  - The `forge_write` ask rules and the DELETE denies in the presets name
    the common spellings as a second layer.
- Excluded commands do not inherit the host list: an excluded `gh` or
  `git push` reaches whatever host it names. The prompt covers `gh api`
  with `--hostname` or an absolute URL; `gh` with `GH_HOST` or a `-R
  host/owner/repo` and a push to a remote URL on another host have no
  rule of their own, so such a call can carry data off the host list;
  `gh` sends a token only to a host it holds one for.
  A push runs the pre-push hooks in `.codeflow/git-hooks` outside the
  sandbox, so the sandbox denies writes to that folder in any checkout
  (`**/.codeflow/git-hooks`; on Linux the sandbox may need explicit
  paths). The Edit tool's deny rule covers the project's own folder, and
  edit-guard refuses an edit to it in any checkout, worktrees included. A
  `core.hooksPath` other than `.codeflow/git-hooks` has no such deny;
  git-guard refuses changing it. The sandbox also removes `GH_TOKEN`,
  `GITHUB_TOKEN`, `NPM_TOKEN` and `NODE_AUTH_TOKEN` from every sandboxed
  command.
- Herdr's socket and the official Codex plugin route (ADR-0029) also need
  host state, but no shipped preset excludes them: an entry there reaches
  every adopter. A person who uses them adds `herdr *`, with denies for
  its update, machine, server and config commands, or the plugin's
  command, in their own user settings or a `--settings` file
  (`/cf-customize`). A project that needs another tool, or `git fetch` on
  a private remote whose credential helper reads a denied key file, adds
  it to `.claude/settings.local.json`; the lists merge across files.
  Never exclude a script or interpreter under a path sandboxed commands
  can write.

Sandboxed commands reach only these development hosts
(`sandbox.network.allowedDomains`, operator decision 2026-10-31, which
replaces ADR-0025's `*`):

| Hosts | Used by |
|---|---|
| `github.com`, `api.github.com`, `codeload.github.com`, `*.githubusercontent.com` | git fetch and clone, release downloads such as the CodeFlow installer, raw files and source archives |
| `crates.io`, `index.crates.io`, `static.crates.io`, `static.rust-lang.org` | cargo and rustup (the Rust stack and CodeFlow's own gates) |
| `registry.npmjs.org` | npm and pnpm (the node stack, CodeFlow's docs portal and present web gates) |
| `pypi.org`, `files.pythonhosted.org` | pip (the python stack) |
| `proxy.golang.org`, `sum.golang.org` | Go modules (the go stack's `go test` and `go vet`) |
| `cdn.playwright.dev`, `playwright.download.prss.microsoft.com` | Playwright browser downloads for the docs portal and present browser checks |
| `code.claude.com`, `docs.anthropic.com` | Claude Code and Anthropic documentation fetched from the shell |

- The private-range, link-local and internal-name `deniedDomains` and
  `allowLocalBinding` are unchanged. Web search and `WebFetch` follow
  their permission rules, not this list. The Python and Go hosts serve
  adopters on those stacks; CodeFlow's own gates use the GitHub, Rust, npm
  and Playwright hosts.
- The private-range denies match host names only. A connection to an IP
  literal such as `10.255.255.1` is not refused by `10.*` (sandbox
  exemptions research, live probe 2026-10-31), so they do not stop a
  command that names an address directly.
- `allowLocalBinding` lets a sandboxed command listen on and reach local
  ports, which dev servers and UI tests need. It also reaches every other
  local listener, such as the roughly 30 ports local Codex app-servers
  hold; whether those require authentication is not verified.
- A connection to an unlisted host prompts in Manual and acceptEdits,
  is refused in auto unless the classifier approves it, and is allowed
  in bypass unless `sandbox.network.strictAllowlist` is `true` in the
  user file or a `--settings` file; a project file cannot set it, and
  under it Claude Code ignores the project's list, so that same file
  lists the hosts (Claude Code sandboxing docs, checked 2026-10-31, and
  the TSK-1070 canary).
- An adopter adds a host in `.claude/settings.local.json` (personal) or
  `.claude/settings.json` (shared), for example
  `{"sandbox": {"network": {"allowedDomains": ["registry.example.com"]}}}`.
  `codeflow update` keeps every host the project added and removes only
  a shipped entry it retired, such as `*`.

The catastrophic classifier is a non-relaxable floor, not a complete endpoint
security product. Managed organization policy, least-privilege host accounts,
verified backups, and authenticated human approval remain necessary at higher
blast radii. Per-platform behavior, the exact configuration keys, and the
credential rules are in Technical.

## Technical

The platform matrix, then the permission profiles, then credentials, then the
dated evidence.

### Platform assurance

- CodeFlow releases target macOS, Linux, and x86-64 native Windows. 3.0.0 was
  the one release without native Windows, which returns in 3.1.0.
- Windows users can also run the Linux build inside WSL2. This is the preferred
  route for a Linux-native toolchain or Claude work that needs OS-enforced
  sandboxing.

| Platform | Sandbox | What it means for the work |
|---|---|---|
| macOS | each harness's native sandbox | the default posture above applies as written |
| Linux, WSL2 | the harness Linux sandbox implementations | install the dependencies reported by the harness and keep fail-closed startup enabled; WSL2 follows the Linux path |
| Native Windows, Codex | its elevated Windows sandbox, selected by default | the scaffolded deterministic guard recognizes both Bash and PowerShell command events and Windows drive, system, profile, disk, recovery, and permission operations |
| Native Windows, Claude Code | none; Git Bash or PowerShell work, but Claude's OS sandbox is not available | settings and hooks still apply, but they are not equivalent containment: move catastrophic or otherwise high-blast-radius work to WSL2 or a container, and if that boundary is unavailable, stop (ADR-0033) |

The test runner's per-target `shell` is `auto` by default (`sh` on
macOS/Linux/WSL2, `cmd.exe` on native Windows). This keeps generated `&&`
command chains compatible with the shells built into each OS. Set it to `sh`,
`powershell`, or `cmd` only when the consuming project's toolchain requires a
specific shell. `/cf-customize` must canary the selected shell and commands.

### Gate evidence in a Claude session

The shipped Claude presets let a sandboxed session write
`~/.codeflow/locks`, so `codeflow reserve` and quick-mode heavy targets can
take their locks. No preset grants `~/.codeflow/gate-runs`: the operator
decided on 2026-10-30 to drop that grant and add no local recorder
(TSK-1044 AC-4).

- **Full gates run outside the sandbox.** A full gate in a sandboxed
  session fails its evidence preflight, so the primary runs it in the
  operator's terminal; the shipped sandbox does not rerun it outside the
  sandbox. Only the primary
  runs full gates. `codeflow doctor --check permissions` reports the
  evidence folder as unwritable by design inside the sandbox.
- **Writable reports earn no credit.** Any process of the user can still
  write `gate-runs`, the candidate's tests included, so no run record
  removes a target from any run (ADR-0089).
- **Writes never follow a planted link.** The preflight, the lock files,
  the run record, each target's logs and report copies, the stale-report
  cleanup, the producer-output check and the durable copy refuse a link,
  junction or planted name at the build-output folder, the CodeFlow home
  or anything beneath them, so a session that plants one cannot redirect
  a later gate's writes, even one run outside the sandbox. The project
  folder, the parent of an absolute `CARGO_TARGET_DIR` and the parent of
  the CodeFlow home are trusted as roots; a session that cannot write
  those parents cannot replace them.

### Codex permission profile

The `cf-guard` permission profile in `.codex/config.toml` is selected via
`default_permissions` and extends `:workspace`.

- It denies the home-directory secret stores and high-confidence workspace key
  material (`~/.ssh`, `~/.aws`, `.env`, `*.key`, `*.p12`, …) at the OS-sandbox
  layer.
- Unlike the PreToolUse guards, it holds even in headless `codex exec`
  (ADR-0014).
- The `gh` and `docker` tool-token stores are deliberately left readable so
  those tools can read their own tokens.

| Key | Value | Why |
|---|---|---|
| `default_permissions` | `cf-guard`, extending `:workspace` | the profile is the only sandbox configuration, because a `sandbox_mode` key would shadow it |
| egress | broad public egress, exact loopback for local UI tests, live search | private, link-local and cloud metadata destinations and arbitrary Unix sockets stay closed under `cf-guard` and `cf-builder`; neither seat can serve on a local port, so local server tests run on the primary (TSK-337) |
| `approval_policy` | `never`; a reviewer or consult seat launches with no `--sandbox` flag, a builder seat with `-c 'permissions.cf-builder.description="CodeFlow builder seat"' -c 'permissions.cf-builder.filesystem={"~/.codeflow/locks"="write"}' -c permissions.cf-builder.network.allow_local_binding=false -c default_permissions="cf-builder"` and no `--sandbox` flag (ADR-0055, ADR-0075 D1 as amended on 2026-10-05; the description override lets Codex 0.163 and later open the seat, TSK-1003; the filesystem override carries the profile's home grants to a main checkout whose profile predates them, TSK-337) | under `cf-guard` the seat gets workspace writes and deletes, read-only enforcement and shell startup files, no Unix sockets and no local binding; a `--sandbox` flag would replace the profile, and full access would leave only git-guard, exec-guard, edit-guard, git hooks and CI |
| secret-file denies | `.env`, `.env.*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `.netrc`, `id_rsa*`, `id_ed25519*`, at the workspace root only | the recursive `**/` forms made Codex refuse every directory delete, so a nested secret file, such as `sub/.env` or a linked worktree's `.env`, stays readable, changeable and deletable by the seat |
| `cf-builder` profile | selected for builder seats, launched from the main checkout root (ADR-0075 D1 as amended on 2026-10-05) | it extends `cf-guard` with the common `.git` writable, its hooks and config read only, `~/.codeflow/locks` writable for `codeflow reserve` and heavy quick targets (TSK-1003), and the same network table as `cf-guard`, local binding off (TSK-337: with it on, Codex skips the proxy's private-network checks, so a builder reached private and metadata addresses; a builder seat now cannot serve on a local port, so tests that do run on the primary); `~/.codeflow/gate-runs` stays unwritable, since full gates and their evidence stay with the primary (TSK-337; since TSK-1044 no run record earns selection credit, so the store only holds reports); `cf-guard` writes neither home folder, since review seats run no gate; its spike ran fetch, worktree add, commit and builds unattended, a push to a remote outside the workspace root is denied, and a push to a hosted remote is not yet verified, so a builder whose push fails reports it and the caller pushes |
| shell startup files | read only in `cf-guard`, so in `cf-builder` too: every home and `/etc` entry of the action table's `startup_paths`, and the workspace root's `.envrc` (TSK-242) | a nested `.envrc`, such as one in a linked worktree, stays writable to the profile, since a wildcard directory part makes Codex refuse every directory delete; exec-guard and edit-guard refuse it. The entries name the default locations, so a `ZDOTDIR` or `XDG_CONFIG_HOME` moved elsewhere is writable; `codeflow doctor` reports one |
| environment | Codex's default `KEY`/`SECRET`/`TOKEN` scrub is kept (ADR-0025, ADR-0026) | the shell does not inherit provider secrets |
| `approvals_reviewer` | `approvals_reviewer = "auto_review"` | vestigial under `never`: it is not a human gate and does not fire on-request prompts |

The Codex hook wiring works as follows.

- `.codex/hooks.json` wires `codeflow hook git-guard` and
  `codeflow hook exec-guard` onto Codex's `PreToolUse` (Bash) event and
  `codeflow hook edit-guard` onto its `apply_patch` edits, each with
  `--contract 3`. `config.toml` enables the hooks engine.
- The `.codex/` starter is part of the enforcement floor, shipped from
  `--minimal` up.
- Codex loads a project's `.codex/hooks.json` only when that project's
  `.codex/` layer is trusted. Run `/hooks` inside an interactive `codex`
  session to trust the CodeFlow hooks.
- Codex records that trust against the absolute path of the hooks file, so each
  new clone or linked worktree asks again. A changed hooks file, such as the
  move to contract 3, asks again too.
- Codex's hook payload is byte-compatible with Claude's, so the same binaries
  run unchanged.

`session-orient` is wired for Codex `SessionStart` too, for all sources
(ADR-0013).

- An interactive Codex session opens with the same orientation digest Claude
  gets, and re-orients to it after a compaction (`source=compact`).
- One handler serves both harnesses with plain-text stdout that each injects as
  session context.
- The `codex_hooks` test pins the JSON wiring, while live firing rests on
  Codex's documented hooks contract.

### Credentials and tools

Authenticated command-line tools must be able to read their own configuration.

- The OS sandbox does not deny `~/.config/gh` or `~/.docker/config.json` to
  every subprocess, because doing so would also disable `gh` and Docker.
- Prefer OS keychains and credential helpers rather than plaintext tokens in
  those files.
- The harness denies direct file-reading tools. A host that cannot provide
  brokered or helper-backed credentials must treat arbitrary shell access as
  credential-bearing and tighten that task's tool boundary.

| Concern | Rule |
|---|---|
| Tool inventory | a settings file cannot install or authenticate every task-specific tool. `/cf-customize` inventories and canaries authoritative-doc research, GitHub, the stack format/lint/test/coverage/security toolchain, browser/Playwright, Computer Use or a surface driver, design tooling, and project-specific Model Context Protocol (MCP) servers, and proposes only the missing pieces |
| Where authentication lives | OAuth, keychain, app/MCP, or supported credential-broker paths; raw tokens do not enter the repository, prompts, logs, or arbitrary commands |
| GitHub and Docker configuration | permitted for autonomous tool use only after `/cf-customize` proves secure keychain or credential-helper storage; on a keyring-less inline-credential host it adds a file deny until a broker is configured |
| Claude's credential mask | the shipped sandbox removes the exact Anthropic, OpenAI, and AWS raw credentials named in ADR-0026 from Bash without stripping credentials from every hook or stdio MCP, and leaves brokered tools and MCP processes available. When a project needs a raw GitHub, npm, Cargo, or provider token, prefer a broker or keychain; otherwise configure Claude's user/managed credential mask with TLS termination and exact `injectHosts` |
| Subprocess scrub | `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` is a user/managed hardening option only after proving the project's hooks and stdio MCPs do not require those provider credentials |
| Auto mode | Claude deliberately ignores repository requests for auto mode and classifier policy, so a Codex-hosted peer launches interactively with `--permission-mode auto` and CLI-scoped `autoMode.classifyAllShell`; `/cf-customize` can offer the equivalent user default but never writes it without approval |
| Bypass mode | a sanctioned host mode the person chooses, like auto (ADR-0093, issue 451). Claude ignores `bypassPermissions` in a project file and starts the session in Manual mode, and a project `default` or `acceptEdits` outranks the user file, so the `default` and `bypassPermissions` presets set no mode and bypass comes from `~/.claude/settings.json` or `--permission-mode bypassPermissions`; `/cf-customize` canaries the live mode. `codeflow claude-session --install`, which the person runs in their own terminal, writes CodeFlow's session file, `~/.codeflow/claude/session.json`, with bypass mode only after they type yes (not offered when managed or user settings set `disableBypassPermissionsMode`), and a `claude` shell function that loads it with `--settings` inside CodeFlow projects, where the sandbox, host list and deny rules bind whatever the repository says. A start without it (an IDE, the desktop app, `command claude`) is warned at session start (operator decision 2026-10-31). The guards (`onFailure: "block"`), deny rules, ask rules, the fail-closed sandbox with no unsandboxed retry, git hooks and CI still hold; the classifier, allow rules and protected-path prompts are gone, and the excluded tools run outside the sandbox without a prompt |
| Codex review approvals | `auto_review` is configured but is not a human authorization path, and it does not pause for that subagent. Use `on-request` and `approvals_reviewer = "user"` only on a consult/no-edit lane when policy requires a human decision |

### Shared and personal settings

The shared files carry team policy; a few keys are each person's choice. The
scaffold ships a status line and leaves effort to each person (ADR-0087).
`cf-customize`'s `references/personal-settings.md` has the full guide.

| Key | Shipped in the shared file | Where a person sets their own |
|---|---|---|
| Claude `statusLine` | a `jq` line: model name and id, version, output style, folder, branch, context used with a green, yellow or red percentage, and, when Claude Code supplies them, the effort, the open pull request and its review state, the 5-hour and 7-day plan usage and the linked worktree (TSK-1070); without `jq`, folder and branch | `.claude/settings.local.json` |
| Claude `effortLevel` | none | `~/.claude/settings.json`, `.claude/settings.local.json`, or `--effort` at launch |
| Codex `model_reasoning_effort` | none | `~/.codex/config.toml`, or `-c model_reasoning_effort=...` at launch |
| Claude compaction point (`env`) | `CLAUDE_CODE_AUTO_COMPACT_WINDOW` 1000000 and `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` 35: compaction near 350K tokens, a team cost policy that also applies to subagents | `env` in `.claude/settings.local.json` |

The compaction point stays in the shared file because it sets the team's
token cost; `cf-customize`'s `references/claude-context-policy.md` states
the tradeoff: per-turn input grows with the context carried, a lower point
compacts more often, and detail kept only in the conversation may be lost;
durable records let a session recover when it rereads them, and the orient
digest points to them after a compaction.

- Claude Code reads managed settings, then `--settings` at launch, then
  `.claude/settings.local.json`, then the shared `.claude/settings.json`, then
  `~/.claude/settings.json`; a key the shared file sets hides the user
  file's, and a one-key flag such as `--effort` overrides that key for the
  session
  ([Claude Code settings](https://code.claude.com/docs/en/settings#settings-precedence)).
- Codex reads flags and `-c` overrides, then a trusted project's
  `.codex/config.toml`, then a `--profile` file, then `~/.codex/config.toml`,
  so a profile does not override a project value and a `-c` flag does
  ([Codex configuration](https://learn.chatgpt.com/docs/config-file/config-basic#configuration-precedence)).
  It has no untracked per-project file.
- `.claude/settings.local.json` is gitignored. Since Claude Code v2.1.211 a
  session in a worktree uses the file at the main checkout's root, so one
  file serves every worktree; outside git, with the repository root at the
  home folder, on Windows, or when the root, `.git` or `.claude` is owned by
  someone else, the file stays per checkout and a worktree needs its own
  copy ([where Claude Code keeps the local file](https://code.claude.com/docs/en/settings#where-claude-code-keeps-the-local-file-in-a-git-repository)).
- `codeflow update` applies its shipped-value rules only to `statusLine`
  and `effortLevel`: it moves a value still equal to the one it recorded as
  shipped, keeps and reports a value the project changed, keeps a deletion,
  and removes a key it no longer ships only while the key holds the
  recorded value. Every other plain top-level key, the attribution
  controls included, is restored whenever a project lacks it. `hooks`,
  `permissions`, `sandbox` and `env` follow their own merge rules:
  CodeFlow's own hook commands and missing sandbox entries are added back,
  while a shipped permission entry or `env` key the project removed stays
  removed when the recorded baseline shows CodeFlow shipped it. An `env`
  value still equal to the recorded shipped value moves to the new one,
  whatever the project meant by it, so a project on the compaction
  percentage of 50 moves to 35; a value that differs from the recorded
  shipped value stays, and the report names its key when the default
  changed. A `permissions.defaultMode` of `bypassPermissions` or `auto`,
  which a project file cannot set, is removed when the recorded baseline
  shows CodeFlow shipped it, and otherwise kept and reported (ADR-0093).

### Hosting the duo from either seat

A Codex-primary session can host the full duo, not only a consult.

- The host owns orchestration and routes production by the qualified
  capability binding, risk, evidence needs, and available capacity. No vendor
  receives implementation work merely because of its name.
- From a Claude Code host, the official Codex plugin provides the independent
  Codex lane.
- Both seats independently research and plan before approving the same
  versioned contract. Material design and implementation receive cross-lineage
  review (ADR-0023, ADR-0046).

Google's Antigravity `agy` is **not** bound automatically. Its hook dialect
differs and its macOS reliability is unresolved. The `cf-delegate` skill
carries an experimental, manual opt-in snippet for those who want it. As a
*delegate*, `agy` is retired: its only documented drive shape is headless
one-shot, which ADR-0018 prohibits.

### Dated verification stamps

Hook payload contracts and config schemas move fast on both sides, so every
stamp below records what was true on its date, not a current guarantee. The
release runbook ([releasing](releasing.md), "Re-verification before tagging")
re-verifies them before each CodeFlow tag.

| Claim | Observed on | Date |
|---|---|---|
| Hook payload and config-schema parity | codex-cli 0.144.3, Claude Code 2.1.220 | 2026-08-02 |
| `pre-push` refuses a Codex force-push to protected `main` | codex-cli 0.142.5 | historical, retained by ADR-0008 |
| Headless `codex exec` did not run project PreToolUse hooks, even with `--dangerously-bypass-hook-trust` and the layer trusted | codex-cli 0.142.5 | historical, retained by ADR-0008 |
| `agy` does not read `AGENTS.md` | Antigravity `agy` 1.0.15 | historical |

- Treat the in-session guards as an interactive-session safeguard, and rely on
  the git-hook plane where Git invokes the installed hooks.
- CodeFlow's own flows no longer produce headless runs. ADR-0018 makes
  cross-model transport interactive-only, so the consult, delegate and duo
  flows never shell out to `codex exec`, and a headless Codex run is outside
  those flows.
- Do not generalize that historical Codex observation to every harness.
  Claude's
  [programmatic-mode documentation](https://code.claude.com/docs/en/headless)
  states that ordinary noninteractive sessions load project hooks, while bare
  mode skips their automatic discovery.
- This does not authorize headless CodeFlow work.
- Earlier hook-specific evidence is retained by ADR-0008, ADR-0013, and
  ADR-0014. How far each plane reaches is in
  [enforcement planes](architecture/enforcement-planes.md).
