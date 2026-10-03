---
id: ADR-0075
uid: 12db10d6-e393-4367-8a33-258001a0df9c
title: "Agent sessions refuse instead of prompting, under a guarded floor"
date: 2026-09-28
status: proposed          # proposed | accepted | superseded
supersedes: [ADR-0066]
superseded_by: null       # ADR id, set on supersession
architecture_impact: "The 2026-09-29 amendment defines the shipped action-table refusals, native edit guards and seat postures; TSK-189 owns policy authority and fail-closed hooks"
---

# ADR-0075: Agent sessions refuse instead of prompting, under a guarded floor

## Delivery note (2026-09-29)

This record is still proposed; TSK-175 accepts it. Release 3.0.0 carries
TSK-171, the first of the five implementing tasks, and TSK-190's D1 spike
and D2 launch text. The rest follows in
3.1. Until then, the decisions below describe intended behavior, and the
3.0.0 CHANGELOG entry is the account of what shipped.

Shipped in 3.0.0 (TSK-171):

- Decision 1: the Claude, Codex and Grok presets carry no ask rules; deny
  rules for the listed action families are generated from one action table
  (`crates/codeflow-core/src/security/actions.json`), with
  `.codex/rules/codeflow.rules` for Codex and `.grok/sandbox.toml` for Grok.
  `codeflow update` merges the permission arrays three ways against the
  last shipped copy. The deny rules match the listed spellings only.
- D5 in part: `security.privilege_escalation` defaults to `block`, and
  exec-guard refuses a privilege launcher run directly, chained, or wrapped
  in a shell `-c` string or `eval`.
- D4 in part: `security.headless_peer_runs` defaults to `block`;
  `security.headless_opt_in` is accepted but not yet read.
- The profile assets for D1 to D3: the Codex `cf-guard` profile with the
  network proxy and a `cf-builder` profile that is defined but not selected,
  and the Grok `cf-guard` and `cf-guard-worktree` profiles. The launch
  postures themselves are unchanged, except D2's below.
- The Claude presets' sandbox credential variable and store denies.
- The policy keys the later tasks read, at their shipped defaults, with no
  check reading them.

Also in 3.0.0 (TSK-190):

- D2: Codex reviewer seats launch with `--ask-for-approval never` and no
  `--sandbox` flag, so `cf-guard` applies; the cf-model-orchestrator and
  cf-herdr launch text says so. `cf-guard` denies secret files at the
  workspace root only, since the `**/` forms blocked every directory
  delete; nested secret files are a recorded gap.
- D1: the spike ran on 2026-09-29 on Codex 0.159.1 and did not pass.
  Fetch with an absolute remote URL, worktree add, stage, commit, cargo and
  npm builds ran unattended from the main checkout root, with `.git/hooks`
  and `.git/config` unwritable; a push to a bare remote outside the
  workspace root was denied, and a push to a hosted remote was not tried.
  Builder seats keep full access with that gap recorded, and `cf-builder`
  stays defined and unselected.
- D7, observed on Codex 0.159.1 in a native capture for TSK-188: the
  quoted form `-c 'projects."<worktree>".trust_level="trusted"'` did not
  skip the folder-trust dialog (the argument reached Codex as written).
  The unquoted `-c projects.<absolute path>.trust_level=trusted` did, and
  works only for a path with no dots in it. Separately, Codex runs a
  project's hooks only after a person grants a one-time hook-trust prompt,
  stored as `hooks.state` with a `trusted_hash` in `~/.codex/config.toml`;
  a changed `hooks.json` needs the grant again. Both stay operator steps;
  the 2026-09-29 amendment below drops decision 8's queue.

Follows in 3.1:

- TSK-172: decision 2 (guards for wrapped, flag-led and interpreter forms,
  and edit-guard), decision 5 (the enforcement baseline, the repository
  binding, the `codeflow baseline` verbs, merge approval and the `gh pr
  merge` rule), the `git pull` refusal, D4's opt-in, D9's workflow-push
  refusal and D10's fixture root.
