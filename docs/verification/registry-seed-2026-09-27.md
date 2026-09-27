# Registry seed and uid backfill, 2026-09-27

## Scope

This is the verification record of the one-time seed of CodeFlow's own id
registry (SPC-013 R-24, R-111) and of the uid backfill of the records the
EPC-020 line carries from `main` (R-3, R-25). It is TSK-109 AC-2's evidence,
with `ids check` and the `id_registry_journey` run named in the task's
Closeout.

## The seed

| Fact | Value |
|---|---|
| Registry | `codeflow/registry` on `origin` (`sathyassn/codeflow-archive`) |
| Seed commit | `2669e5fdfd6c868c5851ae7b2652922389fc69c4`, the branch's orphan root, 2026-09-27 09:29:38 -04:00 |
| Commit subject | `seed: 253 ids (map sha256:051aa289c10aee530c16d730d513f477ee8879000592bedc3a154a2a55c7698d)` |
| Ids registered | 253: 73 ADR, 21 EPC, 13 SPC, 146 TSK (11 of them the legacy `TSK-NNN-MMM` ids at `ids/TSK/NNN-MMM.toml`) |
| Issuer | `seed` on every entry |
| Entries with `landed = "none"` | 3: ADR-0071, TSK-089 and TSK-123, registered from refs that never landed |
| Entries with a nonempty `mapped` list | 16 |
| Explicit map | [`evidence/tsk-109/seed-map.toml`](evidence/tsk-109/seed-map.toml), 17 ids |
| Map SHA-256 | `051aa289c10aee530c16d730d513f477ee8879000592bedc3a154a2a55c7698d` (the tracked copy hashes the same) |
| Binary | a release build of `integration/EPC-020-delivery-system` after the seed fix landed (PR 638, merge `cd0ac7e74`, 09:21:08), built at 09:21:57 in the session's line-binary target |
| Worktree | a detached checkout of the line at `cd0ac7e74` (`.worktrees/line-bin`), so the line's policy and hooks judged the push |

The map names the copies that are one record although no shared commit
proves it: EPC-001 and ADR-0001 to ADR-0014, whose files the rewritten
pre-flip history added again with identical content; ADR-0059, added twice
on `main` by two commits with the same subject; and TSK-093, carried by hand
onto the EPC-016 line with a changed target. The seed wrote a `mapped` entry
for 16 of the 17; ADR-0059's entry has `mapped = []` because the seed's
live-copies rule already decided its two copies (both are ancestors of
`main`), so its map pair had no effect. TSK-079 and ADR-0027 are not in the
map: their copies are different records, and the live-copies rule (R-111 as
amended by PR 634) registered the copy each live tip holds.

## The two runs

1. **Refused, nothing written** (log written 09:27:28). The seed ran from
   the root checkout, which is on `main`. The push of the registry branch
   went through that checkout's pre-push hook, which runs the installed
   `codeflow` on `PATH` (`~/.cargo/bin/codeflow`, built 2026-09-26 13:38).
   That build judged `codeflow/registry` as a code branch and refused it
   under `git.branch_naming`; the line's binary gives the registry its
   data-branch profile, which skips branch naming (R-6). The push failed,
   so nothing reached `origin`. The run's one-line log follows, wrapped,
   with the tool's two long dashes written as colons because this
   repository's policy refuses that character in added lines; one span is
   elided. The log stayed in the session scratchpad (SHA-256
   `838524875b4c4e21...`).

   ```text
   error: push to the registry was refused by a hook: codeflow pre-push:
   BLOCKED: policy rule git.branch_naming (block); branch 'codeflow/registry'
   does not match `{prefix}/{kebab-name}`; sanctioned: rename with a
   sanctioned prefix: feat/ fix/ docs/ refactor/ test/ chore/ ci/ hotfix/
   plan/ task/ spike/ experiment/ integration/; policy file:
   .codeflow/policy.json; codeflow pre-push: warning: policy rule
   git.test_gate_on_push (warn); quick test gate failed for: rust-workspace,
   model-eval-kit; ...; error: failed to push some refs to
   'https://github.com/sathyassn/codeflow-archive.git'
   ```
2. **Registered** (09:29,
   [`evidence/tsk-109/seed-run-2.log`](evidence/tsk-109/seed-run-2.log)). The
   same map from the line worktree with the line binary:

   ```text
   ids seed: registered 253 new id(s); 0 already registered
   registry: reserved on the authority
   map sha256:051aa289c10aee530c16d730d513f477ee8879000592bedc3a154a2a55c7698d (cite it in the verification record)
   ```

That the first run wrote nothing to the authority is shown by the registry's
history: its root commit is the second run's, and `ids check` reports no
damage. Whether it left a local commit in the root checkout before the push
was refused is not recoverable: the local branch's reflog starts at the seed
commit.

## After the seed

- The registry has grown only by issue since: TSK-140 to TSK-144 at
  09:59 to 10:00 (tip `0f89b3242` when this record was written).
- `ids check` passed with 86 collision warnings, all on stale branches that
  never landed. They were 36 distinct copies, and for each one the file its
  introducing commit added is byte-identical to the file the entry's
  `landed` commit added: cherry-picks and rewrites of the landed record,
  which R-111 counts as replicas by their landing. The check compared only
  the copy's own adding commit with `introduced`, `mapped` and `landed`,
  and never computed the copy's landing, so TSK-109 made it compute the
  landing the way the seed does. With that fix `ids check` passes with no
  warning; a different record under a registered id still warns off a
  landing line and blocks on one.
- `ids backfill` wrote 181 uids on the TSK-109 branch: every record under
  `project-management/` (116) and every ADR under `docs/decisions/` (65),
  one added frontmatter line each; `codeflow ci` bound all 181 to the
  registry. The backfill made each record's bytes differ from the migration
  baseline, which turned 34 completed pre-migration tasks into warnings, so
  TSK-109 also keeps a record's baseline exemption when its only change is
  the backfilled uid line (R-3).

## Not verified

- The exact commit the release binary was built from: the build time and
  the worktree's reflog place it at `cd0ac7e74`, but the binary does not
  record its commit.
- Whether any local-only branch in another clone holds a record the seed
  never saw; the seed read every ref of the maintainer's clone.
