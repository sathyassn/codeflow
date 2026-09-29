# Harness permission presets: settled design

**Tasks:** TSK-171 to TSK-175 (EPC-020)

**Decision record:** ADR-0075 (proposed; accepted by TSK-175)

**Date:** 2026-09-28

**Status:** final design, revision 13. Reviewed by Fable, Codex (rounds 1
to 9) and Grok (rounds 1 to 6); the operator decided D1 to D10 on
2026-09-28 (workspace `RELEASE-PLAN.md`). Revisions 1 to 12, the review
reports and the saved harness documents cited below as CCD, GUG and CX are
kept in the planning workspace, not in this repository; the reference
configuration samples are in `evidence/permission-presets/` beside this
file.


Version 13, the last design pass. Versions 1 to 12 are kept unchanged
beside it.

**What this is.** v12 (this record) with the one item Codex round 9
left open completed, and round 9's implementation notes placed on the
criteria that own them. Round 9 confirms SET-R8-1 and SET-R8-3 fixed and
carries every non-blocking round 8 correction. **Where it stands.** This
closes the design; the units are filed from these drafts. v13 re-issues
units 2 and 5 (`task-draft-v13-unit-2.md`, `-5.md`) and the ADR; units 1,
3 and 4 stay at `task-draft-v12-unit-1.md`, `-3.md` and `-4.md`.

## Disposition of Codex round 9

| Item | Fixed in | Summary |
|---|---|---|
| SET-R8-2 (P2): an explicit push destination whose row is deleted | Proposal 9.2 (host record and paired test), 8.1 (one parsed tuple); unit 2 AC-23; unit 5 AC-8; ADR decision 8; `evidence/permission-presets/state/operator-actions.example.jsonl` | Exactly the completion Codex named. A workflow push whose exact source commit and resolved destination (push URL and ref) cannot be reconstructed from the branch's configured push destination is kept, as that tuple, in the host dependency record outside the editable queue until evidence for that destination shows the source reachable from its advertised tip, or the operator explicitly withdraws it; reconciliation uses that tuple and never substitutes the branch's current default. Paired control: an explicit `backup` push with a refspec, the whole row deleted, status still shows the exact tuple; pushing S there clears it. The marked-done and default-destination tests stay |
| Non-blocking round 8 corrections | Unchanged | Round 9 confirms all five themes carried or corrected: token override coverage, slash normalization and session identity, queue details and failed D8 recording, fixture prose and cleanup, version pointers |
| Note: one parsed tuple | Proposal 8.1; unit 2 AC-23; unit 5 AC-8 | Classification, queue entry, host record and reconciliation carry one tuple; the display command is never evaluated |
| Note: fold semantics | Proposal 9.3; unit 5 AC-8; sample | A row is the fold of its action line and later events, which carry only changed fields |
| Note: an unavailable other tip | Proposal 8.1 table; unit 2 AC-23 | Refused and not queued, naming `git fetch`; an inconclusive scan never creates an operator task |
| Note: the listing is point-in-time evidence | Proposal 8.1; unit 2 AC-23 | The guarantee is stated as of the listing; movement before the push is outside the check, with the token as the server-side backstop |
| Note: writer tests and "never opened for writing" | Proposal 9.1; unit 2 AC-25 | Tests run against the real host-side writer. Order of operations: a no-follow metadata read first, so a file known to fail is never opened; a hard-link race can at most open a file append-capable, which is closed with no byte written; a paired race test |

**Judged not to change:** everything else. This closes the design.

---

## 0. No-regression register additions (v10, unchanged)

| Settled | Source | How v10 honors it |
|---|---|---|
| The agent answers a folder-trust prompt itself when the folder is part of its task: a project worktree it works in, or a disposable sample its own harness created; any other folder goes to the operator | Workspace `AGENTS.md:306-311`, authorized by the operator on 2026-09-22 | v9's D7 grammar refused raw keys and every slash command, so a seat blocked on a trust prompt could only wait for the operator, which silently withdrew the authorization. v10 grants trust at launch through each seat's own flags for exactly the folders the authorization covers (1.5), so no prompt appears. Any other folder still goes to the operator, now through the operator-actions list (section 9). **One narrowing, flagged for the final confirmation:** a Claude seat in an evaluation fixture (a disposable sample the authorization covers) has no launch flag, and the documented key lives in `~/.claude.json`, which the sandboxed primary cannot write, so that trust moves into the operator's fixture-root step (section 8, D10). Until a Grok trust-only form qualifies, the Grok trust step is also queued |
| Hook-definition trust is the operator's | Proposal v8 1.6 (REC A5, C7) | Unchanged and kept separate from folder trust: launch-time folder trust never trusts hook definitions |

---

## 1. Seat readiness from guards that actually ran

### 1.1 What was wrong (v5, unchanged)

v4 let a seat count as ready when `session-orient` wrote a record at
SessionStart. Codex trusts each hook definition by its hash, and individual
hooks can be disabled (CX hooks, "Review and trust hooks"). A trusted
`session-orient` beside a changed or disabled `exec-guard` writes a fresh
record while the guard is skipped (SET-R3-1). The same happens after an
update that rewrites only the PreToolUse commands (R2-2).

### 1.2 The guard-readiness preflight and what the host accepts

**Naming.** This step is the **guard-readiness preflight**. The model
orchestrator already runs a seat preflight (availability, identity against
the pinned id, exclusions; E18 `cf-model-orchestrator/SKILL.md:177-212`, per REC R1).
The guard-readiness preflight runs inside it, before the identity check, and
a failure is one of that preflight's exclusions (1.7).

Readiness comes only from each required guard running in this seat's
session. The SessionStart record stays as context and is never the proof.

```
  host                              seat (fresh launch)
  ----                              -------------------
  launch seat, new nonce N
  wait for SessionStart record ---> session-orient writes {session, cwd}
  send the probe prompt ----------> model runs: codeflow guard-probe --nonce N
                                      git-guard, exec-guard see it: write a
                                      receipt each, allow
                                      the probe process runs and writes a
                                      marker .codeflow/probe/N.ran
                                    model edits .codeflow/probe/N
                                      (Codex, Grok): edit-guard writes a
                                      receipt and refuses the edit
  read receipts and marker; read the
  live hook entries once; compare
  receipt, live entries, approved
  identity and route (1.2); all
  match ---------------------------> write the readiness record from that
                                      same read (1.3), send the work brief
  otherwise: route unavailable,
  report which check failed
```

Caption: the guard-readiness preflight a host runs before the work brief. Only a receipt
written by a guard hook counts, and it counts only against the identity and
route that are current when the host accepts it.

**Required guards.** Claude: `git-guard`, `exec-guard` (Claude has native
Edit denies). Codex: `git-guard`, `exec-guard`, `edit-guard`. Grok: the same
three.