- TSK-173: decision 3 and D8 (fail-closed guards, `--contract` and the
  session-start check) and decision 4 (seat readiness).
- TSK-174: decision 6 (the one sandbox exit) and D7's Herdr grammar;
  ADR-0029's classified retry stands until then.
- TSK-175: the D1 to D3 launch postures, D7's briefing rules, D9's token
  route, decision 8 (the operator-actions list), the live journey and the
  acceptance of this record.

## Context

The shipped Claude preset carried 161 ask rules. Ask rules prompt in every
Claude mode, including `bypassPermissions` and `auto`, which ADR-0055 sets as
the production modes, so delegated and primary sessions stalled; the
workspace removed its asks and its guard hooks by hand. Codex and Grok
presets carried separate hand-written lists. A review in four rounds by
Fable, Codex and Grok (2026-09-28) found that the text rules and the existing
guards missed wrapped privilege escalation (`env sudo`, `bash -lc 'sudo -n
id'`), publishing spellings with leading flags, tag pushes, interpreter
writes to the enforcement files, `git stash drop`, and headless peers started
from interpreters; that Claude's unsandboxed retry let any of those leave the
sandbox in bypass mode, where ADR-0029's classifier does not run; that guard
hooks fail open when they do not run; and that a guard reading its policy
from the repository can be relaxed through the index or a branch.

The operator's direction: the most autonomy with the protections that matter
kept in force; delegated seats run autonomous and interactive, never
headless; safety comes from project settings that stay enforced in those
modes; blocks instead of prompts.

## Decision

