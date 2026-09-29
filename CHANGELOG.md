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
  the tag stays where it is. See "Historical bridge into v3" in
  `docs/releasing.md`.

## [3.0.0]

_Staging evidence: this section was first staged on 2026-08-02; that was not a
publication date._

> **Upgrading from 2.1.0.** Take these steps in order; the entries below give
> the detail.
>
> 1. On a planning branch, make the repairs in the breaking migrations list
>    under Changed (coverage scopes, test modes,
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
> 5. With durable work tracking, a pull request carries `Task: TSK-NNN` or
>    `Task: none: <reason>` unless its branch names the task.
> 6. A project that adopted the bundled portal follows the ownership table
>    in `docs/releasing.md` before its next portal update.

### Added

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
  `crontab -e` and `-r`, `systemctl enable`, registry writes). Codex gets
  `.codex/rules/codeflow.rules` and a `cf-builder` profile beside
  `cf-guard`, which now runs the network proxy (tested on Codex 0.157.1;
  earlier versions are unqualified). Grok gets `.grok/sandbox.toml`,
  written only when absent. `security.privilege_escalation` and
  `security.headless_peer_runs` default to `block`, and new policy keys
  ship for the guard checks. exec-guard refuses a launcher run directly,
  chained or wrapped in a shell `-c` string or `eval`; a shell string that
  reaches no launcher, `source` and `LD_LIBRARY_PATH` are not refused.
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
    deny entry from `.claude/settings.json`, or set its policy level in
    `.codeflow/policy.json`; update keeps both. A local ask rule cannot
    restore a denied action, because a deny rule wins.
  - Credentials (D9): agents use their own fine-grained token in
    `GH_TOKEN`, without workflow, administration or gist permission. This
    route narrows credential exposure only on a host that carries the
    agent token alone. The operator's login stays on this host for setup
    and workflow pushes, so the token narrows what agents use by default
    and does not stop a deliberate read of a stored login outside the
    refused forms. A push that introduces a `.github/workflows/` change is
    refused and queued for the operator.
  - Integration: agent sessions replace `git pull` with `git fetch`, then a
    checked `git merge --ff-only` or `git rebase`. A human terminal is
    unaffected.
  - Enforcement baseline: `codeflow update`, `codeflow init` or the first
    `codeflow baseline approve` binds the repository root, with a
    binding-only confirmation for an identity already approved on the
    machine. An outer workspace takes one `codeflow baseline bind
    --inventory` for its nested repositories. A local landing that changes
    an enforcement path waits for the operator's `codeflow baseline approve
    --merge <source> --into <target>`, bound to the source and target
    commits and the method. An agent's `gh pr merge` passes only in an
    immediate mode and only when no commit on the pull request changes an
    enforcement path; auto-merge and merge queues are the operator's.
  - Seats (D7): seats are briefed only through `herdr agent prompt`, folder
    trust is granted at launch, `Escape` is the only key sent to a seat,
    and `/compact`, `/clear` and `/new` are the only slash commands.
  - Guard hooks (D8) fail closed at every tier, with no fail-open switch.
    A session starts with a line naming the outdated side and its exact
    command, and the options: update, continue other work, or roll back
    the binary.
  - Operator actions: every step that needs the operator is recorded on
    `.codeflow/operator-actions.jsonl` in the main checkout and shown by
    `codeflow status`, the orient digest and the orchestrator's status
    report.

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
  finding carries the smallest evidenced remedy. One review round runs every
  reviewer in parallel, the reviewer who raised a fixed finding confirms it,
  and a minor fix needs no new round. Rounds are bounded by change class: two
  repair cycles for code, and two rounds per submitted version for docs and
  records. cf-develop's bound for code drops from three cycles to two.
  Blocker navigation now stops before a change departs from what was
  approved. The writing reference gains a copy guide with ten sections, each
  with an example quoted from a named source. `codeflow update` installs the
  new section and the changed skills at the standard and full tiers, and the
  writing reference at every tier.

<!-- codeflow:release-impact minor -->
- **Acceptance bound to the reviewed commit.** Completing a task with
  `task status complete`, and every completion in a pull request range in
  `codeflow ci`, now checks that the acceptance block's `reviewed` commit is
  the completing commit (a task pull request's head) or an ancestor after
  which only the record's status and Closeout changed, or the second parent
  of a clean landing merge that only merges and planning records follow,
  and that each waiver names a planning-only amendment commit that changed
  that criterion, is in the completion's history and is on the task's own
  integration target. `task status complete` also
  refuses uncommitted changes outside the record. A pull request that
  changes a task's criteria is refused unless its validated class is
  planning-only or a checked epic line, whatever its branch prefix. A range touching the adopter-facing path set
  needs a task with a `(journey)` criterion or one serving the epic's
  journey, and a leaf serving it links the evidence that ran or names its
  narrower path. A criterion tagged `(after release)` is `deferred` with an
  owner, a window and a listed follow-up, never verified at build time.
  A tag opens or closes its criterion, and a period, comma, semicolon or
  colon after a closing tag still reads as the tag; a tag inside the text
  does not count. `git.work_records` sets the binding and journey rules to block or warn;
  frozen criteria always block. The output states that the check proves
  structure and binding only. An open task that changes product paths
  without a journey criterion gains one by a planning pull request, or the
  project sets `git.work_records: warn` while it catches up.

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
  gains an `id-registry` check, and the scaffold adds a
  `codeflow-registry` workflow that runs on `pull_request_target`. A project
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
  the PR sections its body omits; `codeflow ci --actor` passes the actor, the
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
  and PR-body standards move to a new `codeflow-policy` workflow on
  `pull_request_target`, so a pull request cannot edit the job that judges
  it. That job and the registry job check out the pull request's base
  commit, because GitHub's default checkout for the event is the default
  branch; a pull request into an integration branch is judged by that
  branch's pin and policy. Upgrade in this order: install the new binary,
  land a pull request that raises only `scaffold_version`, then run
  `codeflow update` on a new branch. Hook shims now warn when the `codeflow`
  on `PATH` is older than they are, and a policy with keys the binary cannot
  read names this order, including a pull request's own policy judged from
  the target branch. The `codeflow-registry` workflow installs its binary
  the same way.

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
- **Portable pull request checks.** `codeflow ci` reads Markdown sections,
  rejects explicitly empty PR bodies and ambiguous headings, and warns about
  summary detail, missing testing limits and oversized evidence. Generic release
  checks default to warn, with a project-owned breaking level and commit floor.
  Fresh installs include Reviews and Release impact in the required sections
  and ship the PR template at every tier. Without an explicit list, the
  built-in default stays Summary and Changes. Updates preserve existing policy
  values and customized templates. Upgrade order matters: `codeflow update`
  adds `git.pr_release_impact` and `git.pr_breaking_level`, and an older
  binary then fails every `codeflow ci` run with exit 2 and
  `unknown key git.pr_release_impact`. Upgrade the local and CI binaries
  first, then commit the policy change from `codeflow update`.

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
  managed `AGENTS.md` block is now a map of about 7 KB (was 28.7 KB at
  standard and full, 16.4 KB at minimal), rendered with `CLAUDE.md` from one
  kernel: at most 12 one-line always rules and a "when you are about to"
  table (estimate, status, explanation, plan, design, build, review,
  blocker, branch, ship, consult, instruction change, session start or
  resume), each pointing one hop away. The
  doctrine moved unchanged in substance to four references installed at
  every tier under `.codeflow/rules/` (workflow discipline, git rules,
  worktrees, writing). Always rules now include: durations for agent work
  come from cf-estimate, never human weeks, sprints or person-days; replies
  lead with outcomes in words, IDs after; complex explanations go through
  cf-present where the harness can show it; and orchestration entry is
  decided by the paths a task touches. The full tier gets its own map, which
  alone names `project-management/`. `codeflow update` replaces the managed
  block and keeps the project section byte for byte, CRLF line breaks and
  a missing final newline included; `doctor` gains an `instructions` check
  that warns when the `AGENTS.md` chain Codex loads for any directory, root
  to nested, passes its 32 KiB limit. Map rows print skill references as
  paths from the repository root (`.agents/skills/...`).
  Migration: `codeflow update` never edits the project section, so a
  project section that cites the old section names ("Git rules", "Worktree
  doctrine", "Workflow discipline", "Entry points", "Planning and tracking",
  "Session flow") should point at `.codeflow/rules/git-rules.md`,
  `worktrees.md` or `workflow-discipline.md`, or at the map, instead.

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
  mapping, and a missing Release impact there reads as no impact unless a
  commit is marked breaking. A path in your product or watched contract
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
  needs and moves trigger-only guidance, such as project model overrides,
  other hosts and parallel tasks, to references. Duplicated rules now live
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
- **Pull request classification and light planning paths.** With durable
  work tracking on, `codeflow ci` gives every pull request one class: tracked
  (`Task: TSK-NNN`, or the task id its branch carries), direct change
  (`Task: none: <reason>`), planning-only, an epic's integration line
  landing on the default target with only merges on it, or an automation
  profile, and blocks an unclassified one. The range is read as one diff
  from the merge-base, and tracking is read at the target as well as the
  head, so a pull request cannot classify itself lighter. A direct change is
  refused on policy, hooks, managed instructions, CI files, manifests, the
  record schema and the project's product code, named by the new
  `git.product_paths` policy key: `init` writes a default for the detected
  stack and `update` adds it once, keeping any project value;
  `git.direct_changes: forbid` refuses direct changes entirely. The planning
  anchor check runs at `work start` and in CI on every work prefix carrying a
  task id (`task/`, `fix/`, `feat/`, `spike/` and the rest) and no longer on
  every commit. A spike lands only `docs/research/` findings and its own
  record, and a pull request cannot claim the task record it adds. New
  `task new --follow-up-of`, `epic new --integration` and `adr new` (written
  `proposed`, the ADR template's new default). The shipped pull request
  template now carries the `Task:` line with a hint. Migration: a pull
  request from a `task/TSK-NNN-<slug>` branch is classified from the branch
  id and needs no change; any other pull request in a project with durable
  work tracking adds `Task: TSK-NNN` or `Task: none: <reason>` to its body.
  No policy key restores the old behavior: `git.direct_changes` only allows
  or forbids direct changes, and classification is off only where durable
  work tracking is off.

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
- **One full gate at a time, running the suite once.** Public behaviour
  change: `codeflow test --mode full` takes a gate lock before any target
  runs, and a second full gate on the machine refuses, naming the holder's
  pid, directory and start time. A killed gate's lock stays held while the
  targets it started are still running, and is reclaimed once they exit.
  On Windows the lock covers the gate process only; a target left running
  by a killed gate is not detected. Follow-up: a job object that ends the
  target tree with the gate.
  The locks are `locks/full-gate.lock` under the CodeFlow home
  (`CODEFLOW_HOME`, else `~/.codeflow`), which spans the machine, and
  `codeflow/full-gate.lock` in the repository's git common directory, which
  spans its worktrees; where one cannot be opened, as in a sandbox, the gate
  says so and holds the other. In every mode, a gate that runs cargo warns
  when `CARGO_TARGET_DIR` points outside the worktree, naming the shared
  directory: builds in parallel worktrees can overwrite each other's binaries
  there. The gate still runs; a shared directory to save disk stays valid.
  Each target now prints `[codeflow test] starting target '<name>' (<mode>
  mode)` on stderr as it starts, so a killed gate's log names the target it
  died in; stdout, the summary lines and the exit codes are unchanged, and
  targets skipped by `enabled` or `ci_skip` print nothing. CodeFlow's own
  full gate runs the Rust suite once, under coverage, plus
  `cargo test --workspace --doc`, which coverage skips.

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
- **Pull request template.** The shipped template has five fixed sections
  (Summary, Changes, Testing, Reviews, Release impact) with short comments,
  and lists its conditional sections with the exact condition for each. The
  Release impact block states `Breaking: yes | no` and always carries
  `Migration`. Existing policies are unchanged: the required headings are
  still Summary, Changes and, for code, Testing.

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
- **An approved spec is amended until it ships.** The lifecycle guidance,
  the spec template and the refusal of an approved spec moved back to
  `draft` now agree with how specs change in practice. While a spec is
  approved and not yet implemented, a change to it is amended in place
  through a reviewed planning change, with a dated note for each change of
  meaning and each bound consumer's disposition named; once implemented it
  is frozen and a change is a new spec. They no longer say that approval
  freezes the criteria. `codeflow update` brings the changed guidance and
  template.

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