**A receipt** is written by the guard hook process, never by the probe
command, and holds: the nonce, guard name, contract number, binary version,
harness, the payload's `session_id` and `cwd`, the posture fields the
payload carries (Claude's `permission_mode`; for Codex and Grok, whichever
fields unit 3's pre-dispatch check finds, otherwise `unknown`), the
enforcement baseline identity the guard evaluated with, the binding state
of the path (section 2.3), a hash of the hook entries in the harness's
config files that invoke this guard, and **the admission tuple the hook
observed** in its own environment (`CODEFLOW_ADMISSION`: authority root,
enrolled root, baseline key), recorded as the value, `absent` or
`malformed`.

**The host accepts** only when, for this nonce, all of the following hold:

1. every required guard has a receipt, and records older than the launch
   are ignored;
2. all receipts carry the same `session_id`, equal to the SessionStart record
   of this launch;
3. `cwd` is the seat's worktree, and that worktree is **bound** to an
   enrolled repository (section 2.3);
4. the contract matches the installed binary's;
5. the baseline identity is the current approved identity of that
   repository;
6. **the hook entries match three ways** (SET-R5-3): at acceptance the host
   reads, once, the live effective entries that invoke each required guard
   in the harness config files the seat loads (project and user layer), and
   for each guard the receipt's entry hash, the live hash and the expected
   hash from the current approved identity must be equal. With approved H0,
   receipts written under H0 and the live file edited to H1 before
   acceptance, the live hash differs and the route is refused. A hook
   definition changed after approval, or between the receipts and
   acceptance, fails here;
7. **the receipt's harness equals the launched harness, and the posture
   equals the posture of the route's role** in the route table (1.5): for a
   builder, Claude `bypassPermissions`, Codex `never` with full access or
   `cf-builder` per D1, Grok `--always-approve` with `cf-guard-worktree`; for a
   consult or no-edit review, Claude `auto`, Codex `never` with `cf-guard`
   (D2), Grok `--permission-mode auto`; for Grok, the receipt's `cwd` is
   also the seat's own worktree, the directory its trust names. A builder posture on a consult or
   review route is a mismatch. Where the harness exposes no posture field,
   the receipt says `unknown`, the host compares the launch argv it
   recorded, and the report says the posture was not observed;
8. the marker `.codeflow/probe/N.ran` exists, which shows the command ran
   after the hooks allowed it, and no file exists at `.codeflow/probe/N`;
9. **every receipt's observed admission tuple equals the tuple the host set
   at launch** (SET-R6-2). An `absent`, `malformed` or different tuple in any
   required guard's receipt is never ready (`pin-missing`, `pin-mismatch`).
   If the harness cannot carry the pin to hooks on this route, the route is
   `pin-unsupported` until a separately qualified carrier exists; the host
   never substitutes a tool command's environment and never skips the check.
   The primary session is operator-launched and carries no pin; it keeps the
   v5 exception (nothing refuses the primary).

**A repository that is not enrolled** has no current approved identity, so
item 5 fails. The route is reported unavailable with "repository not
enrolled: the operator runs `codeflow update` or `codeflow baseline
approve`". A repository whose binding is lost fails item 3 with "binding
changed: the operator runs `codeflow baseline bind`". This is the explicit
readiness outcome for the legacy and loss routes (SET-R4-1).

**What makes a missing guard visible.** If a Bash guard is skipped, its
receipt is missing; the probe command still runs and prints which receipts
it cannot find, for the seat's own transcript. If `edit-guard` is skipped,
the edit succeeds and the probe file exists. If the model does not run the
probe, no receipts exist. Each case reports the route unavailable and names
the guard, with the remedy (trust the folder, re-trust changed hooks, enable
hooks, install the binary, approve the change).

**Every hook carries `--contract N`** (R2-2), `session-orient` and
`session-summary` included (`evidence/permission-presets/`). A contract bump changes every
hook definition's hash together, so Codex's re-trust covers them as one set;
the guard-readiness preflight still checks each guard, since a partial re-trust or a single
disabled hook remains possible.

**When the harness cannot establish it,** the route is reported unavailable,
not assumed ready: a hook payload with no session identifier, a seat that
cannot run a shell or edit tool, or a receipt location the hook cannot
write.

**The primary session** is launched by the operator. Its orient digest asks
the model to run the same probe at session start and reports the result to
the operator; nothing refuses the primary (v4 3.7).

**Expected result corrected** (v4 10.1 item 5): changing only a PreToolUse
command in Codex need not stop SessionStart; the guard-readiness preflight shows that
guard's receipt missing, or its entry hash different, and the route
unavailable.

### 1.3 Readiness holds at one point in time

Acceptance writes a **readiness record** under the CodeFlow state root: the
nonce, the repository's baseline key and current identity, the expected
entry hashes, the hashes of the harness config files the seat loads
(project layer and user layer), the policy hash in the identity, the
binding, and the binary's contract and version.

**The initial snapshot is the validated state** (SET-R5-3). The config-file
hashes in the record are the ones computed from the single read used for
item 6, never from a second read taken after it. After writing the record,
the host reads the files once more; if anything differs from the validated
read, the record is marked stale at once and no brief is sent. A change
during validation therefore produces stale or refusal, never a snapshot of
an unvalidated state.

Before every brief and every later prompt the host skills send, `delegate
wait` compares the record with the current state. Any difference makes the
record stale, and the host runs a fresh guard-readiness preflight with a new nonce before
the next prompt. The triggers:

- `codeflow baseline approve`, `codeflow update` or `codeflow baseline bind`
  for the same common dir, from any worktree. The current identity is shared
  by every worktree of the repository, so an approval in worktree A makes
  worktree B's readiness stale;
- a change to any harness config file the seat loads, or to its content in
  the approved identity;
- a ref transition in the seat's worktree that changes those files (2.2);
- a new binary version or contract.

**The validity boundary.** Readiness means that at the guard-readiness preflight every
required guard ran with the approved entries in the approved posture. It
does not see a change the harness keeps outside the files CodeFlow compares:
a per-hook trust toggle or disable in Codex (CX hooks, "Review and trust
hooks"), `disableAllHooks` in a local file the host does not re-read, or a
hook that later times out. Such a change is caught at the next guard-readiness preflight, not
continuously. Between guard-readiness preflights the guards still fail closed whenever they
run (D8), they always read the current baseline (section 2), and the git
hooks and CI stay below them. A seat already working continues; its next
prompt waits for the fresh guard-readiness preflight.

### 1.4 What a receipt shows, by route

| Route | Where the hooks run | Receipt location | Seat can write it | Assurance |
|---|---|---|---|---|
| Claude seat, sandboxed, builder or consult | host, outside the sandbox (CC hooks) | state root `probe/` | no: outside the preset's `allowWrite` | guard receipts |
| Codex under `cf-guard` or `cf-builder` | host (CXS `hooks/src/engine/command_runner.rs:209-221`) | state root `probe/` | no: outside the writable roots | guard receipts |
| Codex under full access (D1, until its spike passes) | host | state root `probe/` | yes: no sandbox | cooperative |
| Grok under D3 | inside Grok's sandbox (GKS `xai-grok-hooks/src/runner/command.rs:199`) | `~/.grok/codeflow-probe/` | yes: D3 writes the cwd, `~/.grok` and temp (GUG `18-sandbox.md`) | cooperative |
| Grok consult or review, `--permission-mode auto` | no sandbox on this route | `~/.grok/codeflow-probe/` | yes: no sandbox | cooperative |

Caption: where a forged receipt is possible. "Guard receipts" holds only
while the sandbox holds.

**Cooperative readiness evidence.** On the two seat-writable routes, the
receipts show that the guards ran for a seat that followed the guard-readiness preflight
prompt. They do not prove it. A seat whose guard was skipped can write that
guard's receipt and the `.ran` marker, and leave no probe file, and the
acceptance predicate cannot tell the difference. The possible writers are
wider than a shell write: a file-edit tool the guards do not cover, an MCP
write tool (unguarded; `codeflow doctor` lists enabled MCP servers), or a
direct run of a guard binary with a fabricated payload on stdin. exec-guard
refuses a shell call that runs `codeflow hook <name>` directly, since only a
harness invokes a hook. That removes the plain form, not an interpreter or
binary that does the same.

The host says so: `delegate wait` reports `ready (guard receipts)` or
`ready (cooperative: receipts are seat-writable on this route)`, and the
delegate provenance records the assurance. A cooperative route is still
briefed, since D1 and D3 are decided; the ADR states the residual. D1's
`cf-builder` spike, if it passes, moves Codex builders to the first form.

### 1.5 Postures per route (restored)

Consults and no-edit reviews stay non-bypass (settled: ADR-0055 decision 3,
E18 `docs/decisions/ADR-0055-...md:44`, per REC L2; S1 and S13 in the v2 register). v7
listed one Grok posture and so dropped the Grok reviewer row that v4 kept
(`proposal-v4.md:836`; REC L2, C3). v8 restores it, and checks that every
harness has both postures in readiness (1.2 item 7) and here.

| Route | Role | Posture | Readiness | Decision |
|---|---|---|---|---|
| M5 Claude seat | builder | `--permission-mode bypassPermissions --settings <state>/settings.json`, sandboxed | guard receipts | S1 |
| M5 Claude seat | consult, no-edit review | `--permission-mode auto --settings ...`, sandboxed | guard receipts | S1 |
| M4 Codex seat | builder | `--ask-for-approval never --sandbox danger-full-access`; after the D1 spike, `never` with `cf-builder` from the main root; both with `-c 'projects."<dir>".trust_level="trusted"'` naming the directory the seat opens | cooperative under full access; guard receipts under `cf-builder` | D1 |
| M4 Codex seat | consult, no-edit review | `--ask-for-approval never`, no `--sandbox`, so `cf-guard`; with the same trust override | guard receipts | D2 |
| M6 Grok seat | builder | the trust step in the seat's own task worktree (below); then, launched in that same worktree, `--always-approve --sandbox cf-guard-worktree --trust` | cooperative | D3, D7 |
| M6 Grok seat | consult, no-edit review | the trust step in the worktree it reviews; then, launched in that same worktree, `--permission-mode auto --trust` | cooperative | D3, D7, S13 |
| M3 Codex plugin | review, rescue | plugin-set `never` with `read-only` or `workspace-write`, through the one retry or inside Claude's sandbox after spike 0 | not a seat: no guard-readiness preflight | ADR-0029 as amended |

Caption: every harness has a builder posture and a non-bypass consult or
review posture. Whether Grok `auto` prompts on a dangerous call, or on the
guard-readiness preflight's probe edit, is unit 5 AC-6's row; if it does,
that route is excluded with the reason, not moved to `--always-approve`.

**Grok seats open their own worktree** (Grok round 5). Grok's folder
trust covers subdirectories of the same repository but not a nested
checkout, which is a separate workspace (GUG `10-hooks.md:81`), and the
project hooks Grok loads are those of the folder it opens. v10 launched
Grok builders from the main root and trusted the task worktree, so the
trust named one directory and the seat opened another, and the hooks came
from the main checkout's branch rather than the task's. v11 launches every
Grok seat in its own task worktree, and the trust step names that same
directory.

The reason v6 launched from the main root was write access: under the
`workspace` base the sandbox writes only the working directory, `~/.grok`
and temp (GUG `18-sandbox.md:29`, `:40`), and a linked worktree keeps its
index, `HEAD` and objects in the common `.git` of the main checkout. So
builders get a second profile, `cf-guard-worktree`, equal to `cf-guard`
plus `read_write = ["../../.git"]` and `read_only = ["../../.git/hooks"]`,
which for the `.worktrees/<slug>` layout (ADR-0055 decision 5) is the
common git dir (`evidence/permission-presets/grok/sandbox.toml`). The write set is the
same as v10's main-root launch gave: the common `.git`, with its hooks
read-only and `core.hooksPath` still checked by the guard. Whether Grok
resolves a relative `read_write` entry containing `..` against the
working directory is not documented (`read_write` entries are "literal
directory grants", GUG `18-sandbox.md:105`; the sample uses absolute
paths) and is a unit 5 AC-2 row. If it does not, the Grok builder route is
recorded unsupported in linked worktrees and Grok stays a consult and
reviewer there; that fallback changes where D3 applies, so it is
reported to the operator rather than chosen silently. Consult seats need
no profile change: they run without a sandbox, in `auto` mode.

**Folder trust at launch** (D7 as decided; the 2026-09-22 authorization,
section 0). A seat never waits on a folder-trust prompt, because its own
launch establishes trust for its own task folder:

| Harness | How trust is granted at launch | Source |
|---|---|---|
| Grok | a separate trust step in the exact directory the seat opens, which is its own task worktree for builders and consults alike, before the sandboxed launch, since under `cf-guard` the sandbox write-protects `~/.grok/trusted_folders.toml` and a trust accepted inside it lasts for that session only. `--trust` on the launch line is kept, for the session. A linked worktree is its own checkout, and Grok's trust does not cover a nested checkout, so each seat worktree is trusted on its own (inferred from the nested-checkout rule, unverified for linked worktrees) | GUG `18-sandbox.md:57`, `10-hooks.md:79-81` |
| Codex | the launch-time config override `-c 'projects."<seat worktree>".trust_level="trusted"'`; CLI overrides take precedence over config files, and the `projects` trust table is a documented key | CCD `codex2-config-file-config-basic.md:23`, `:39`; `codex2-config-file-config-sample.md:1120-1126`; `codex2-config-file-config-basic.md:37` |
| Claude | no launch flag exists (the CLI reference lists none; `--bg` only checks trust). None is needed for a seat worktree: in a worktree, Claude keys trust on the main checkout's root, so a seat in `.worktrees/<slug>` inherits the trust the main checkout already has. The gap is a disposable sample that is its own repository, such as an evaluation fixture (D10): the only documented grant outside the dialog is the per-folder key `projects["<path>"].hasTrustDialogAccepted` in `~/.claude.json`, which the sandboxed primary cannot write. v10 puts those keys in the operator's fixture-root step (section 8, D10), written once for the fixtures that step creates and queued on the operator-actions list | CCD `cli-reference.md:74`; `permissions.md:659-661`, `:666`, `:697`; `hooks.md:3768` |

Caption: launch-time folder trust per harness. It covers only the folders
the 2026-09-22 authorization covers. Hook-definition trust (1.6) is never
granted this way.

Trust matters for enforcement, not only convenience: Claude holds back every
settings-file hook until the folder is trusted (CCD `hooks.md:3768`), and an
untrusted Codex project loads no project-local configuration, so no
`cf-guard` profile, rules or hooks (CCD `codex2-config-file-config-basic.md:37`).
None of these launch forms has been run. The Codex override's effect on the
trust prompt and on project-config loading, and the Grok trust step's exact
form, are unit 5 AC-2 rows. On the Grok form: the user guide documents
`--trust` (GUG `10-hooks.md:79`, `18-sandbox.md:57`), but `grok --help` of
the installed 1.0.41 does not list it (run for v10), so it may be an
undocumented flag or may have moved. The trust step is also interactive as
documented: `grok --trust` starts a session, and the D7 grammar gives the
primary no way to end one except by the seat finishing. Unit 5 AC-2
qualifies a trust-only form that saves trust and exits without a model turn
(for example `grok --trust inspect --json`, unverified); D7's list carries
that one form as a pending entry. Until a form qualifies, the Grok trust
step is an operator action per seat worktree, queued, and the Grok route is
excluded in that worktree with `hooks-untrusted` meanwhile. The guard-readiness preflight never assumes a
flag worked: it judges trust by its effect, the project hooks' receipts, and
reports `hooks-untrusted` or `guard-missing` otherwise. A trust prompt that
still appears is never answered with keys; it goes to the operator-actions
list (section 9).


### 1.6 Hook trust belongs to the operator (REC A5, C7)

Codex skips a hook definition until it is trusted, and records trust by
hash (CX hooks, "Review and trust hooks"). An agent that answered that
review would decide which guards run. So agents never approve hook trust:

- exec-guard and edit-guard refuse writes to Codex's hook-trust store and
  to the user-level `~/.codex/config.toml` and `~/.codex/hooks.json`. Where
  Codex keeps hook trust is not verified; unit 3's pre-dispatch check finds
  it, and until then the refusal covers the user-level Codex config files;
- the Herdr grammar refuses `agent prompt` text that starts with `!`, and
  every slash command except `/compact`, `/clear` and `/new`, so `/hooks`,
  `/permissions`, `/approvals`, `/model` and any other command that changes
  permissions, approvals, trust or the model stay refused (unit 4 AC-6);
- a readiness failure for untrusted or changed hooks names the operator as
  the remedy ("the operator reviews and trusts the hooks in Codex");
- folder trust is a different thing (section 0, 1.5). It is granted at
  launch through the seat's own flags for the seat's own folder, which also
  keeps the Codex route clear of the refusal above: the `-c` trust override
  is a launch argument and writes nothing to `~/.codex/config.toml`. Folder
  trust never trusts a hook definition; Codex still skips an untrusted or
  changed hook, and readiness reports it as `hooks-untrusted`;
- the permitted slash commands are context housekeeping only, and none
  changes permissions, approvals, trust or the model:

  | Command | Claude | Codex | Grok |
  |---|---|---|---|
  | `/compact` | documented (CCD `desktop.md:341`) | documented (CCD `codex2-developer-commands.md:402`) | documented (CCD `grok-modes-and-commands.md:40`) |
  | `/clear` | documented for the web client (CCD `claude-code-on-the-web.md:237`); CLI not in the saved docs | documented (CCD `codex2-developer-commands.md:398`) | alias of `/new` (CCD `grok-modes-and-commands.md:32`) |
  | `/new` | not in the saved docs (unverified) | documented (CCD `codex2-developer-commands.md:428`) | documented (CCD `grok-modes-and-commands.md:32`) |

  Caption: the three permitted commands per harness. Herdr's check is the
  same for every harness; a command a harness does not know does nothing
  there. The match is exact on the command word, so `/compact keep the
  test output` passes and `/compact-mode` (Grok, a UI toggle) does not.

### 1.7 Readiness failures as exclusion reasons (REC R2, C6)

A guard-readiness failure is a fresh native exclusion of that harness route
in that worktree, recorded with its remedy. It is never a model exclusion.
`delegate wait` reports one of these reasons:

| Reason | Remedy |
|---|---|
| `guard-missing:<guard>` | trust the folder, re-trust or enable hooks, install the binary |
| `hook-entry-mismatch` | the operator approves or reverts the hook change, then re-trusts it |
| `hooks-untrusted` | the operator reviews and trusts the hooks (1.6) |
| `posture-mismatch`, `harness-mismatch` | relaunch with the route's posture (1.5) |
| `not-enrolled` | the operator runs `codeflow update` or `codeflow baseline approve` |
| `binding-lost` | the operator runs `codeflow baseline bind` after checking |
| `excluded-repository` | the operator adopts it, or the work moves |
| `fixture-only` | the route is valid for evaluation fixtures only (D10) |
| `stale` | run a fresh guard-readiness preflight |
| `contract-mismatch` | install the matching binary |
| `pin-missing`, `pin-mismatch` | relaunch the seat with the host's admission pin |
| `pin-unsupported` | the route stays unsupported until a qualified pin carrier exists |

Caption: the exclusion vocabulary for guard readiness. The catalog's own
vocabulary and ADR-0069's note live on EPC-018 (REC R2; task N1); unit 5
uses these names in the skill text.

---

## 2. A trusted enforcement baseline

### 2.1 What was wrong (v5, unchanged)

v4's drift check compared the working copy with `HEAD` and read the policy
from `HEAD`. `HEAD` is mutable: a blob staged without touching the working
file passes the check, a plain commit moves it into `HEAD`, and the guard
then reads the relaxed policy (SET-R3-2). Fable's round 2 found the opposite
cost: v4 refused every call in a fresh project with an unborn `HEAD` and
while the operator's own update sat uncommitted (R2-3).

### 2.2 The baseline

The guards take their authority from an **enforcement baseline** outside
the repository that only the operator advances. Nothing is refused because
the working copy differs; the added rules police what could move the
baseline, the files git runs, or the route to the baseline.

**What the baseline holds.** One file per repository, keyed by the real path
of the common git directory so every worktree shares it, under the CodeFlow
state root (`baselines/<key>.json`). It records a list of approved
**identities**, each a map from enforcement path to its mode and blob hash,
or "absent", with the approving command, time and **the contract it was
approved under**; one identity is current, and its `.codeflow/policy.json`
and `.codeflow/project.toml` content is stored in full. Enforcement paths:
`.claude/settings.json`, `.codeflow/policy.json`, `.codeflow/project.toml`,
`.codeflow/git-hooks/*`, `.codex/config.toml`, `.codex/hooks.json`,
`.codex/rules/*`, `.grok/hooks/*`, `.grok/sandbox.toml`,
`.github/workflows/codeflow-ci.yml`.

**Who advances it.** Only a process outside every agent sandbox: `codeflow
init` and `codeflow update` record the identity they write, `codeflow
baseline approve` records the current index's identity after the operator
reviews it, each binding the root in the same operation, and `codeflow
baseline bind` records a binding, an inventory or an exclusion (2.3). From an
agent session, exec-guard refuses all four. The OS keeps agents out where a
sandbox holds: the state root is outside the Claude preset's `allowWrite`,
outside Codex's writable roots under `cf-guard` and `cf-builder`, and outside
Grok's D3 write set. Codex builders under full access and Grok without D3
have only the text refusal: a stated residual that D1 and D3 close.

**How the guards use it.**

- **Authority.** git-guard and exec-guard evaluate the policy stored in the
  current identity of the repository the path is bound to. They never read
  the policy from `HEAD`, the index or the working copy of an enrolled
  repository.
- **Commits: every tree that will be committed.** Before a command that
  creates a commit, git-guard computes the enforcement entries of each tree
  it will commit, in the target worktree:
  - `commit` in every form, including `--amend`, `-a` and pathspecs: the
    target worktree's index, or the file `GIT_INDEX_FILE` names, plus the
    working entries `-a` or a pathspec adds;
  - `merge`: the result tree, computed before the merge with `git merge-tree
    --write-tree` (`git-merge-tree(1)`), which touches neither the index nor
    the working tree;
  - `rebase`, and `cherry-pick`, `revert` or `am` over a range: each
    replayed commit. It is allowed when no commit in the range changes an
    enforcement path and the base tree's entries are approved, or when each
    intermediate result equals an approved identity. Approved endpoints
    cannot hide an unapproved step;
  - `--continue` after a conflict (`merge`, `rebase`, `cherry-pick`,
    `revert`, `am`): the index at that moment, like a commit;
  - a committing `stash`: its index and working trees;
  - `pull`: refused in agent sessions. Git fetches first and then
    integrates (`git-pull(1)`, "Description"), and the harness sends no
    separate tool event for the integration, so the guard cannot check the
    fetched tip before it lands. The refusal names the replacement: `git
    fetch <remote>`, then `git merge --ff-only <remote>/<branch>` or `git
    rebase <remote>/<branch>`, each checked; the skills say the same (unit
    5 AC-3). This is an in-session guard rule. A human terminal is
    unaffected: the git-hook plane does not police it, and existing git
    hooks apply as before.
  - **`codeflow integrate <branch> <target>`** rebases the branch onto the
    target, runs the test gate and fast-forwards the target with
    `update-ref`, all in child processes the hooks never see (E20
    `crates/codeflow-core/src/integrate.rs:1-13`, `:245-262`). git-guard
    therefore classifies the `codeflow integrate` command itself as a replay
    plus a ref move: it runs the merge pre-check (below) over the branch's
    commits and requires every replayed tree, and the target's resulting
    tree, to equal an approved identity, exactly as for `git rebase`;
  - **`gh pr merge`** lands server-side, where no local check runs. It is
    classified by the mode it actually lands in (2.2.1, SET-R7-3): allowed
    only when no tree it can land changes the base's enforcement entries,
    and otherwise refused with the operator's route.

  **Landing enforcement changes through integration pull requests.** A
  local merge by the primary (`git merge --no-ff` or `codeflow integrate`)
  whose result changes an enforcement path is refused with the operator's
  step: "the operator approves this landing with `codeflow
  baseline approve --merge <source> --into <target>`". That operator command
  computes, with the same pre-check and sanitized query, every tree the
  landing will create (the merge result, or each replayed tree for
  `integrate`), shows each enforcement diff, and records those identities
  in an approval bound to the repository (its baseline key), the source ref
  and commit, the target ref and commit, and the method (`merge --no-ff`,
  `merge --ff-only`, `integrate`, `rebase`). At the landing, git-guard
  re-reads all four; if the source or target moved, or the method differs,
  the approval does not apply and the landing is refused and queued for a
  fresh approval. The primary's merge then passes, and its push of the
  target is checked against the approved result. This is how units 1 to 3 land on E20 once
  unit 2's binary is installed and the repository enrolled: each of their
  pull requests changes `.codeflow/policy.json`, the settings or the hook
  files, so each landing takes one operator approval, queued on the
  operator-actions list (section 9). Before
  unit 2's binary is installed, the current guards apply and nothing changes.

  **2.2.1 Server-side landing (SET-R7-3).** `gh pr merge` lands in one of
  several modes, and only some of them land what the guard can see when
  the command runs (gh 2.67.0, `gh pr merge --help` lines 6-9, 16, 18,
  23-26):

  | Mode | What lands on the base, and when | Rule in an agent session |
  |---|---|---|
  | `--merge`, immediate | one merge commit, its tree computed by GitHub from the base as it is at that moment and the head; the head's commits become reachable through the second parent | allowed under the invariant below, else refused |
  | `--squash`, immediate | one commit with the tree GitHub computes | allowed under the invariant, else refused |
  | `--rebase`, immediate | each head commit replayed as its own commit on the base, so every intermediate tree lands | allowed under the invariant, else refused |
  | `--auto` | later, when requirements pass; the head and base may both move first | refused as unsupported: it cannot be validated where it lands |
  | a base that requires a merge queue (any flags without `--admin`) | later, in a merge group GitHub builds from the base and the entries ahead; with checks pending, gh enables auto-merge instead (help lines 6-8) | refused as unsupported |
  | `--admin` | immediately, bypassing the base's requirements and queue (help lines 9, 16) | refused: bypassing branch rules is the operator's |
  | no method flag on a base without a queue | gh asks interactively or fails | refused: mode unknown |
  | `--disable-auto` | removes a pending deferral | allowed |
  | `gh api` merge, enqueue or auto-merge mutations | as the mode they name | refused in agent sessions (`gh api` with a write method is already refused, unit 2 AC-2); the refusal names `gh pr merge` |

  Caption: `gh pr merge` by landing mode. Whether a base requires a queue
  comes from a bounded lookup of the base branch's rules; whether the pull
  request would merge now comes from its merge state (`gh pr view`), and
  anything but a mergeable-now state is treated as deferred. On any failed
  or ambiguous lookup the guard refuses.

  **The invariant for an immediate server-side landing.** Let E(t) be a
  tree's enforcement entries. With the base tip B and the head H read at
  the command, and both present locally (fetched; otherwise the refusal
  names `git fetch`), the landing is allowed only when:

  1. the command pins the head: `--match-head-commit H`, equal to the H the
     guard read, so GitHub refuses the merge if the head moved;
  2. every commit in `B..H` has the same E as each of its parents, so no
     commit on the pull request changes an enforcement path, even one a
     later commit reverts;
  3. E(H) = E(B).

  Together these mean E is constant from the merge base to H and equals
  E(B). Whatever tree GitHub builds, by merge, squash or replay, and
  wherever the base has moved by the time it lands, the enforcement
  entries it carries are the base's own at that moment, which were
  approved when they landed. The rule is the server-side form of the
  local replay rule above ("allowed when no commit in the range changes
  an enforcement path"); it needs no approval, and no approval can extend
  it.

  **When the invariant fails**, `gh pr merge` is refused in every mode, and
  the refusal names the operator's route, queued for the operator: the
  operator runs `codeflow baseline approve --merge <source> --into
  <target>` and the primary lands locally (`git merge --no-ff` or
  `codeflow integrate`) and pushes the target, which the approval's
  binding checks; where the base's rules do not accept a direct push (a
  pull request requirement or a merge queue), the operator merges the pull
  request on GitHub. A server-side landing is never covered by a local
  approval, since the tree it lands is not the tree the approval computed.

  | Case | Expected |
  |---|---|
  | A pull request whose first commit changes `.codeflow/policy.json` and whose second reverts it (A to B to A), `gh pr merge --rebase --match-head-commit H`; the same with `--merge` and `--squash` | refused in all three (item 2), naming the operator route |
  | an approval for source S1 into target T1 by `merge --no-ff`; S moves to S2, then the primary's `git merge --no-ff` | refused: source changed; queued for a fresh approval |
  | the same approval; T moves to T2 | refused: base changed |
  | the same approval; the primary lands with `codeflow integrate` instead | refused: method differs |
  | `gh pr merge 12 --squash` without `--match-head-commit`; with a stale H | refused; refused |
  | `gh pr merge 12 --auto --squash --match-head-commit H` on an unchanged-enforcement pull request; a base with a merge-queue rule (fake service) | refused as unsupported, queued for the operator; the same |
  | `gh pr merge 12 --admin --squash --match-head-commit H` | refused |
  | an unchanged-enforcement pull request, mergeable now, no queue, `gh pr merge 12 --squash --match-head-commit H` | allowed |
  | the same pull request with B or H not fetched | refused, naming `git fetch` |

  The entries must equal an approved identity that is compatible with the
  current contract, compared by path, mode and blob, including additions and
  deletions. This is a check of state, so an index changed by any means,
  including a library in an interpreter, is caught at the next commit.
- **Ref transitions.** `checkout`, `switch`, `restore --source`, `reset`,
  `worktree add`, `stash apply` and `stash pop` are allowed when the target
  tree's enforcement entries equal an approved identity that is compatible
  with the current contract. **Correction of v5 line 190:** a transition
  replaces every enforcement file on disk with that identity's content:
  harness hook definitions, deny rules, sandbox profiles, the Codex and Grok
  config and the git-hook scripts, not only the git hooks. The policy
  authority stays the current identity, but which hooks a harness runs can
  change. Therefore:
  - an identity is **compatible** only when, for each harness, its hook
    config holds every required guard entry in the fail-closed form with the
    current `--contract`. `codeflow baseline approve` checks this at
    approval. A contract bump makes older identities incompatible until
    **requalified**: the tree is brought to the current contract (a `codeflow
    update` there) and approved again. "Once approved" never means that an
    obsolete or missing guard entry satisfies current readiness;
  - a transition that changes a harness config file in a worktree makes the
    readiness of seats there stale (1.3);
  - anything else is refused with "the operator approves this tree
    (`codeflow baseline approve --tree <ref>`) or checks it out". Reading an
    older tree needs no transition: `git show <ref>:<path>`, `git log -p` and
    `git diff` stay available.
- **Plumbing and routing, classified by effect** (unit 2 AC-16). Refused from
  agent sessions when the command:
  - writes live enforcement files: `checkout-index`, `read-tree -u`, `apply`
    or `apply --index` with a patch that touches an enforcement path;
  - writes the index or refs directly: `update-index`, `read-tree`,
    `update-ref`, `symbolic-ref`, `commit-tree`, `write-tree`, `replace`,
    `filter-branch`, `fast-import`, `apply --cached` touching an enforcement
    path, and `fetch` whose resolved destinations include `refs/heads/` or
    `HEAD`, or with `--update-head-ok` (`git-fetch(1)`). **Destinations
    are resolved in git's precedence** (SET-R6-1; `git-fetch(1)`,
    "Configured remote-tracking branches" and `--refmap`):
    - with no source on the command, the effective `remote.<name>.fetch`
      values (and `remote.<name>.mirror`) are the refspecs, so their
      destinations are the destinations;
    - with sources on the command, the destinations are the **union** of
      the command's explicit destinations and the destinations that the
      mapping gives the selected sources, where the mapping is
      `remote.<name>.fetch` unless `--refmap` is given, in which case the
      `--refmap` values replace it; an empty `--refmap=` removes the
      configured mapping, leaving only the explicit destinations.

    So with `remote.origin.fetch = +refs/heads/*:refs/heads/*`, both `git
    fetch` and `git fetch origin topic` are refused, since the configured
    mapping updates `refs/heads/topic` even when the command names no
    destination; `git fetch --refmap= origin topic` is not refused on that
    account, but `git fetch --refmap= origin topic:topic` is, for its
    explicit destination;
  - can change the route to the baseline: `init` inside a bound root,
    including `--separate-git-dir` (`git-init(1)`), `worktree move`,
    `worktree repair`, and `config` writes of `core.worktree`, `core.bare`,
    `core.hooksPath`, `extensions.worktreeConfig`, `include.*` or
    `includeIf.*`. File writes to a worktree's `.git` pointer and to
    `commondir` and `gitdir` files join the enforcement write paths (unit 2
    AC-4), though they are not part of an identity.

  A git subcommand the guard does not classify is refused from agent
  sessions in a bound repository, naming the operator. Ordinary `fetch`,
  whose resolved destinations are objects, tags, `FETCH_HEAD` and
  remote-tracking refs, stays usable.
- **Hooks path, from effective config.** Every call resolves the effective
  `core.hooksPath` for the command in its target worktree, by asking git with
  the command's own `-c` and `--config-env` flags and `GIT_CONFIG_*`
  environment, so git applies case folding, includes and worktree config
  (`git-config(1)`, "ENVIRONMENT"), and refuses a value that is not the
  shipped `.codeflow/git-hooks`. The existing guard already detects the text
  forms (E20 `crates/codeflow-core/src/hooks/git_guard.rs:356-363`,
  `:578-583`, `:616-628`, `:2745-2790`); that is useful, and it is not
  evidence that the new query shares it.
- **Working copy.** A working-copy difference is reported, not refused.
  Agents' writes to those files are refused before they happen (Edit denies
  and the Claude sandbox, Codex `read` entries, Grok `read_only`, edit-guard,
  `hook_integrity`), and the operator's own uncommitted edit does not stall
  agent sessions (R2-3).
- **The direction of a change is not judged.** Any identity the operator has
  not approved is refused, stricter or looser.

**The guard's own git queries (SET-R5-1).** Claude and Codex hooks run on
the host, outside the seat's sandbox, against a repository the seat can
write. **Invariant: host-side inspection runs no program the repository
selected.** v6 called the queries read-only and turned off fsmonitor, the
pager, textconv and external diff. That missed the merge query: a content
merge runs a custom merge driver, a command defined in config and selected
by the `merge` attribute or `merge.default` (`gitattributes(5)`, "Defining
a custom merge driver"; Codex round 5 cites git v2.53.0 `merge-ll.c:225-227`
starting it through a shell), and a defined filter runs its clean or smudge
program when renormalization applies. An agent can change an ordinary
driver script without touching an enforcement file.

```
  git command that merges (merge, rebase, cherry-pick, revert, am,
  stash apply/pop, checkout -m, and each --continue)
    |
    v
  pre-check, running no program: effective config (all scopes,
  includes, the command's -c and environment) and the attributes of
  every path the merge changes (check-attr from worktree, index and
  each tree involved)
    |
    +-- custom merge driver selected, merge.default not built-in,
    |   a defined filter on a changed path, or renormalization on
    |   --------------------------------------------> refuse the COMMAND
    v
  candidate merge query in a guard-private temporary repository:
  objects borrowed through alternates, config = validated non-program
  keys only, system and global config off, attributes = the validated
  built-in merge selections only, fixed PATH, private HOME, no other
  GIT_* variable
    |
    v
  enforcement entries of the result tree --> approved identity check
  temporary repository and its new objects discarded
```

Caption: the merge query. The real command is refused when the repository
selects a program, so the query never needs to disable one.

- **Refuse the command, not only the query.** The pre-check refuses the
  guarded git command itself, with the route "run this merge in a human
  terminal, or remove the driver or filter". Drivers are never disabled only
  for inspection, which could validate a tree the real merge would not
  produce. Config and attribute reads run no program.
- **What is built in, precisely** (Codex round 6, non-blocking). Drivers
  are resolved by their effective definition, never by name:
  - an **unset** `merge` attribute means `-merge` (take the current side
    and mark a conflict), a built-in; it is distinct from **unspecified**
    (`!merge` or no attribute), which selects `merge.default` when that is
    set (`gitattributes(5)`, "Performing a three-way merge");
  - a **set** `merge` attribute selects the built-in text driver;
  - a string `merge=text`, `merge=binary` or `merge=union`, and a
    `merge.default` of one of those names, is built in only when the
    effective config defines no `merge.<name>.driver` of that name. Git
    looks up user-defined drivers before the built-in names (Codex round 6
    cites git v2.53.0 `merge-ll.c:341-369`), so a same-name custom driver
    is a custom driver and the command is refused;
  - the `binary` macro expands to `-diff -merge -text` (`gitattributes(5)`,
    "Defining macro attributes"), so paths marked `binary` merge with the
    built-in unset behavior. CodeFlow marks its compressed `cf-present`
    payloads this way (E20 `.gitattributes:9-10`, `*.br` and `*.gz`), so
    ordinary merges touching them pass the pre-check; conflicting binary
    content still stops as a conflict.

  Attribute selection is evaluated through git's own resolution
  (`check-attr`), so every source counts: in-tree `.gitattributes` at any
  depth, `$GIT_DIR/info/attributes`, `core.attributesFile` and the system
  file (`gitattributes(5)`).
- **The query cannot run one.** It runs in a guard-private temporary
  repository whose `objects/info/alternates` borrows the repository's
  objects (`gitrepository-layout(5)`), with `GIT_CONFIG_NOSYSTEM=1`,
  `GIT_CONFIG_GLOBAL=/dev/null`, a config holding only the validated keys
  that affect the merge result and name no program (rename detection and
  similar), `GIT_ATTR_SOURCE` set to the empty tree (`git(1)`) plus an
  `info/attributes` holding the validated built-in `merge` selections of the
  changed paths, `core.fsmonitor` off, a fixed system `PATH`, a private
  `HOME`, and no other `GIT_*` variable. No driver or filter definition
  reaches it, even if the repository changes between the pre-check and the
  query. Because the pre-check has refused every program selection, the
  sanitized query computes the same enforcement entries as the permitted
  command. Replay steps use `--merge-base` (`git-merge-tree(1)`).
- **Wording corrected.** `merge-tree --write-tree` writes tree and blob
  objects (`git-merge-tree(1)`). In the temporary repository they are
  written there and discarded with it, so the repository's object store is
  unchanged. The other queries (`rev-parse`, `ls-files -s`, `ls-tree`,
  `config --get`, `check-attr`) write nothing. None of the queries changes
  an index, a ref or a working tree.
- **Stated residual.** A change between the guard's check and the git
  command is the time-of-check residual of every state check here. The
  command runs inside the seat's sandbox, and the next commit or transition
  is checked on state.
- **Autonomy cost.** An agent merge whose changed paths use a custom driver
  or a defined filter (Git LFS paths, for example) is refused, and the
  operator runs it. Whether this workspace uses such drivers was not
  checked.

**Cases Fable raised, and the operator's paths.**

- **Bootstrap.** `codeflow init` is an operator bootstrap, run outside every
  agent session. It records the scaffolded identity and binds the root, so
  the first agent commit, whose index equals it, passes; an unborn `HEAD`
  needs no special case, since approval does not depend on an existing
  commit. Agent-run init is refused.
- **Update.** After `codeflow update`, the new identity is approved and
  current; the operator commits when convenient. Working differences are
  diagnostic, and committing the complete approved identity passes.
- **A mixed staged identity.** If the index holds some enforcement files from
  the new identity and some from the old, it matches no identity and the
  commit is refused on purpose. The refusal lists, per enforcement path,
  which approved identity its staged entry matches, and names the two ways
  out: stage every enforcement path from the working copy (`git add --
  .claude/settings.json .codeflow .codex .grok
  .github/workflows/codeflow-ci.yml`), or unstage them all (`git restore
  --staged -- <paths>`), then commit. Changed hook definitions still need
  re-trust in Codex and a fresh guard-readiness preflight (1.3).
- **The workspace file** the operator applies by hand (S19) is approved with
  `codeflow baseline approve` before commit.

### 2.3 Binding a worktree to its repository (SET-R4-1, SET-R5-2)

v5 found the baseline by the common dir that git reports for the current
path, and read a lookup miss as "installed before this release". The address
was not bound: a changed `commondir` in a linked worktree (a writable file
under `cf-builder`'s `.git` grant, `evidence/permission-presets/codex/config.toml:132`), a
relocation with `git init --separate-git-dir` (`git-init(1)`), or
`GIT_DIR`, `GIT_COMMON_DIR` or `--git-dir` pointing elsewhere (`git(1)`,
"ENVIRONMENT"; `gitrepository-layout(5)`, "commondir") reaches a common dir
with no baseline, and the guard then read that repository's own policy. v6
bound the address; round 5 found that the store itself also needs a fixed
location and a rule for failed reads.

**The authority root.** The state root that holds `bindings.json` and
`baselines/` is fixed by the operator's account, never by the tool command.

- macOS: `<home>/Library/Application Support/codeflow/`. Linux:
  `<home>/.local/state/codeflow/`. `<home>` is the effective user's home
  directory from the system user database, not `HOME`, and
  `XDG_STATE_HOME` is not read. Windows: the Local AppData known folder, if
  Windows is supported at all (unit 3 AC-7 records the outcome).
- The existing `present` state root reads `HOME` and `XDG_STATE_HOME`
  (E20 `crates/codeflow-present/src/state.rs:2773-2806`). The authority root
  must not reuse that resolver.
- The root must be owned by the effective user and not writable by group or
  others; otherwise it is unavailable, which is binding lost.
- **No replacement store.** A guard never creates the root, the registry or
  a baseline. Operator commands create them only when the root is absent,
  and never overwrite an unreadable or unparsable registry: they refuse and
  name the file for the operator to restore or move aside.
- A `HOME=...` or `XDG_STATE_HOME=...` prefix on a tool command does not
  reach the hook process, which inherits the harness's environment. A seat
  launched with a different `HOME` still resolves the same root, since no
  variable is read. Both cases are tested (unit 2 AC-20).

**The binding registry.** `bindings.json` maps each enrolled root (the real
path of the main worktree) to its common dir key, and records exclusion
boundaries (below). Only operator commands write it. Seat worktrees live
under the root (`.worktrees/`, ADR-0055 decision 5), so the root's binding
covers them.

**Admission pin.** When a host launches a seat (`delegate init` for Claude
seats, the Herdr `agent start` launch environment for Codex and Grok seats),
it sets `CODEFLOW_ADMISSION` in the seat's launch environment: the
authority root's real path, the enrolled root and the baseline key. Hooks
inherit it, and a tool command cannot change a hook's environment. The pin
can only add refusals. A guard that finds a pin and a store that does not
match it (root missing, entry missing, another key, another root) treats the
path as binding lost. A forged pin never grants anything, so its
provenance does not matter.

**A pin that does not arrive is an admission failure** (SET-R6-2). v7 let
a harness that drops `CODEFLOW_ADMISSION` pass acceptance while the store
was healthy, and that seat would later read "not enrolled" after store
loss, which is broader than the declared residual. So every receipt
records the tuple its hook observed, and acceptance compares it with the
host's expected launch tuple (1.2 item 9): absent, malformed or mismatched
is never ready. Whether each harness passes its launch environment to hook
processes is a pre-dispatch check (unit 3); for Claude seats the planned
carrier is the task-scoped settings `env` block, not yet verified to reach
hooks. A route whose harness cannot carry the pin is recorded
`pin-unsupported` and stays unavailable until a separately qualified
carrier exists. Once a seat is admitted with its pin, a later loss of the
store makes that seat's guards read binding lost, never not enrolled. The
primary session keeps its exception, and a new process without a pin after
total erasure on a full-access route stays the declared residual.

**Reading the store.**

| What the guard reads | State |
|---|---|
| root, registry and baseline read and parse; the path's root is bound to the resolved common dir | bound |
| root or registry unreadable, unparsable, or with the wrong owner or mode | binding lost, for every path |
| root present, registry missing while `baselines/` holds any file | binding lost, for every path |
| registry lists the path's root, its baseline is missing or unparsable | binding lost |
| valid registry with no entries, and `baselines/` empty | not enrolled |
| root absent, and the process holds an admission pin | binding lost: the pinned expectation fails |
| root absent, no pin | not enrolled (see "Total loss") |

Caption: a failed read is never an empty namespace. Only a readable, empty
store or an absent store without a pin reads as not enrolled.

**Total loss.** A new process with no retained evidence cannot tell an
erased store from a machine that never enrolled. On sandboxed routes the
seat cannot erase the store, since it is outside every sandbox's write set.
On Codex under full access and Grok without D3 it can, and a process started
after that sees "not enrolled" with the built-in policy. The built-in policy
is the shipped default and does not keep an adopter's own stricter settings.
This is the declared residual; the design does not detect it. An admitted
seat fails its pin instead. This replaces v6's sentence at `:511-513`, which
AC-11 contradicted.

**Resolution, on every guard call.**

```
  guard call
    |
    v
  authority root from the account (never HOME or XDG); read the store
    +-- unreadable, unparsable, wrong owner, partial loss --> BINDING LOST
    |
    v
  resolve the command's effective context with git itself:
  worktree top level, git dir, common dir, index, core.hooksPath
  (cwd, cd, -C, --git-dir, --work-tree, GIT_DIR, GIT_COMMON_DIR,
   GIT_WORK_TREE, GIT_INDEX_FILE, -c, --config-env, GIT_CONFIG_*)
    +-- an override it cannot resolve, or a GIT_* variable exported
    |   for later calls ---------------------------------> refuse
    v
  longest registered entry containing the top level?
    | bound root                | exclusion            | none
    v                           v                      v
  common dir == key,          common dir ==          common dir has a
  baseline loads,             recorded one?          baseline?
  pin (if any) agrees?          | yes --> built-in     | yes --> that baseline
    | yes --> baseline          |   policy, no seats   | no
    | no  --> BINDING LOST      | no  --> BINDING LOST v
             refuse git                (parent)      pin present?
             mutations and                             | yes --> BINDING LOST
             guard-probe; other                        | no
             calls on built-in                         v
             policy; operator                        hook ran with --contract?
             runs `codeflow                            | no  --> LEGACY: narrow
             baseline bind`                            |         form (selector,
                                                       |         not proof)
                                                       | yes --> NOT ENROLLED:
                                                       |         built-in policy,
                                                       |         seats unavailable
```

Caption: how a guard finds its authority. Bindings, exclusions and the pin
are checked before the `--contract` flag is looked at.

**The states, kept apart.**

- **Bound.** The path's root is registered, the effective common dir equals
  the recorded key, the store and baseline load, and a pin, if present,
  agrees. Two ordinary linked worktrees under the root share the baseline.
  A linked worktree outside the root whose common dir is an enrolled key uses
  that baseline.
- **Binding lost.** Any row of the table above that says so, a registered
  root whose effective common dir is not its key, a void exclusion boundary,
  or a pin the store does not satisfy. Git commands that create commits,
  move refs or change routing are refused, and so is `codeflow guard-probe`,
  so no seat is briefed. Other calls are judged under the binary's built-in
  policy. The operator restores the store or the repository, or runs
  `codeflow baseline bind` after checking it. A lost binding is never read
  as not enrolled while any evidence of it remains, meaning the store or a
  pin; "Total loss" is the one declared exception.
- **Excluded child.** A nested repository the operator recorded as excluded
  (below): built-in policy, never a ready seat route.
- **Fixture.** A repository under an admitted fixture root (2.6; D10
  decided as option A): its own policy for local rules, the built-in floor for the
  security families, remote-affecting actions refused, seats `ready
  (fixture)` only. A fixture root is checked like an exclusion, before the
  `--contract` flag.
- **Not enrolled.** No entry covers the path, its common dir has no
  baseline, no pin is held, and the store is readable and empty or absent.
  Then the `--contract` flag selects the policy source:
  - **without `--contract`**, the narrow form: policy from `HEAD` when it
    exists, from the working copy otherwise, with "no enforcement baseline"
    reported. **The missing flag is a compatibility selector, not proof**
    that the repository predates this release. It says only that the
    harness config that invoked the hook has pre-release hook definitions.
    Inside a binding the flag is never consulted, so omitting it there
    changes nothing; guard-entry protection and readiness's entry
    comparison stop it being stripped casually;
  - **with `--contract`**, the policy built into the binary (release builds
    embed the shipped assets, E20 `crates/codeflow-cli/src/embedded.rs:1-12`,
    and the in-code defaults are tested against `assets/base/policy.json`,
    E20 `crates/codeflow-core/src/hooks/policy.rs:781-790`). A scratch
    repository an agent creates outside every root is judged this way.

  Seats are unavailable in both not-enrolled forms (1.2).

**Enrollment in the same operation** (cost 2). `codeflow init`, `codeflow
update` and the **first `codeflow baseline approve` in a root** each record
the identity and bind that root in the same operation, as does an
operator-run setup flow that calls one of them. When the tree to approve
equals an identity already approved on this machine under the current
contract (same paths, modes and blobs), `baseline approve` shows a
**binding-only confirmation**: the exact destination root and the matching
earlier approval (its root and time), with no full policy review. A content
match never picks the path: the operator's command names it, and the
confirmation shows it. No remote signature or trust service is added.

**Nested repositories: one inventory at adoption** (cost 3). A child
repository's own `.git` cannot establish that it is independent: a
relocated worktree under `.worktrees/` looks the same. So the boundary is
the operator's record, taken once for the whole workspace.

- `codeflow baseline bind --inventory <root>` lists every nested git top
  level under the root, and the operator decides each in one reviewed batch:
  - **adopt**: the child gets its own binding and baseline, through its own
    `init`, `update` or first `approve`;
  - **exclude**: an exclusion boundary in the registry (the child path, its
    common dir at that time, who recorded it and when). The child is judged
    by the built-in policy and is never a ready seat route;
  - a directory that is not a repository is an ordinary folder under the
    parent and needs no entry.
- **The boundary is void** when the child's effective common dir differs
  from the recorded one. The path is then binding lost under the parent's
  authority.
- **Authority follows the resolved repository.** A command inside a child
  that targets the parent's git state (`git -C ..`, or a path whose top
  level is the parent) uses the parent's binding, and a write to a parent
  enforcement path stays a parent integrity refusal.
- **A new child** needs one `codeflow baseline bind <path>` or `codeflow
  baseline bind --exclude <path>`. Until then it is refused, as in v6.
- Discovery reads directories, which an agent can write; only the
  operator's decision, recorded outside the repository, counts.

**This workspace.** `/Volumes/DATA/Local/software-workspace/projects/agentic-systems`
is a CodeFlow minimal-tier repository whose `.gitignore` excludes the
project folders (`agentic-systems/.gitignore:1-7`). Observed this pass by
listing each child's `.git` and `.codeflow/policy.json`:

| Child | Observed | Proposed inventory decision |
|---|---|---|
| `codeflow` | git repository, CodeFlow installed | adopt |
| `agent-os` | git repository, CodeFlow installed | adopt |
| `agent-workstation-kit` | git repository, CodeFlow installed | adopt |
| `agent-os-portal` | git repository, no `.codeflow/policy.json` | exclude, unless the operator adopts it |
| `agentic-development-model` | git repository, no `.codeflow/policy.json` | exclude, unless the operator adopts it |
| `project-organization-guide` | plain folder, ignored by the workspace repository | none: a folder under the workspace repository's authority |
| `design` | plain folder (not in the brief's list) | none: under the workspace repository |

Caption: the one inventory for this workspace. The two exclusions are the
operator's decision; the sample record is
`evidence/permission-presets/state/bindings.workspace.example.json` (v7's, with the workspace-mode note).

**Supported overrides.** `-C` and `cd` move the path, and the path's own
state applies. `GIT_INDEX_FILE` is supported: the commit check reads that
file. `-c`, `--config-env` and `GIT_CONFIG_*` are supported and enter the
effective config. `GIT_DIR`, `GIT_COMMON_DIR`, `GIT_WORK_TREE`, `--git-dir`
and `--work-tree` are supported only when they resolve inside the bound
repository; pointing elsewhere from a bound path is refused. A statement
that exports any of these variables for later calls (`export`, `declare -x`,
`set -a` followed by an assignment) is refused, since a later hook process
does not see the shell's environment. Unit 4's canary records whether
Claude keeps exports across Bash calls; the refusal holds either way. The
existing guard at E20 models only a `GIT_DIR` given on the git command and
refuses `GIT_COMMON_DIR` and `GIT_WORK_TREE` in the command's environment as
unresolvable (E20 `crates/codeflow-core/src/hooks/git_target.rs:320-323`;
`git_guard.rs:2503-2525`); it does not model `GIT_INDEX_FILE`, which appears
only in its tests (`:7049`). When a retargeted repository cannot be read it
keeps its earlier reading, the session branch and policy included, and a
readable retargeted repository is judged by that repository's own policy
(`git_guard.rs:44-63`). The baseline check must refuse where that path
keeps an earlier reading, and must use the binding, not the target's own
policy, for an enrolled repository.

**The guard's own git queries** follow the rules in 2.2.

**Cost.** The registry and baseline are two small files read once per call.
A call that is not a git command finds its policy through the registry by
path alone, with no git query, since the route matters only to the checks on
git commands. Context resolution costs one `git rev-parse` per git command;
tree and index comparisons, and the merge pre-check and query, run only for
commands that create commits, move refs or change routing. The guards' time
budget is a test (unit 2 AC-12). A clone on a new machine needs one operator
admission, which is a binding-only confirmation when its identity is already
approved there.

**What stays outside.** A commit created by a git library inside an
interpreter is not seen by the guards; it cannot move the baseline, and the
next CLI commit or transition is checked on state. CI reads the committed
policy; a pull request that changes enforcement files is visible in review.
The git-hook plane cannot tell the operator from an agent and does not police
enforcement changes. Total erasure of the store on a full-access route is
the declared residual above.

### 2.4 SET-R2-2 and the OS residual

SET-R2-2 is fixed at the authority level: a written policy file no longer
changes a guard decision in any harness. The OS protection of the baseline
and the registry is a stated residual for Codex under full access and for
Grok without D3.

### 2.5 Paired tests

| Case | Expected |
|---|---|
| `git update-index --cacheinfo` staging a weaker `policy.json`, then `git commit` | the plumbing call refused; with the index changed by a library instead, the commit refused on state |
| a staged mode change on `.codeflow/git-hooks/pre-commit` | commit refused |
| `git rm --cached .codex/rules/codeflow.rules`, then commit | refused (deletion) |
| a new file under `.codex/rules/`, staged | refused (addition) |
| `git checkout <old commit with a weaker policy>` | refused, unless that identity was approved and is contract-compatible |
| `git checkout` of an approved identity from an older contract | refused until requalified |
| `git merge` of a branch that changes `.grok/sandbox.toml` | refused (merge-tree result) |
| rebase of a range whose middle commit weakens the policy and whose last commit restores it | refused; the unchanged-enforcement rebase and cherry-pick pass |
| conflict, then `rebase --continue` with an unapproved staged policy | refused |
| `git config core.hooksPath /tmp/x`, then any call | refused |
| `git -c core.hookspath=/x commit`, `git --config-env=core.hooksPath=V commit`, `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.hooksPath ... git commit` | refused; the same with `user.name` or `user.email` passes |
| approved default index, `GIT_INDEX_FILE=<alt>` with an unapproved policy, `git commit` | refused; an alternate index with approved entries passes |
| worktree B's index unapproved, root's approved, commit in B | refused; the converse passes |
| `commondir` of a seat worktree edited to point at a copy | binding lost: refused, `codeflow baseline bind` named |
| `git init --separate-git-dir <new>` at the bound root | refused; with the relocation done by the operator, later calls refused until bound |
| `GIT_DIR=<copy> git commit`, `GIT_COMMON_DIR=<copy> git commit`, `git --git-dir=<copy> commit` from a bound path | refused |
| `export GIT_INDEX_FILE=<alt>` | refused |
| baseline file deleted, or registry corrupted | binding lost: refused, not legacy |
| genuine legacy install: pre-release hooks, no state root | narrow form; doctor reports "no enforcement baseline"; seats unavailable; after `codeflow update`, bound |
| contract-3 hooks, no state (fresh clone on a new machine) | not enrolled: built-in policy; seats unavailable; `codeflow baseline approve` binds |
| two linked worktrees under the root | both bound to one baseline; an approval in A makes B's readiness stale |
| nested repository under a bound root, not bound | git mutations refused until `codeflow baseline bind <path>` |
| `checkout-index -a -f`, `apply --index` of a patch to `.codex/hooks.json`, `fetch origin main:main` | refused; `git fetch`, `git fetch origin`, `git fetch origin main` pass |
| ordinary commit, branch switch, rebase onto the integration tip, worktree add | pass |
| fresh `codeflow init` on an unborn `HEAD`, then the first agent commit | pass |
| operator edits `policy.json`, runs `codeflow baseline approve`, commits | pass; agent calls in between pass |
| operator update, then a commit staging half the new identity | refused with the per-path listing; staging all passes |
| agent session runs `codeflow baseline approve`, `bind` or `codeflow update` | refused; in Claude's sandbox the write is also denied at the OS |
| inert canary merge driver selected by `.gitattributes` for a path both sides changed, then `git merge`, `rebase`, `cherry-pick` of a range, `stash pop` | each refused before any query; canary marker absent |
| `merge.default` naming the canary driver, no attribute | refused; marker absent |
| a defined canary filter on a changed path with `merge.renormalize=true`, and with `-X renormalize` | refused; marker absent |
| canary driver defined in the trusted home's global config | refused; marker absent |
| built-in text merge of non-enforcement changes, and a `merge=union` path | pass; the repository's object count unchanged by the guard; computed enforcement entries equal the real result |
| `remote.origin.fetch = +refs/heads/*:refs/heads/*`, then `git fetch` | refused; the default `+refs/heads/*:refs/remotes/origin/*` passes |
| the same heads mapping, then `git fetch origin topic` (explicit source, no destination on the command, `topic` not checked out) | refused: the configured mapping updates `refs/heads/topic` |
| the same heads mapping, then `git fetch --refmap= origin topic`; and `git fetch --refmap= origin topic:topic` | first passes (configured mapping suppressed); second refused on its explicit destination |
| default mapping, `git fetch origin topic` | passes: `refs/remotes/origin/topic` only |
| healthy store, a seat whose launch drops `CODEFLOW_ADMISSION` for one required hook | `pin-missing`; no brief |
| healthy store, pin present and equal in every receipt | ready; after the store is then deleted, that seat's next git mutation is refused as binding lost, not not enrolled |
| a route whose harness cannot carry the pin to hooks | `pin-unsupported`; recorded as unsupported in the closeout matrix |
| `merge.text.driver` defined (canary) with `merge=text` on a changed path; the same for `binary` and `union`; `merge.default=text` with `merge.text.driver` defined and no attribute | each refused before any query; marker absent |
| canary driver selected from `$GIT_DIR/info/attributes`, from `core.attributesFile`, and from a subdirectory `.gitattributes` | each refused before any query; marker absent |
| merges touching a text path, a `merge=union` path, and CodeFlow's `*.br` and `*.gz` paths under the `binary` macro | pass the pre-check; binary conflicts still stop as conflicts |
| a Grok builder seat whose folder was trusted by `grok --trust` beforehand; one never trusted | ready (cooperative); `hooks-untrusted` or `guard-missing`, whatever the launch line says |
| registry deleted while `baselines/` holds a file | binding lost, not not-enrolled |
| whole state root deleted: admitted seat with a pin; new process without a pin | binding lost; not enrolled (the declared residual, recorded as such) |
| state root or registry unreadable (mode 000), or unparsable | binding lost; no file created or replaced |
| valid empty registry and empty `baselines/` | not enrolled |
| `HOME=/tmp/x XDG_STATE_HOME=/tmp/y git commit` as a tool command; a seat launched with `HOME=/tmp/x` | authority root unchanged in both |
| hook without `--contract` in a bound root | baseline authority, not the narrow form |
| first `codeflow baseline approve` in a fresh clone; the same identity already approved for another root | root bound in the same operation; binding-only confirmation naming the destination |
| inventory: adopted child, excluded child, plain folder; excluded child's `commondir` repointed; `git -C .. commit` from inside an excluded child | own binding; built-in policy, seats unavailable; binding lost; parent authority |
| `codeflow integrate task/x integration/y` where a replayed commit changes `.codeflow/policy.json` | refused, naming `codeflow baseline approve --merge task/x --into integration/y`; passes after the operator runs it |
| `gh pr merge 12 --squash --match-head-commit H` for a pull request that changes `.claude/settings.json`; the same when the base rules or merge state cannot be read | refused (2.2.1); refused |
| fixture root admitted: `codeflow init --yes` in a trial; the trial's own policy protects `trunk`; `git push` to a local bare repository and to a real host; `git remote set-url origin https://github.com/...` | init passes with no store write; a commit on `trunk` refused by the trial's policy; both pushes refused; set-url refused |
| a Grok consult seat in `--permission-mode auto`; the same seat launched with `--always-approve` on a consult route | ready (cooperative) for the consult; `posture-mismatch` |

### 2.6 Evaluation fixtures (D10)

**The need.** The evaluation kit materializes every trial with `codeflow
init --yes` in a temporary run root (E18
`.agents/skills/cf-evaluate-model/scripts/eval_kit.py:1889`); duo cases need
real native seats, and a full suite runs every case three times (E18
`.agents/skills/cf-evaluate-model/SKILL.md:59-62`, `:92-93`). Under v7 such a
trial is not enrolled, so its own `.codeflow/policy.json`, which is what the
case tests, never reaches the guards, and its seats are unavailable (REC R4,
C1). Admitting each trial by hand is not practical. Whether a fixture may
use its own policy is an authority boundary, so it is operator decision D10
(section 8), decided as option A. The route:

```
  operator, once:  codeflow baseline bind --fixture-root <harness temp path>
                                   |
  eval kit, per trial:  codeflow init --yes in <root>/<trial>   (no store write)
                                   |
  guards in <root>/<trial>:  local git rules from the trial's own policy
                             security families never looser than built-in
                             every remote-affecting action refused
                                   |
  seats in <root>/<trial>:   ready (fixture), never production readiness
                                   |
  eval kit, after grading:   preserve results, then marked cleanup
```

Caption: the fixture route. One operator admission covers every trial under
the root.

- **Admission.** The operator records one fixture root, a temporary path the
  harness or eval kit created (for example under the operator's `$TMPDIR`).
  It may not contain, or sit inside, a bound root, an exclusion or another
  fixture root. It is a registry entry of kind `fixture-root`, written only
  by the operator; its fields and identity are below (SET-R7-2).
- **Init inside it.** `codeflow init` under an admitted fixture root
  scaffolds and writes nothing to the store, and exec-guard allows it there.
  Outside every root and fixture root, an init that cannot write the store
  completes the scaffold and reports "not enrolled" rather than failing.
- **Authority inside it.** A repository under the fixture root is judged by
  its own committed policy for local rules (git discipline, protected
  branches, discard and clean rules, hook integrity inside the fixture),
  since that policy is under test. The security families (privilege,
  outward, secret reads, interpreter scan, headless, enforcement paths
  outside the fixture) take the stricter of the fixture's value and the
  built-in policy.
- **Confinement.** The fixture route applies only to a command whose every
  location and mutation target resolves inside the admitted root (below,
  SET-R7-1).
- **Remote-affecting actions are refused inside the fixture route**,
  whatever the fixture policy says: every `git push` except to a local
  repository that is itself eligible under the same root; `gh pr` create, merge,
  edit and review; `gh release` and `gh repo` writes; `gh api` with a write
  method; and `git remote add` or `set-url`, or a `url.<base>.insteadOf`
  config, pointing at anything other than a local path inside the fixture
  root. A fixture therefore cannot become a way to clone a real repository
  and push under a looser policy. The shipped fixtures have no network
  remote and use an offline `gh` stand-in (E18
  `.agents/skills/cf-evaluate-model/resources/fixtures.json:2088`); a
  trial that keeps a local bare `origin` inside the root can push to it.
- **Seats.** A seat in a fixture passes the guard-readiness preflight
  against the trial's committed state (below) and is reported `ready
  (fixture)`. Host skills never send production work to a fixture-labelled
  seat, and a production route never accepts that label.
- **Disposal.** Fixtures are disposable, under the eval kit's own
  preserve-then-clean contract: after grading and evidence capture, results,
  grader notes, fixture digests and permitted session references are kept
  outside the fixture root, and only then is the run removed, by the kit's
  marked cleanup, which refuses unmarked paths, symlinks, roots,
  repositories and a mismatched run id (E18 `.agents/skills/cf-evaluate-model/SKILL.md:126-129`, `scripts/eval_kit.py:2791-2807`). Doctor lists
  trial directories left under the fixture root.

**Fixture eligibility by real path and git routing (SET-R7-1).** v10
decided "under the fixture root" by the working directory. A trial can
route git elsewhere while its working directory stays inside, so
eligibility is decided on every call from what the command will touch:

- **Locations resolved.** With the guard's effective context resolution
  (unit 2 AC-13 to AC-15: `-C`, `--git-dir`, `--work-tree`, `GIT_DIR`,
  `GIT_COMMON_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`), the command's
  worktree top, git dir, common dir and index file, and every linked
  worktree the common dir lists (`<common>/worktrees/*/gitdir`,
  `gitrepository-layout(5)`). Each is reduced to its real path, symlinks
  resolved and `..` removed, at the call; nothing is cached.
- **Mutation targets resolved.** Besides the repository itself: the
  destination of a push or fetch to a local path, a clone destination, a
  `worktree add` path, output files (`format-patch -o`, `bundle create`,
  `archive --output`), and, for exec-guard and edit-guard, any file a
  command or file tool writes.
- **Eligible** when the admitted root's real path is a prefix of every
  location and target, and the root's recorded device and inode still
  match. Linked worktrees and local bare origins wholly inside the root
  stay eligible, so a trial may keep a worktree and a bare `origin` beside
  it.
- **External target.** When any location or target resolves outside the
  root, the fixture route does not apply to that command. Each external
  target is judged under its own authority (its binding's baseline, its
  exclusion's built-in policy, or the built-in policy if it is not
  enrolled), never under the fixture's policy; a command whose targets
  fall under two authorities, or whose target cannot be resolved, is
  refused.

| Case | Expected |
|---|---|
| a trial with a linked worktree and a local bare `origin`, both inside the root: commit in the worktree, push to `origin` | fixture route; allowed under the trial's own local rules |
| `git --git-dir=/outside/prod.git commit` from inside a trial, where `/outside/prod` is a bound production repository | not the fixture route; judged by `/outside/prod`'s baseline, so the trial's looser local rules do not apply |
| a trial whose `.git/commondir` (or a linked worktree's `.git` file) points at a common dir outside the root | not eligible; git mutations judged by that common dir's authority, refused if it has none |
| `GIT_INDEX_FILE=/outside/index git commit` inside a trial | refused: external index with no authority of its own |
| `<root>/t1` a symlink to `/outside/t1`; a trial whose `.git` is a symlink out of the root | not eligible; judged outside the fixture route |
| `git worktree add /outside/wt` from a trial | refused under the fixture route; judged as a target outside it |

**Fixture admission identity (SET-R7-2).** The fixture route replaces the
production baseline with two records the guard can check: the operator's
fixture-root record and the trial's committed state.

- **The fixture-root record**, in `bindings.json` under the authority root
  (`evidence/permission-presets/state/fixture-root.example.json`): `admission_id` (random,
  made by the operator command), `root` (real path), `root_device` and
  `root_inode`, `contract` at admission, `builtin_floor` (the identity of
  the built-in policy of the admitting binary), `recorded_by`,
  `recorded_at`. Only `codeflow baseline bind --fixture-root` writes it and
  only `codeflow baseline bind --remove-fixture-root` removes it, both run
  by the operator.
- **The authority lookup** is the production one: the authority root from
  the account database, never `HOME`, `XDG_STATE_HOME`, the environment or
  anything in a trial; then the one fixture-root record whose real path is
  a prefix of the command's resolved locations (admission refuses
  overlapping roots, so at most one matches).
- **The admission tuple** a fixture seat carries in `CODEFLOW_ADMISSION`:
  authority root, fixture root, `admission_id`, trial real path. `delegate
  init` in the trial sets it from the record.
- **The trial identity** receipts and readiness validate: `admission_id`,
  trial worktree and common dir real paths, the trial's `HEAD` commit at
  the preflight, and E(`HEAD`), its committed enforcement entries (path,
  mode, blob), from which the expected hook-entry hashes are computed.

| Production check (1.2) | Under the fixture route |
|---|---|
| item 3: the worktree is bound to an enrolled repository | the worktree is eligible (SET-R7-1) under the record that matches the tuple's `admission_id` |
| item 5: the receipt's baseline identity is the current approved identity | the receipt's trial identity equals the one read at acceptance, and the index and working copy equal `HEAD` on every enforcement path (an uncommitted enforcement change is `hook-entry-mismatch`) |
| item 6: expected hook-entry hash from the approved identity | expected hash from E(`HEAD`): receipt, live entries and committed entries must agree |
| commit and transition checks against approved identities (2.2) | local rules from the trial's committed policy at the commit's parent; a trial may change its own enforcement files, since that is under test, and the change makes its seats stale |
| item 9: tuple of authority root, enrolled root, baseline key | the fixture tuple above |

Caption: the production checks the fixture route replaces. Everything
else in 1.2 and 1.3 is kept as it is: the nonce and launch freshness (item
1), one session (item 2), the contract (item 4), harness, role and posture
(item 7), the probe marker (item 8), observed-pin equality (item 9 with
the fixture tuple), drift invalidation (1.3: a change to E(`HEAD`), to
the record, to harness config or to the binary makes the seat stale and
requires a fresh preflight), and loss refusal: when the record is
removed, or replaced (another `admission_id`, or another device or inode
at the same path), git mutations in the trial are refused as binding lost
and its seats report `binding-lost`, never not enrolled while a pin names
the old admission. Every receipt carries `route: fixture` and the
`admission_id`; production acceptance requires `route: production` and
refuses a fixture receipt as `fixture-only`.

| Case | Expected |
|---|---|
| a trial under the admitted root with no per-trial baseline anywhere in the store | `ready (fixture)` |
| the record removed after the seat is ready, then `git commit` and the next prompt | commit refused as binding lost; seat `binding-lost`, no prompt |
| the record replaced by a fresh admission of the same path | `pin-mismatch`; no brief |
| a fixture seat's receipts offered to a production route's acceptance | refused, `fixture-only` |
| a trial hook file edited but not committed before the preflight | `hook-entry-mismatch` |

### 2.7 Wording fixes (REC H2, R1, C8, A3, C4)

- **Headless opt-in.** The field `security.headless_opt_in.harnesses` took
  catalog family ids, not harness ids (REC H2). It is renamed
  `security.headless_opt_in.families` and takes `claude`, `codex` or
  `grok`; doctor names the matching catalog harnesses (`claude-code`,
  `codex-cli`, `codex-app`, `grok-cli`). Nothing shipped the old name, so no
  migration is needed (`evidence/permission-presets/policy/headless-opt-in.example.json`).
- **D4 reading.** The workspace release plan records D4 as "refused ...
  when the peer's harness is available" (REC C8). The ADR states the
  implemented reading: a headless peer run is refused unless
  `security.headless_opt_in` names its family with a reason; config decides,
  and detected availability only shapes the message and doctor's report.
- **Preflight.** "Guard-readiness preflight" throughout, as defined in 1.2.
- **Refused families in the ADR.** The ADR's decision 1 names the refused
  action families in its own text. It no longer points at
  `cf-method/references/autonomy.md`, which is not on E20 (REC C4).

### 2.8 Workspace mode: settings come from where a session starts

The operator decided workspace mode on 2026-09-28: an umbrella repository
whose root stays on its designated branch, with CodeFlow projects nested
inside (workspace `RELEASE-PLAN.md:544`). That branch is named by the
policy key `git.root_branch`, whose conventional value is
`integration/workspace`, held as one constant and applied by `codeflow init
--workspace` (`RELEASE-PLAN.md:546`); the worktree-rule task builds the key,
the constant and the command. This section uses that key and defines no
workspace-mode detection of its own. A session started at the
umbrella root loads the umbrella's harness preset (its deny rules, sandbox,
hook set and contract) even while it works inside a nested project. The
guards already follow the target: git-guard judges a command aimed at
another repository by that repository's own branch and policy (E20
`crates/codeflow-core/src/hooks/git_guard.rs:36-63`, TSK-112), and the
binding design resolves each command's effective context (2.3). The
harness layer does not follow the target, and cannot: a harness reads its
settings once, from where it started.

So doctor at an umbrella, a repository whose policy sets `git.root_branch`,
compares presets (unit 3 AC-13). For each
enrolled nested project in the binding registry (the inventory of 2.3), it
reads both presets from their approved identities and warns, naming the
child, the harness and the action family, when the umbrella lacks a deny
entry, Codex rule or Grok profile entry the child has, has a weaker
sandbox setting, lacks a guard hook or its fail-closed form, or has an
older hook contract. A stricter umbrella and an excluded child raise
nothing. The workspace-mode docs say, in a few lines, that settings come
from where a session starts, and that the remedy is to start the session
in the project or keep the umbrella at least as strict (unit 5).

---

## 3. Spike 0 (Fable R2-1)

Before the retry canary, run the companion and Herdr inside the Claude
sandbox with narrow local allowances, as machine relief in
`.claude/settings.local.json` (S7):

- **Companion.** `allowWrite` only for the `~/.codex` subdirectories the
  app-server writes (the spike lists them), never the whole directory:
  `~/.codex` also holds `hooks.json`, whose commands Codex later runs on the
  host, as well as `config.toml`, `rules/` and `auth.json`. The spike records
  whether the app-server starts, whether Codex's own nested sandbox can apply
  inside Seatbelt, and whether a companion `review` and `task` complete.
  **On a pass**, the companion entry leaves `sandbox_retry_allow`, the seat
  deny stays, and the companion's Codex thread runs inside Claude's sandbox,
  which closes the v4 2.3 residual. **The shortcut covers only the
  companion's retry criteria** (unit 4: the companion clause of AC-3, AC-4,
  AC-5 and AC-7), which reduce to "every companion retry refused". The retry
  canary (AC-2) still runs, since refusing every retry depends on the guard
  seeing the retry field. **On a fail**, v4 section 2 and the criteria of
  section 4 below stand.
- **Herdr.** Rejected as a socket allowance. `sandbox.network.allowUnixSockets`
  for Herdr's socket would let every sandboxed process drive Herdr, and Herdr
  can type into any pane (`pane run`, `pane send-text`; HD). The spike
  records which Herdr calls fail inside the sandbox. D7 stays the route.

**D7 as decided (2026-09-28).** The operator adopted D7 with adjustments
(RP lines 537-538). The named Herdr list runs outside the Claude sandbox
through the retry path, each entry meeting the general retry conditions
with `herdr` pinned by absolute path, and the Herdr grammar (unit 4 AC-6)
applies to every Herdr call, sandboxed or retried, whatever spike 0 shows.
The list may otherwise be empty after a companion pass of spike 0; a retry
is allowed only for a call that matches both an entry and the grammar, and
a companion retry is then refused (unit 4 AC-8).

| Herdr call | Allowed form | Refused |
|---|---|---|
| `workspace list`, `tab list`, `pane list`, `pane current`, `agent list` | no arguments | any argument |
| `agent get`, `agent read`, `agent wait` | a registered seat name; `wait` with `--until` and `--timeout` | any other target |
| `tab create` | `--workspace` only | any other option |
| `agent start` | name, `--kind claude\|codex\|grok`, `--pane`, `--timeout`, and after `--` only the seat flags of the route table (1.5) for that kind and role, including its launch-time trust form | any other seat flag; a trust form for a folder that is not the seat's own worktree |
| `agent prompt` | a registered seat; text that does not start with `!` or `/`, or that starts with exactly `/compact`, `/clear` or `/new` as the command word; `--wait`, `--until`, `--timeout` | `!` text; every other slash command, so any that changes permissions, approvals, trust, hooks or the model |
| `agent send-keys` | a registered seat and the single key `Escape` | any other key, any key sequence, any unregistered target |
| `pane run`, `pane send-text`, `pane send-keys`, `agent attach` | none | always |

Caption: the decided D7 grammar. A registered seat is a live agent that
`delegate init` recorded for this task and `herdr agent list` shows; a shell
pane is never one.

- **Briefing.** Seats are briefed only with `herdr agent prompt`, never by
  typing into a pane. This holds for the first brief and for every later
  instruction.
- **Escape.** `agent send-keys <seat> Escape` lets the primary interrupt a
  seat's current turn, for example a runaway tool loop, without typing text
  into it. Escape to a seat that is showing an approval or trust prompt
  dismisses that prompt; it never answers it.
- **Slash commands after normalization.** The grammar judges `agent
  prompt` text as the harness will read it: leading whitespace and line
  breaks removed and Unicode normalized (NFKC, so a full-width or other
  look-alike solidus counts as `/`), then the first word compared exactly
  with `/compact`, `/clear` and `/new`. Any other text the harness could
  read as a slash command is refused, known or not. Where a harness's own
  normalization is not documented, the guard refuses text whose first
  visible character is any solidus look-alike unless it is an exact
  allowlisted word (unit 4 AC-6).
- **`/clear` and `/new` end the session readiness was bound to.** Both can
  give the seat a new native session, and readiness is bound to the
  session identifier (1.2 item 2). The host marks the seat stale when it
  sends either, and runs a fresh guard-readiness preflight before the next
  work brief; it does not wait to discover the new identifier. `/compact`
  is treated the same where the harness changes the session identifier
  (unit 5 AC-2 records which do).
- **Left to the operator.** Shell panes and a seat's approval prompts. A seat
  stuck on an approval prompt is reported by `agent wait`, recorded on the
  operator-actions list (section 9), and the primary carries on with other
  work.
- **Folder trust at launch** (1.5): Grok's trust step, Codex's `-c` trust
  override, and for Claude nothing, since a worktree takes the main
  checkout's trust. This keeps the 2026-09-22 authorization (section 0)
  without a key or slash command that answers a prompt.
- **Grok trust step.** Carried in the list as one pending entry: `grok`
  pinned by absolute path, cwd a registered seat worktree, arguments exactly
  the trust-only form unit 5 AC-2 qualifies. It stays unusable (refused)
  until that form qualifies (1.5).

`evidence/permission-presets/policy/retry-allow-herdr.D7.json` carries the list.

---

## 4. Criteria from rounds 3 to 6

Each becomes an acceptance criterion in the unit named. None changes the
design.

| Finding | Criterion | Unit |
|---|---|---|
| SET-R3-3, R2-6 executable identity | The retry entry pins the program to an absolute path recorded at qualification, outside every sandbox-writable directory, and records the companion's `codex` child path; refused when a PATH entry of the Bash call is sandbox-writable or resolves `codex` elsewhere. Synthetic PATH-shadow controls with inert canaries, and a passing trusted-toolchain case | 4 |
| SET-R3-4 wildcard versions | The entry names the audited companion version and the sha256 of the script and its `scripts/lib` modules; negative control `99.0.0` | 4 |
| R2-6 lone prompt with option-like words | Prompts pass as several arguments; one-argument `task "fix the --cwd handling"` refused and documented | 4 |
| R2-4 `agent start --settings` | exec-guard accepts a `--settings` value only when its hash equals the one `delegate init` recorded; the Herdr grammar applies to every Herdr call | 3 (hash), 4 (grammar) |
| R2-5 heuristic keys | One key, `security.interpreter_scan`, with positive controls | 2 |
| R2-7 `gh auth` forms | In the account family (`evidence/permission-presets/actions.json`) | 1 |
| R2-8 wording | `delegate wait` reports and the host skills wait (S6); `/cf-customize` prints; the D4 refusal names an interactive seat and the tier's route | 3, 5 |
| R2-9, SET-R3-5 unverified rows | A closeout evidence matrix; no row may close as unverified | 5, with each unit's rows |
| SET-R4-1 binding | Registry, three states, `codeflow baseline bind`, effective context resolution, paired tests of 2.5 | 2; readiness outcome 3 |
| SET-R4-2 acceptance and staleness | Entry hash, harness and posture compared at acceptance; readiness record and recheck; contract-compatible identities; validity boundary documented | 3; compatibility 2 |
| SET-R4-3, Grok R3 gap 2 | Cooperative label per route; direct `codeflow hook` refused; residual in the ADR | 3, 5 |
| SET-R4-4 | Shortcut limited; D7 coexistence test; profile assets with the Codex floor | 4; assets 1 |
| Codex round 4 adversarial table | Alternate index, effective config, plumbing by effect, replayed trees, target index, mixed identity guidance, bootstrap wording | 2 |
| SET-R5-1 merge query | Pre-check refuses custom drivers, `merge.default`, filters and renormalization; sanitized query in a temporary repository; canary controls | 2; journey 5 |
| SET-R5-2 authority store | Account-derived root, no replacement store, read-failure states, admission pin, contract-less selector | 2; pin carrier 3 |
| SET-R5-3 initial acceptance | Three-way entry match, snapshot from the validated read | 3 |
| Round 5 costs | Pull replacement named; enrollment in the same operation; nested inventory | 2, 5 |
| REC items (v8) | Grok consult posture; D10 fixture route; `codeflow integrate` and `gh pr merge`; operator-only hook trust; exclusion reasons; opt-in `families`; guard-readiness naming | 2, 3, 4, 5 |
| Round 6 (v9) | Fetch destinations as a union in git's precedence; admission tuple in receipts and acceptance, `pin-unsupported` routes; AC-19 driver resolution by definition and source-specific controls; Grok trust before the sandbox | 2, 3, 5 |
| Round 8 (v12) | Workflow pushes judged against the destination's advertised refs; the queue writer's safe-path boundary; status reconciliation and host-kept dependencies; slash normalization and session staleness | 2, 3, 4, 5 |
| Round 7 and Grok round 5 (v11) | Fixture eligibility by real path and git routing; fixture admission record, tuple and trial identity; `gh pr merge` by landing mode with the server-side invariant; approvals bound to repository, source, target and method; Grok seats launched in their own worktree | 1, 2, 3, 5 |
| Operator decisions (v10) | D7 decided grammar with launch-time folder trust, Escape and the three slash commands; D8 fail closed with the session-start contract check and exact-command refusals; D9 agent token with workflow pushes queued; the operator-actions list | 1, 2, 3, 4, 5 |
| SET-R3-5 sequencing | Existing-harness capability checks run before dispatch; acceptance tests of guards a unit builds run after it | all |

---

## 5. The split

```
  unit 1  presets, action table, Codex profiles, update engine
     |
     v
  unit 2  guard families, baseline, binding  (after TSK-141 and unit 1)
     |
     v
  unit 3  guard runtime: fail closed, contract, guard-readiness preflight, readiness,
          delegate, doctor
     |
     v
  unit 4  retry and Herdr grammar  (spike 0 and the retry canary first)
     |
     v
  unit 5  postures, skills, journey, closeout, ADR
          (the D1 spike uses unit 1's cf-builder asset)
```

Caption: the five tasks and their order. Unit 2 reads unit 1's policy keys,
so unit 2 merges after unit 1. The two can be drafted and reviewed in
parallel and ship in the same release as separate pull requests on the
integration branch.

| Unit | Scope | Depends on |
|---|---|---|
| 1 | Action table and generator, Claude presets, Codex rules, the Codex `cf-guard` and `cf-builder` profiles with the proxy, Grok sandbox profile, manifest minimal tier, three-way array merge and the scalar baseline rule, parse test, release note skeleton | none (TSK-107 for the schema) |
| 2 | Guard families: privilege (D5), outward, discard, integrity with normalization, interpreter scan, secret reads, headless switch (D4), queued workflow pushes (D9); the enforcement baseline, the authority root, the binding registry with exclusions, effective context resolution and the merge query (section 2); `gh pr merge` by landing mode (2.2.1); fixture confinement and admission identity (2.6); the operator-actions writer | TSK-141; unit 1 for the policy keys |
| 3 | Guard runtime: fail-closed wrapper (D8) naming the exact command, `--contract` on every hook, the session-start contract check, the D8 agent guidance and no fail-open switch, the guard-readiness preflight, acceptance and readiness record (section 1), delegate settings deny and `--settings` hash, doctor | units 1 and 2 |
| 4 | Retry: spike 0 and the canary first; the argv grammar, pinned identity and PATH binding; the decided D7 Herdr grammar and list | unit 3; spike 0 |
| 5 | Postures and skill text (D1 to D3, D7 route, D8 and D9 guidance), launch-time folder trust, the operator-actions list criteria, the live journey, the closeout matrix, the ADR | units 1 to 4 |

---

## 6. Round 3 dispositions (v5, unchanged)

| Finding | Disposition |
|---|---|
| SET-R3-1 SessionStart proves nothing about PreToolUse | Fixed: section 1; acceptance tightened in v6 (1.2, 1.3) |
| SET-R3-2 drift authority can move without a working-copy difference | Fixed: section 2; lookup bound in v6 (2.3) |
| SET-R3-3 executable resolution | Criterion, unit 4 |
| SET-R3-4 wildcard versions | Criterion, unit 4; `configs-v6` policy pins `1.0.6` with digest fields |
| SET-R3-5 unverified rows can survive closeout | Criterion: closeout matrix, unit 5 and per unit |
| R2-1 to R2-10 | As v5 section 6 |

Round 4 dispositions are the table at the top of `proposal-v6.md`; round 5 dispositions are the table at the top of `proposal-v7.md`.

---


## 7. Configurations

`evidence/permission-presets/state/operator-actions.example.jsonl` adds the parsed tuple
fields to `workflow-push` lines and an explicit-destination example;
the review record's diff shows the change.
Every other sample is as listed in this record section 7.

---

## 8. Operator decisions D7 to D10 (decided 2026-09-28)

All four were decided on 2026-09-28, D7 to D9 as adjusted (RP lines
537-541). This section states what each now means for the design.

**D7, Herdr for a sandboxed primary.** Section 3 has the grammar, 1.5 the
launch-time folder trust, and section 0 the kept 2026-09-22 authorization.
The Codex fallback (EPC-018 falls back to the Herdr interactive seat when
the plugin login is stale; E18 `EPC-018.md:418-420`, per REC L4) now has
its route from a sandboxed Claude primary.

**D8, fail closed at every tier.**

```
 session start                       a guarded call later
 -------------                       --------------------
 session-orient --contract N         hook wrapper --contract N
   |                                   |
   binary range vs project N           exit 0 -> allow, 2 -> refuse
   |                                   other exit, 127, rejected flag
   match -> digest as usual              -> refuse (exit 2), naming the
   mismatch -> first digest line:           exact command, "queued for
     outdated side, exact command,          the operator"
     "queued for the operator",
     options; entry on the list      agent: continue work needing no
                                     guarded tool; after the operator
                                     confirms, `codeflow doctor`
```

Caption: where D8 shows up. The same entry is recorded once, whichever
point meets it first.

- **Every tier, every hook.** The fail-closed wrapper and `--contract` on
  every hook, `session-orient` and `session-summary` included (1.2, unit 3
  AC-1).
- **The exact command.** A binary older than the project's contract: the
  install command for the channel the generating binary records, or the
  documented install forms (E20 `docs/adoption.md:317-321`: the installer,
  `cargo install --path crates/codeflow-cli`, or `gh release download
  vX.Y.Z`); there is no self-updater (same lines). A project older than the
  binary's supported range: `codeflow update` in the named root. Both are
  operator actions: `codeflow update` rewrites enforcement paths, which the
  baseline treats as an approval step (2.2).
- **The options** are update, let the agent continue other work, or roll
  back the binary (reinstall the last version that supports the project's
  contract, by the same install forms).
- **No fail-open switch.** CodeFlow has no variable, flag or key that turns a
  guard that cannot run into an allow, and unit 3 AC-12 tests the absence. A
  harness's own hook disable is the operator's act outside CodeFlow; the
  digest and doctor report it and readiness excludes the route. This
  replaces v9's "last resort" wording in unit 3's recovery line.
- **What the agent can still do.** With a failed guard hook every Bash, edit
  and write call is refused, so "work that needs no guarded tool" is
  reading, searching, research, planning and drafting in the reply. The
  skills say so, and say to interrupt only when nothing else can proceed.

**D9, route A.**

- The agents use their own fine-grained token, exported as `GH_TOKEN` in
  the primary's and seats' launch environment, with no workflow,
  administration or gist permission; D6 already removed `delete_repo` from
  the agents' token (RP line 536). The permission names are GitHub's
  (repository Contents, Pull requests, Issues, Workflows, Administration;
  account Gists), from GitHub's fine-grained token documentation, not
  rechecked this pass.
- A GitHub App is documented in `docs/adoption.md` as the team route (unit 5
  AC-4). Nothing ships for it.
- The operator's own `gh` login stays on this host for setup and workflow
  pushes. **Residual, stated:** the token narrows what agents use by
  default; it does not stop a deliberate read of a stored login beyond the
  refused forms (the `gh auth` forms of the account family, the keychain
  family, and unit 2 AC-23's credential-override refusal).
- **Precedence, verified for gh.** `GH_TOKEN`, then `GITHUB_TOKEN`, "takes
  precedence over previously stored credentials" for github.com (gh
  2.67.0, `gh help environment` lines 1-3, read with an empty
  `GH_CONFIG_DIR` because the sandbox denies the real one). So `gh` in an
  agent session uses the agents' token even with the operator's login
  stored. **git over HTTPS is not covered by that sentence:** git asks its
  configured credential helpers in order, and a keychain helper holding
  the operator's credential can answer before `gh auth git-credential`
  does. Unit 5 AC-4 checks it against the local fake service: with
  `GH_TOKEN` set and an operator credential stored, `git push` over HTTPS
  must present the agents' token; if it does not, the release note and
  `/cf-customize` name the helper order to set. (git runs the helper as a
  child process, so the account family's `Bash(gh auth git-credential*)`
  deny does not stop it; that deny covers only a direct call.)
- **Workflow changes never stop the agent.** A push that would introduce
  a commit under `.github/workflows/` to its destination is refused before
  it transfers anything, queued for the operator with the exact push
  command, and the agent carries on; the message names moving the workflow
  commit to its own branch so the rest pushes (unit 2 AC-23). Which
  commits a push introduces is defined in 8.1. The coordinator measured 2 of 326 merged pull
  requests since August touching `.github/workflows` (coordinator's
  measurement, not rechecked here), so the queue stays short.

**D10, option A**, as recommended in v9 and unchanged: one operator-admitted
fixture root (2.6, unit 2 AC-22, unit 3 AC-10). One addition from D7: a
Claude seat in a fixture needs the fixture's folder trust, which has no
launch flag (1.5), so the fixture-root step also writes the per-folder
trust keys for the fixtures it admits, as part of the same queued operator
action. Whether the eval kit can pre-create its trial repositories so that
one step covers a full run is unit 5 AC-2's row; if it cannot, Claude duo
cases in fixtures record one queued trust action per trial and the rest of
the run continues.

**D10 options, for the record** (v9, unchanged).

| Option | What it means | Consequences |
|---|---|---|
| **A. Admitted fixture root (decided)** | One operator admission of a harness-created temp root; trials use their own policy for local rules; security families at least the built-in floor; every remote-affecting action refused except to a local repository inside the root; `ready (fixture)` seats; the kit preserves results and then cleans up | TSK-139 runs unattended after one admission; the policy under test reaches the guards; duo cases get real seats. Remote refusal costs no case: the shipped fixtures have no network remote and use an offline `gh` stand-in (E18 `.agents/skills/cf-evaluate-model/resources/fixtures.json:2088`) |
| B. Per-trial admission | Every trial's init binds through the operator | One operator action per case per repeat |
| C. Built-in policy only | v7 as written, plus a fixture readiness label | Cases that test a fixture's own policy (E18 `fixtures.json:2589`, per REC R4) give wrong results |
| D. Evaluate outside agent sessions | The operator runs trials from a terminal with hooks off | A different cohort under the evaluation protocol (E18 `.../cf-evaluate-model/resources/protocol.md:13-16`, per REC F3) |

### 8.1 Which commits a push introduces (SET-R8-1)

v11 subtracted the commits reachable from the destination's
remote-tracking ref. That ref is a local record of an earlier fetch, not
the server's state (`git-push(1)` distinguishes the remote ref it updates
from the local tracking ref). Two cases broke it: a first push of a new
task branch has no tracking ref, so every inherited workflow commit looked
new; and a destination rewound on the server from W to A, with the source
S built on W, re-sends W while a scan of `W..S` sees nothing.

**Destinations are resolved as git resolves them.** For each `git push`,
the guard resolves, from the effective config and without running
anything: the remote (explicit; else `branch.<name>.pushRemote`,
`remote.pushDefault`, `branch.<name>.remote`, `origin`), every push URL
(`remote.<name>.pushurl`, else `url`, after `url.<base>.pushInsteadOf` and
`insteadOf`), and every (source commit S, destination ref D) pair from the
command's refspecs, else `remote.<name>.push`, else `push.default`
(`--all` gives one pair per branch; a deletion `:D` introduces nothing).

**Evidence is the destination's own ref list, read-only.** For each push
URL the guard reads the server's advertised refs with one ref listing
(`git ls-remote <url>`). That listing transfers no objects and updates no
ref, and it is not the push being judged; the push itself is refused
before it transfers anything. It runs in AC-19's sanitized context:
repository-local config is dropped, the URL is the one resolved above,
and a destination that needs a repository-selected program (a local
`credential.helper`, `core.sshCommand`, an `ext::` or `fd::` transport) is
refused. Local remote-tracking refs are never used as evidence.

**The comparison.** For each pair, the introduced set is the commits
reachable from S and from no advertised tip that is present locally. A
commit in it whose `.github/workflows/` entries differ from those of every
parent (a root commit: that has any) makes the push a workflow push; a
merge that only brings in a parent's existing workflows is not its own
change.

| Destination | Evidence | Outcome |
|---|---|---|
| exists, tip present locally | its tip and every other advertised tip | the introduced set above; a rewound D (tip A) re-sends W, so W is in the set |
| new, or deleted on the server | every advertised tip; D has none | the same formula: a branch from `main` excludes `main`'s history, so inherited workflow commits are not new |
| exists, tip absent locally | incomplete | refused, not queued: "fetch `<remote>` and retry; the destination has commits this checkout lacks" |
| listing unavailable (offline, auth failure, refused transport) | none | refused, not queued: "could not read the destination; retry". The push would need the same connection |
| another advertised tip absent locally | partial | the tip is ignored, which can only enlarge the set; if that enlarges it into a workflow hit, the push is refused and not queued, naming `git fetch` and the retry: an inconclusive scan never creates an operator task |

Caption: the workflow-push comparison per destination state. A refusal
for missing evidence is never queued, since fetching or retrying is the
agent's own step.

**One parsed tuple.** Classification, the queue entry, the host dependency
record (9.2) and reconciliation all carry the same parsed tuple (source
commit, resolved push URL, destination ref). Nothing recovers intent by
evaluating the queue's display command.

**The listing is evidence at one moment.** The guard's guarantee is that
the push introduces no workflow change relative to the refs the
destination advertised at the listing. If the destination moves between
the listing and the push (a concurrent rewind), that movement is outside
the check; the agents' token, which has no workflow permission, is the
backstop at the server, and the next status reconciliation reads the new
state.

| Case | Expected |
|---|---|
| a bare fixture remote whose `main` has workflow history; a new `task/x` from `main` plus one source-only commit; no tracking ref for `task/x` | passes |
| the same new branch with one commit adding `.github/workflows/ci.yml` | refused, queued |
| remote `task/x` rewound from W (a workflow commit) to A; local tracking still at W; S = W plus a source commit | refused, queued: W is introduced |
| remote `task/x` at W; S = W plus a source commit | passes |
| `remote.origin.pushurl` naming a second fixture remote that lacks W, fetch URL at a remote that has it | judged against the push URL: refused |
| the listing fails | refused, not queued, naming the retry |

---

## 9. The operator-actions list

Every step that needs the operator goes on one list (RP line 541). The
agent records it and continues; it interrupts the operator only when no
remaining work can proceed without an open entry, and then names those
entries.

```
 guard refusal ---------.                         codeflow status
 (queued for the        |                         orient digest
  operator)             v                         orchestrator status
                .codeflow/operator-actions.jsonl ---> report (hourly)
 agent meets an ------>  one JSON line per action        |
 operator step          (open entries de-duplicated      v
 (codeflow              by key)                       operator acts,
  operator-actions add)                               marks done
                                                         |
                                  guard re-checks the live condition;
                                  the entry itself grants nothing
```

Caption: how an operator step is recorded, shown and resolved.

- **Where it lives.** `.codeflow/operator-actions.jsonl` in the main
  checkout, found through the common git directory so every worktree
  writes the same file; listed in the managed `.gitignore` block, never
  committed, and not an enforcement path (it decides nothing), so writing
  it needs no approval. Because agents can write it and host-side guards
  append to it, the writer has a safe-path boundary (9.1). A writer that
  cannot record gets a refusal saying the action was not recorded, and its
  host records it or reports it in the reply.
- **What an entry holds.** The file is an append-only event log: a line
  records an action (`id`, `key`, `kind`, `subject`, `command`, `why`,
  `blocks`, `recorded_by`, `recorded_at`) or an event on one (`id`,
  `event`: `updated`, `done` or `withdrawn`, with `at` and `by`); the
  row per `id` is the fold of its action line and later events
  (`evidence/permission-presets/state/operator-actions.example.jsonl`; unit 5 AC-8). A
  `workflow-push` line carries the parsed tuple as fields. No line is ever rewritten in place.
- **What goes on it.**

  | Kind | Recorded by | Command or step |
  |---|---|---|
  | `herdr-keys`, `seat-approval-prompt` | unit 4 refusal, or the host after `agent wait` | the operator answers in the seat's pane |
  | `folder-trust` | the host, when a trust step cannot run (Grok before AC-2 qualifies a form; a Claude fixture) | the trust step for the named folder |
  | `binary-update`, `project-update`, `binary-rollback` | D8 wrapper and `session-orient` | the install command, `codeflow update`, or the rollback install |
  | `workflow-push` | unit 2 AC-23 | the exact `git push` |
  | `enforcement-approval` | unit 2 refusals | `codeflow baseline approve --merge <source> --into <target>` |
  | `codex-hook-trust` | readiness `hooks-untrusted` | review and trust the hooks in Codex (1.6) |
  | `fixture-root`, `enrollment` | unit 2 refusals, `delegate wait` | `codeflow baseline bind --fixture-root <path>`; `codeflow update`, `init` or the first `baseline approve` |

- **Where it is shown.** `codeflow status` and the orient digest list the
  open entries with the count. The orchestrator's status report to the
  operator, the hourly status, leads with open entries and those added
  since the last report; the orchestrator skill at E20 has no such step, so
  unit 5 adds it.
- **It grants nothing.** No guard reads an entry as approval; each re-checks
  the live condition. A row marked `done`, or deleted, does not hide a live
  blocker either (9.2).
- **Wording.** Every refusal that records an entry says "queued for the
  operator" and names the command.

### 9.1 The writer's safe-path boundary (SET-R8-3)

The queue is agent-writable and outside the enforcement identity, while
Claude and Codex guards run on the host outside the seat's sandbox. A plain
path-based append would follow `.codeflow/operator-actions.jsonl`, or
`.codeflow`, replaced by a link to a host file the sandbox cannot write,
and a refused command would cause an out-of-sandbox write. So one writer,
used by guards and by `codeflow operator-actions add`, opens the file this
way:

1. **Bind to the validated main checkout.** The root is the one the guard
   already resolved for the command: the bound root from the binding
   registry, the trial under an admitted fixture root, or, for a
   repository that is not enrolled, the parent of its resolved common git
   dir. The writer opens it by real path as a directory, then re-resolves
   the real path and compares device and inode with the open handle; a
   difference means it was swapped, and nothing is written.
2. **Walk down without following links.** `.codeflow` is opened relative
   to the root handle as a directory with no link following, and the queue
   file relative to that handle with no link following, for append, and
   created with mode 0600 when absent. A link at either step fails the
   open.
3. **Check the opened file, not the name.** On the open handle: a regular
   file (a FIFO or device is refused, and the open is non-blocking so a
   FIFO cannot hang the guard), one link only (a hard link aliasing another
   file is refused), owned by the effective user, on the root's device.
4. **Serialize.** An exclusive lock on that same handle for the read,
   de-duplication and one-line append; no separate lock file, which could
   itself be redirected. A rename or replacement after step 2 changes
   nothing, since every later step uses the handle.

On any failure the writer writes nothing and returns the reason. The guard
keeps its original refusal and exit code and adds one line: "not
recorded: <reason>; the host records it or reports it in the reply".

**Order of operations, so an outside file is never written.** Before
opening, the writer reads the name's metadata relative to the directory
handle without following links (`fstatat` with no-follow) and requires a
regular single-link file owned by the user, or no file. Only then does it
open for append with no link following and non-blocking, and it repeats
the checks on the open handle, requiring the same device and inode as the
first read. A file known to fail the checks is never opened. A race that
swaps in a hard-linked alias between the two reads can at most get a file
opened append-capable, which the second check rejects and closes with no
byte written, so the canary's content and modification time stay
unchanged in every case. The tests below run against the real host-side
writer the guards call, not a stand-in. On a platform where the
writer cannot make these checks (native Windows until qualified), it does
not write and says so.

| Case | Expected |
|---|---|
| the queue file replaced by a link to an inert canary outside the sandbox's write set, then a refused command | the same refusal plus "not recorded"; canary content and times unchanged |
| `.codeflow` replaced by a link to an outside directory | the same |
| the queue file a hard link to an outside canary | not recorded; canary unchanged |
| the queue file a FIFO | not recorded; the guard returns without waiting |
| the file swapped for a link between the root check and the open (test hook) | refused open or append to the original file; canary unchanged |
| a hard link to the canary created between the first metadata read and the open (test hook) | closed with no byte written; not recorded; canary content and times unchanged |
| a normal refusal in a clean checkout | one line appended; mode 0600 on creation |
| two refusals at once | two complete lines, one row per key |

### 9.2 Status reconciles; deletion is not completion (SET-R8-2)

The file is editable, so a row marked `done`, or deleted, before the
operator acts must not make an outstanding dependency disappear from
status and the hourly report. `codeflow status` shows each dependency with
its evidence source:

- **Reconciled from live evidence.** `binary-update`, `project-update`,
  `binary-rollback` (contract check), `enforcement-approval` (the landing
  check), `codex-hook-trust` (readiness), `enrollment` and `fixture-root`
  (binding state), and now `workflow-push`: for each local task branch and
  each branch of a live worktree, status runs the 8.1 classification
  against its push destination. The dependency is outstanding while the
  recorded source commit is not reachable from the destination's advertised
  tip, and complete once it is, which closes the row with a `done` event
  by `status`. A row marked `done` while the evidence says outstanding is
  shown as outstanding ("marked done; destination lacks <commit>"); a
  deleted row is reconstructed from the evidence. When the listing is
  unavailable, status shows "unknown: could not read the destination",
  never done.
- **Kept by the host.** `herdr-keys`, `seat-approval-prompt` and a
  `folder-trust` the harness store cannot show have no repository
  evidence. The same record also keeps every workflow-push requirement
  whose exact source and destination cannot be reconstructed, whatever
  its kind: A workflow push whose exact source commit and resolved destination (push URL and ref) cannot be reconstructed from the branch's configured push destination is kept, as that tuple, in the host dependency record outside the editable queue until evidence for that destination shows the source reachable from its advertised tip, or the operator explicitly withdraws it; reconciliation uses that tuple and never substitutes the branch's current default. This is not an approval: the guard still
  refuses the push itself. The host keeps the dependency in its own task record under the
  CodeFlow state root, the delegate record `delegate wait` already writes,
  not in the queue file. It stays outstanding until its evidence clears
  (`agent wait` shows the seat no longer blocked; the trust step's
  receipts load) or the operator confirms it to the host. Status and the
  hourly report show it with "the queue row was deleted or marked done;
  deleting from the queue is not completion".

| Case | Expected |
|---|---|
| a queued workflow push; the row marked `done`; no push made | status: outstanding, "marked done; destination lacks <commit>" |
| the same with the row deleted | status: outstanding, reconstructed from the branch |
| the operator pushes the branch | status: complete; a `done` event appended by `status` |
| a `seat-approval-prompt` row deleted while the seat still waits | status: outstanding from the host record, with the deletion note |
| `task/x` at S already on its configured `origin`; `git push backup task/x:refs/heads/review-x` refused and queued because `backup` lacks a workflow commit reachable from S; the whole row and its events deleted | status: outstanding for exactly (S, `backup`'s push URL, `refs/heads/review-x`), from the host record |
| the operator then pushes S to `backup` `refs/heads/review-x` | status: complete for that tuple; the host record clears it |

### 9.3 Queue details

- **Never run automatically.** `command` is display text. No CodeFlow
  component executes, pastes into a pane or pipes a stored command;
  status and the digest print it quoted. Test: a row whose `command` is
  `$(touch <canary>)`, shown by status and the digest, leaves the canary
  absent.
- **Rows are a fold.** An action line is folded with its later `updated`,
  `done` and `withdrawn` events, which carry only the fields they change;
  the row is the result of that fold, never the last line alone.
- **De-duplication keeps the current request.** A new request with an open
  `key` appends an `updated` event carrying its current destination,
  command and blocker, so the row shows the latest, never a stale one.
- **A failed D8 hook** cannot record: the writer is the binary that failed,
  and the agent's `codeflow operator-actions add` runs through the refused
  Bash tool. The wrapper says "not recorded"; the skill tells the agent to
  put the exact command in its reply to the operator and to record it once
  guarded tools work again.

The v9 hand-off table's rows (landing approval, Codex hook trust,
enrollment, workspace inventory, fixture root) are now kinds on this list.
The workspace inventory (`codeflow baseline bind --inventory`, 2.3) is a
one-time adoption step and is recorded as an `enrollment` entry.

---

## 10. Checks run for v13

- **Parse and generator.** As in v12, rerun with `evidence/permission-presets/` (results in
  the report).
- **Read this pass.** Codex round 9 (`../review-settings-codex.md`, "Round
  9").
- **Not run.** No writer, race test, listing, push or reconciliation.