1. **No prompts in agent sessions.** The presets carry no ask rules. These
   action families are refused in agent sessions by native deny rules
   generated from one action table for all three harnesses, and the operator
   performs them:
   - privilege escalation: `sudo`, `su`, `doas`, `pkexec`, `gsudo`,
     `runas`, `Start-Process -Verb RunAs` and `osascript ... with
     administrator privileges` (wrapped forms are decision 2's);
   - publishing packages and gists: `cargo`, `npm`, `pnpm`, `yarn` and `uv`
     publish, `twine upload`, `gem push`, `gh gist` create and edit;
   - releases and tag pushes: `gh release` create, edit, upload and delete,
     and `git push` of tags (`--tags`, `--follow-tags`, `refs/tags/`);
   - repository and account changes: `gh repo` delete, archive, rename and
     visibility, `git push --mirror`, `gh secret` set and delete, and `gh
     auth` login, switch, setup-git, token, refresh and logout, and `git
     credential`;
   - keychain reads: `security find-generic-password`,
     `find-internet-password` and `dump-keychain`, and reads of the listed
     secret stores;
   - user-level persistence: `defaults write` and `delete`, `launchctl`
     load, bootstrap and enable, `crontab -e` and `-r`, `systemctl enable`
     and `disable`, and user registry writes.

   This list is the decision's own; it does not depend on another
   document. The only remaining prompt is Claude's critical-path
   countdown, which denies itself.
2. **Guards parse what text rules cannot.** exec-guard, git-guard and a new
   edit-guard refuse the unwrapped and interpreter-embedded forms of those
   actions, discarding of locally unique work, and writes to enforcement
   paths. These are command-level rules; a script or binary that starts a
   child process is outside them, with the sandbox, the baseline and the
   token's scope below.
3. **Guards fail closed and are version-gated, at every tier.** Every
   CodeFlow hook command carries `--contract N`; a guard that runs and
   fails, or a binary that is missing or older than the contract, refuses
   the call and names the exact command (the binary install, or `codeflow
   update`), queued for the operator. The session-start check reports an
   outdated binary or project before work begins. The operator's options
   are to update, to let the agent continue work that needs no guarded
   tool, or to roll back the binary; the agent re-checks with `codeflow
   doctor` once the operator confirms. CodeFlow has no fail-open switch. A
   hook that times out, and a harness's own hook disable by the operator,
   are outside CodeFlow; the digest reports the second, and both are stated
   residuals.
4. **Seats show their guards ran before a brief.** A host briefs a seat
   only after a guard-readiness preflight in which each required guard writes a receipt for
   this launch, bound to the session, working directory, contract and
   baseline, and only when each receipt's hook-entry hash equals both the
   live entries read at acceptance and the current approved identity, and
   its harness and posture match the route; the readiness snapshot is taken
   from that same validated read, and a change during validation is stale.
   Each receipt also carries the admission pin its hook observed; it must
   equal the tuple the host set at launch, and an absent, malformed or
   different pin is never ready. A route whose harness cannot carry the pin
   to its hooks is unsupported until a qualified carrier exists. If the harness
   cannot establish it, or the repository is not enrolled or its binding is
   lost, the route is reported unavailable. Readiness holds at the moment of
   the guard-readiness preflight: the host checks it again before every prompt and runs a
   fresh guard-readiness preflight after any change to the baseline, the binding, the
   harness config or the binary. Where the hooks run outside the seat's
   sandbox and write where the seat cannot (Claude seats; Codex under a
   profile), a receipt shows that the guard ran while the sandbox holds.
   Where the seat can write the receipt location (Grok, whose hooks share
   its sandbox; Codex under full access), the receipts are cooperative
   readiness evidence, not proof, and the host labels the route so.
5. **Guard authority comes from an enforcement baseline.** Outside the
   repository, keyed by the common git directory, advanced only by `codeflow
   init`, `codeflow update`, `codeflow baseline approve` and `codeflow
   baseline bind` run by the operator; init, update and the first approve
   bind the root in the same operation. The store's location comes from the
   operator's account, never from `HOME` or `XDG_STATE_HOME`, and a guard
   never creates a store. An operator-written binding ties each enrolled root
   to its common dir; the guards resolve each command's effective git dir,
   worktree, index and config, and refuse overrides they cannot resolve.
   Guards never read their policy from `HEAD`, the index or the working copy
   of an enrolled repository. A changed common dir, an unreadable,
   unparsable or partly missing store, or a seat's launch pin the store does
   not satisfy is a lost binding: git mutations are refused until the
   operator rebinds, and it is never read as not enrolled while the store or
   a pin remains. A new process after total erasure of the store, on a route
   where the seat can write it, sees "not enrolled"; that is the declared
   residual. Outside every binding, a hook without `--contract` keeps its old
   policy source, which is a compatibility selector and not proof that the
   repository is old; with `--contract`, an unenrolled repository is judged
   by the policy built into the binary. Nested repositories are decided once,
   in an operator-reviewed inventory: adopted children get their own binding,
   excluded children are judged by the built-in policy and are never seat
   routes, and a changed child route voids its boundary. Commits and ref
   transitions whose enforcement entries, in every tree they commit or check
   out, match no approved identity compatible with the current contract are
   refused; git commands that write live enforcement files, write the index
   or refs directly, or change the route to the baseline are refused.
   **Host-side inspection runs no program the repository selected:** a merge
   whose changed paths select a custom merge driver (resolved by its
   effective definition, so a custom driver named `text`, `binary` or
   `union` counts as custom), a non-built-in `merge.default`, a defined
   filter or renormalization is refused before any
   query, and the merge query runs in a discarded temporary repository with
   sanitized config, attributes and environment. **A local landing that
   changes an enforcement path** waits for the operator's approval, which is
   bound to the repository, the source and target commits and the method,
   and is revalidated at the landing. **A server-side landing** (`gh pr
   merge`) passes only in an immediate mode, with the head pinned, when no
   commit on the pull request changes enforcement entries relative to its
   parents and the head's entries equal the base's, so every tree GitHub
   can land carries the base's own entries; deferred modes (auto-merge, a
   merge queue), `--admin` and every other case are refused, and a local
   approval never covers a server-side landing.
   **Evaluation fixtures** take their authority from the operator's
   fixture-root record (an admission id, the root's real path, device and
   inode), found through the same authority root, and from each trial's
   committed state in place of a baseline. A command gets the fixture route
   only when every location and mutation target it resolves, by real path
   and effective git routing, lies inside the admitted root; an external
   target is judged under its own authority, never the fixture's.
