# Audit of the landed policy read with a baseline in place

This audit records, for TSK-1057, how each state of the policy authority
read behaves once an operator has recorded a policy baseline, and how the
no-remote rule applies. It was written on 2026-11-15 against the EPC-022
line at `5405cdb1b8`, after TSK-1077 made the remote's default branch at one
commit the only local policy authority. Each row cites the test that shows
it, in `crates/codeflow-core/src/hooks/policy_baseline_tests.rs`.

## TSK-1077's refusal states hold

The baseline is applied after the authority is read, so every refusal of
the read stands before any baseline is consulted.

| State | With a baseline | Test |
|---|---|---|
| `refs/remotes/<remote>/HEAD` unset | refused, as before: the operator sets the remote `HEAD` | `the_authority_refusals_hold_with_a_baseline` |
| remote `HEAD` malformed, outside the remote, or symbolic twice | refused, as before | covered by the existing `landed_policy` tests; the baseline is not read |
| empty namespace for a remote that has refs | refused, as before: run `git fetch` | `the_baseline_catches_each_command_run_outside_a_session` (`update-ref -d`) |
| several remotes, none `origin` | refused, as before | not changed |
| a branch or commit that cannot be read, an invalid policy | refused, as before | not changed |

## The no-remote rule

| State | Result | Test |
|---|---|---|
| no remote, with a baseline record for the repository | the baseline's policy is enforced; doctor names the removed remote | `with_a_baseline_and_no_remote_the_baseline_is_enforced` |
| no remote, no baseline record | labelled local only, as TSK-1077 reads it | `without_a_baseline_and_no_remote_the_state_is_local_only` |
| remote advertises no refs, with a baseline record for the same remote URL | the baseline's policy is enforced | covered by `apply` (`State::Labelled`) |
| remote advertises no refs, no baseline record | labelled initialization, as TSK-1077 reads it | not changed |

## Baseline states

| State | Result | Test |
|---|---|---|
| never initialized (no record, not in the ledger) | the authority read stands; doctor notes the missing baseline | `a_repository_never_initialized_reads_the_authority_with_a_note` |
| current (the ref is at the baseline's commit) | unchanged | `a_current_baseline_changes_nothing` |
| initialized, record missing | the policy check blocks | `a_missing_baseline_for_an_initialized_repository_blocks` |
| malformed record, or a digest that does not match its text | blocks | `a_malformed_baseline_blocks` |
| record writable by others | blocks | `a_baseline_others_can_write_blocks` |
| record for another repository (replay) | blocks | `a_baseline_replayed_from_another_repository_blocks` |
| another authority branch (re-pointed remote `HEAD`) | blocks | `a_baseline_for_another_authority_blocks` |
| another remote URL | blocks | `a_baseline_for_another_remote_url_blocks` |
| stale (the ref descends from the baseline's commit) | the stricter of the two; doctor names the refresh | `a_stale_baseline_enforces_the_stricter_of_the_two` |
| the ref does not descend from the baseline's commit | the baseline's policy | `a_non_descendant_ref_enforces_the_baseline` |

## What this does not cover

- The hosted verdict reads its policy from the default branch at one commit
  (ADR-0091) and `codeflow ci --policy-from` reads the commit it is given;
  neither reads the baseline, which lives on the operator's machine.
- A process outside the harness sandbox can write the baseline directory,
  as it can write any file of the account. The baseline closes a weakened
  ref; it does not defend against such a process.
- On a platform where the account's home is not read (outside Unix), no
  baseline applies and the read stands, labelled.