6. **One sandbox exit.** A primary Claude session may rerun outside the
   sandbox only an argument vector an allowlist entry spells out in full,
   with the program and child executables pinned by path and the script by
   version and digest; delegated seats deny the retry natively. Default entry:
   the Codex plugin companion, unless spike 0 shows it runs sandboxed with a
   narrow local allowance, in which case the companion entry is dropped.
   The workspace policy also carries D7's named Herdr list under the same
   conditions and grammar; it is not shipped to consumers.
7. **Settings stay the operator's.** Enforcement files are never written by
   agent sessions; `/cf-customize` prints changes; machine relief stays in
   `.claude/settings.local.json` (kept from ADR-0066).
8. **Operator steps are batched on one list.** Every step that needs the
   operator (Herdr keys and seat approval prompts, folder trust a launch
   cannot grant, binary and project updates, workflow pushes, enforcement
   approvals, Codex hook trust, the fixture root, enrollment) is recorded on
   `.codeflow/operator-actions.jsonl` in the main checkout, which is
   gitignored and is not an enforcement path. Guards append when they refuse
   and say "queued for the operator"; agents append what no guard recorded.
   `codeflow status`, the orient digest and the orchestrator's status report
   show the open entries. The list grants nothing: each guard re-checks the
   live condition. It is an append-only event log written by one writer
   that binds to the validated main checkout, opens the file without
   following links, accepts only a single-link regular file it owns, locks
   the open file for each append, and on any failure writes nothing and
   says the action was not recorded, leaving the original refusal in place.
   Marking a row done or deleting it hides nothing: kinds with repository
   evidence, workflow pushes included, are shown from that evidence, and
   the host keeps the others in its own task record, since deleting from
   the queue is not completion. That record also keeps every workflow-push
   requirement whose exact source and destination cannot be reconstructed
   from the branch's configured push destination, as the parsed tuple
   (source commit, push URL, destination ref), until evidence for that
   destination clears it or the operator withdraws it. A stored command is display text and is
   never run automatically. The agent records and continues, and
   interrupts the operator only when nothing else can proceed.

### Operator decisions of 2026-09-28

| # | Decision |
|---|---|
| D1 | Codex builder seats move to the `cf-builder` profile (`never`, writable common `.git`, secret denies) only if a spike shows fetch, worktree add, commit and builds working unattended; otherwise full access stays with the gap recorded |
| D2 | Codex reviewer seats run `--ask-for-approval never` with no `--sandbox` flag, which selects the project's `cf-guard` profile |
| D3 | Grok builder seats run `--always-approve` with a sandbox: `cf-guard-worktree` (the `cf-guard` profile plus the common git dir), launched in their own task worktree with `--trust`, since Grok treats a nested checkout as a separate workspace. Because the sandbox write-protects the trust file, folder trust for that exact directory is saved by a separate trust step before the sandboxed launch (D7), and readiness is judged by the loaded hooks, never by assuming one argv saved trust. Grok reviewer and consult seats keep `--permission-mode auto --trust`, launched in their own worktree, non-bypass (ADR-0055 decision 3; S13), unless the closeout excludes that route. Every harness has a builder posture and a non-bypass consult or review posture |
| D4 | Consumers default to interactive harness sessions: a headless peer run is refused unless `security.headless_opt_in.families` names its catalog family (`claude`, `codex`, `grok`) with a reason. Config decides; whether an interactive harness is detected only shapes the message and doctor's report. Supersedes TSK-136's consumer warn default; adopters at the old default move to block on update |
| D5 | Privilege escalation is blocked, including wrapped forms, `su`, `doas`, `pkexec`, `runas` and `osascript ... with administrator privileges`. Amends ADR-0008's warn level |
| D6 | The operator removes the `delete_repo` scope from the gh token personally |
| D7 | A sandboxed primary may run the named Herdr list outside the sandbox. Seats are briefed only with `herdr agent prompt`, never by typing into a pane. Folder trust is granted at launch through each seat's own flags, for the seat's own worktree only: a separate Grok trust step, the Codex `-c projects."<worktree>".trust_level="trusted"` override, and for Claude nothing, since a worktree takes its main checkout's trust; this keeps the operator's 2026-09-22 folder-trust authorization. `agent send-keys` sends only `Escape`, only to a registered seat. `agent prompt` refuses `!` and every slash command except `/compact`, `/clear` and `/new`, so nothing that changes permissions, approvals, trust, hooks or the model passes. Shell panes and seat approval prompts stay the operator's. Enforced on every Herdr call, independent of the companion entry. Folder trust never trusts a hook definition |
| D8 | Guard hooks fail closed at every tier, with `--contract` on every hook; decision 3 |
| D9 | Agents use their own fine-grained token in `GH_TOKEN`, without workflow, administration or gist permission; a GitHub App is documented as the team route. The operator's own login stays for setup and workflow pushes. A push that would introduce a workflow change to its destination, judged against the destination's own advertised refs (read once, read-only) and never against a local tracking ref, is refused before it transfers anything and queued for the operator; the agent keeps its commits and carries on. Missing evidence is refused without queuing, naming the fetch or retry |
| D10 | Evaluation fixtures run under one operator-admitted fixture root; trials use their own policy for local rules and at least the built-in floor for security families; every remote-affecting action is refused there, except a push or fetch to a local repository inside the same root; eligibility is decided by real path and effective git routing (decision 5); seats are `ready (fixture)`, never production-ready; the eval kit preserves results outside the root and then removes the run by its marked cleanup. The fixture-root step also carries the Claude folder trust for its fixtures, which has no launch flag |

### Readings of earlier decisions

- **ADR-0066** is superseded: its default preset kept ask rules for rooted
  deletes and relied on the classifier; this decision refuses instead. Its
  rule that the preset sets no `defaultMode` and that machine relief lives in
  local settings is kept.
- **ADR-0029** is amended: `allowUnsandboxedCommands` stays `true`, and the
  retry's "trusted installed tool" condition becomes the argument-bound
  allowlist, which also holds in bypass, where the classifier does not run.
- **ADR-0008** is amended by D5.
- **ADR-0055** is amended by D1 to D3; the production modes themselves are
  unchanged.

## Consequences

- Delegated and primary sessions run without prompts; rung-4 actions need
  the operator, by design.
- Adopters upgrade the binary before `codeflow update`; Codex needs the
  changed hook definitions re-trusted once; enforcement-file changes are
  approved with `codeflow baseline approve` before commit.
- A machine without the `codeflow` binary, or with one older than the
  project's contract, cannot run shell or edit tools in agent sessions at
  any tier; the session starts with a line naming the command, and the
  agent continues reading, research and planning until the operator
  updates or rolls back. There is no switch that lets guarded calls through
  meanwhile.
- Agents never stop for an operator step: it goes on the operator-actions
  list and the agent continues other work. A workflow change waits on the
  list for the operator's push (2 of 326 merged pull requests since August
  touched `.github/workflows`, by the coordinator's count).
- Seats are briefed and steered only through `herdr agent prompt` and
  `Escape`; a seat stuck on an approval prompt waits for the operator.
- The agents' token narrows what agent sessions use by default. The
  operator's login stays on the same host for setup and workflow pushes, so
  a deliberate read of a stored login beyond the refused forms remains a
  stated residual.
- An adopter clones an enrolled repository on a new machine and admits it
  there once, through `update`, `init`, the first `baseline approve` or an
  operator-run setup flow; an identity already approved on that machine
  needs only a binding-only confirmation of the destination. A repository
  the operator moved on purpose needs `codeflow baseline bind`. An outer
  workspace takes one inventory of its nested repositories. Checking out a
  tree approved under an older contract needs requalification; reading it
  with `git show` does not.
- A merge that changes an enforcement path, by `git merge` or `codeflow
  integrate`, waits for the operator's `codeflow baseline approve --merge
  <source> --into <target>`, named in the refusal and queued, and a moved
  source or target needs a fresh approval. An agent can merge a pull
  request on GitHub only immediately and only when it changes no
  enforcement path in any commit; auto-merge and merge queues are the
  operator's.
- A session takes its harness preset (permissions, sandbox, hook set and
  contract) from where it starts, while guards judge each command by the
  target repository's policy. In workspace mode (an umbrella whose policy
  sets `git.root_branch`, conventionally `integration/workspace`), doctor at the umbrella
  warns when the umbrella's preset is less strict than an enrolled nested
  project's.
- Hook trust is the operator's: agents never review or trust hook
  definitions, and a readiness failure for untrusted hooks names the
  operator.
- A guard-readiness failure is a native exclusion of that harness route in
  that worktree, with a named reason, never a model exclusion.
- Agent sessions integrate with `git fetch` and then a checked `git merge`
  or `git rebase`; `git pull` is refused there and unaffected in a human
  terminal. An agent merge whose changed paths use a custom merge driver or
  a defined filter is refused, and the operator runs it.
- Residuals stated: hook timeouts; a harness-level hook disable by the
  operator; a hook trust change between guard-readiness preflights, which the next guard-readiness preflight
  catches; guard-readiness preflight receipts on Grok and on Codex under full access are
  seat-writable, so a seat, an unguarded file or MCP write tool, or a direct
  guard run with a fabricated payload could forge them; command-level
  checks miss opaque child programs; the baseline and the binding registry
  have no OS protection for Codex builders under full access or Grok without
  D3, and total erasure of the store there is not detected by a new process
  without a launch pin; a Codex thread started through the companion runs Codex's legacy
  sandbox without the secret denies, unless spike 0 moves it inside Claude's
  sandbox.

## Still to establish

Each item closes in the implementing tasks as a measured or sourced outcome,
an explicitly unsupported route, or an N/A citing the operator's decision:
whether Claude's hook payload carries `dangerouslyDisableSandbox`; whether an
exported variable persists across Bash calls; spike 0; the D1 spike; Grok
`read_only` precedence inside the cwd and its absolute-glob denies; Herdr
calls inside the sandbox and `agent prompt` to an exited agent; whether
`codex exec` and `claude -p` can start inside Claude's sandbox; Codex hook
timeout behavior and re-trust of changed hooks; the Windows shell forms;
minimum harness versions; sandbox handling of read carve-outs; the Grok
reviewer in auto mode; the live argv of existing Herdr Codex
seats; which posture fields each harness's hook payload carries; whether
each harness passes a seat's launch environment to its hooks; where Codex
keeps hook trust; whether Grok `auto` prompts on the guard-readiness
preflight's probe edit; whether each harness carries the admission pin to every required hook on
each route; which of `/compact`, `/clear` and `/new` change a harness's
session identifier; each harness's text normalization before slash-command
parsing; a Grok trust-only form that saves trust and exits (the
installed 1.0.41 does not list `--trust` in its help), and whether a linked
worktree needs its own Grok trust; whether Grok resolves the relative
`read_write` entry `../../.git` of `cf-guard-worktree` (if not, Grok
builders are unsupported in linked worktrees); the Codex trust override's effect on the
prompt and on loading project config; which of `/compact`, `/clear` and
`/new` each harness accepts; which credential git over HTTPS presents when
`GH_TOKEN` and a stored operator credential both exist (for `gh` itself,
`GH_TOKEN` takes precedence, per `gh help environment`); the landed TSK-150 authority
rows.

## Sources

- Claude Code: permissions, permission modes, sandboxing, hooks and settings
  reference at <https://code.claude.com/docs/en/>.
- Codex: permissions, rules, hooks at <https://learn.chatgpt.com/docs/>;
  source `openai/codex` `codex-rs/` at `4fd5745e`.
- Grok Build: <https://docs.x.ai/build/> and the shipped user guide.
- GitHub CLI 2.67.0: `gh help environment`, `gh pr merge --help`.
- Git: `git(1)`, `gitrepository-layout(5)`, `gitattributes(5)`,
  `git-init(1)`, `git-config(1)`, `git-merge-tree(1)`, `git-fetch(1)`,
  `git-pull(1)` at <https://git-scm.com/docs>.
- Review record: settings review rounds 1 to 6, Grok round 4 and the EPC-018 reconciliation, 2026-09-28; the operator's decisions D7 to D10 of 2026-09-28; Codex rounds 7 to 9 and Grok rounds 5 and 6.

## Amendment, 2026-09-29: guard the commands without a separate state system

TSK-188 replaces the guard work in TSK-172 and the planned mechanisms in
TSK-174 and TSK-175. This note supersedes the earlier delivery schedule and
architecture description where they differ. Version 3.0.0 is still pending;
its earlier delivery note records the implementation before this amendment.
The pending changelog now includes these guard fixes in 3.0.0.

Decisions 1, 2, 3 and 7 remain: no ask rules in agent sessions; guards judge
parsed commands and edits; guards fail closed at every tier; and the
operator owns enforcement settings. TSK-188 carries decision 2's wrappers,
leading flags, interpreter forms, loss of local-only work and edit-guard.
TSK-189 carries decision 3's fail-closed hooks and contract checks. Keeping
decision 3 does not claim that TSK-188 has shipped those checks. Opaque
programs that launch children remain outside command parsing; a hook or a
native deny list is not an operating-system boundary.

Decision 6 is amended. A delegated seat denies an unsandboxed retry through
its native configuration. A primary's permitted retry remains subject to
the seat's native permissions and to exec-guard and git-guard judging the
retried command. There is no argument-bound retry allowlist, executable
pinning store or separate retry approval mechanism. A refusal is not a
reason to choose another spelling or launcher. This replaces the allowlist
reading of ADR-0029 above; it does not authorize an otherwise refused
action.

D7's Herdr list remains unshipped workspace practice. Its enforcement clause
is withdrawn: CodeFlow does not implement Herdr subcommand, key or slash-
command grammar enforcement. The workspace's briefing and trust practice
may still use `herdr agent prompt` and its stated restrictions, but those
instructions are not a guard guarantee. An unavailable route is reported
as unavailable; it does not authorize a shell-pane or approval bypass.

Decisions 4, 5 and 8 are dropped:

- Decision 4's readiness receipts and admission pins would add a session
  state protocol without proving that a seat-writable receipt was genuine.
  Native launch evidence and the existing route preflight remain; no
  receipt store is introduced.
- Decision 5's external baseline, binding registry, enrollment, approval
  identities and fixture-root authority would add a second policy system.
  TSK-189 instead owns the landed-policy source and its tracking-ref
  protections. It applies the stricter policy from the configured remote's
  default branch and the declared target. Its no-remote fallback and other
  residuals are stated in that task; this note does not claim that those
  checks are already implemented by TSK-188.
- Decision 8's operator-actions queue duplicates the host's record of work
  waiting on the operator. A refusal names the operator's route, and the
  session records the blocker in the existing task or status report while
  continuing authorized work. No queue file, writer or automatic replay is
  introduced. Earlier statements that an action is queued no longer apply.

D10's operator-admitted fixture root and `ready (fixture)` status are also
withdrawn. The evaluation flow retains disposable marked fixtures,
trial-specific containment, local fixture remotes, evidence preserved
outside the fixture and cleanup limited to the run's marked roots. These
are the evaluation flow's containment rules, not a new production guard
policy or authority store.

D4's proposed `security.headless_opt_in` mechanism is dropped. Existing
policy files may still carry the key: validation accepts it with a clear
deprecation warning, enforcement ignores it, and `codeflow update` removes
it. Removing the unused Rust structure does not invalidate adopter policy.
`security.headless_peer_runs` remains the single configurable policy level
for headless peer runs, including interpreter forms. Changing that level
does not change the interactive-only consult, delegate and qualification
contracts. Native deny rules are independent and still apply.

TSK-188 owns the corresponding retry, deletion and route-availability text
from TSK-164 AC-1, AC-4 and AC-7. TSK-189 owns the policy-source,
fail-closed, integration and hook-trust text in AC-2, AC-3 and AC-6. TSK-190
owns the Codex launch-profile changes. No dropped mechanism is a
prerequisite to those tasks.

## Amendment, 2026-10-03: agents trust unchanged CodeFlow hooks in Codex

The operator decided on 2026-10-03 (TSK-213) that when a seat asks whether
to trust the project's hooks, the calling agent may answer "trust" only when
every hook definition the seat would load is byte-identical to the one at
the pull request's base, the target tip; otherwise the operator answers in
the seat's own surface, and the agent never trusts a changed hook or picks
"continue without trusting".

Review of that rule (PR 37, round 1) showed that identical definitions do
not mean identical executed code: an unchanged hook running `sh ./check.sh`
passed the byte comparison while the script it runs had changed. It also
showed that Codex loads hooks from inline config, user, global and plugin
sources as well as the project file, and that Grok reads other harnesses'
hook files and its project trust grants MCP and LSP servers along with
hooks. This amendment therefore narrows the operator's decision, on that
review evidence, to the case where the decision's premise holds. The agent
answers "trust" only when all of these hold:

1. The seat is Codex, whose prompt grants hooks only.
2. The prompt lists only the project's Codex hooks file.
3. That file is a regular file, not a symlink, and is byte-identical to the
   file at the immutable SHA of the successfully fetched target tip. A
   failed fetch or an unresolved target means no trust.
4. The file is also byte-identical to CodeFlow's managed copy, so every
   hook command is, as a whole string, a form CodeFlow ships:
   `codeflow hook <name> --contract <N>` followed by CodeFlow's shared
   missing-binary probe. These run only the installed `codeflow` binary,
   resolved outside the repository, and system tools; no repository script,
   interpreter or relative path.

Everything else goes to the operator: Grok's combined grant, any changed or
extra hook, any hook from another source, and any hook that runs repository
code. This replaces the consequence "Hook trust is the operator's: agents
never review or trust hook definitions" and decision 8's Codex hook-trust
entry only for that one case.

Why: the earlier rule kept an agent from deciding which guards run. When the
four conditions hold, the hooks the seat would trust are CodeFlow's own
guard commands as reviewed and landed on the target, and they run no code
from the repository, so trusting them leaves the reviewed guards in place.
The check does not review hook behaviour, and it makes no claim about hooks
outside that case.

This also settles D3's agent-launched `--trust`: a Grok seat never launches
with `--trust` from an agent, since Grok's project trust is the operator's.
D3's builder route (`--always-approve --sandbox cf-guard-worktree`) stays
unqualified until the items listed under "Still to establish" are proven,
so no Grok seat builds until then. The procedure lives in
`cf-model-orchestrator/resources/routing/hook-trust.md`, reached from the
transport rule's "First-run prompts" (ADR-0077).
