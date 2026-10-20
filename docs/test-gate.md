# Test gate selection

## Concept

The gate skips a check only when a full candidate run proves the check that
would execute now. Unclassified checks continue to run. Selection does not
reuse a previous result as a new execution.

## Architecture

On a task branch, the default comparison is the merge base of HEAD and the
task record's `integration_target`. The output names the commit and its source.
Detached HEAD, an unknown task or an unresolved line selects every eligible
target. The selector never substitutes `main` or the previous commit.

| Control | Behavior |
|---|---|
| `--since <rev>` | Resolves this explicit comparison to a commit and requires it to be an ancestor of HEAD |
| `--all` | Executes every eligible target; landing candidates use this control |
| `--only <targets>` | Executes named targets and their prerequisites; never creates complete gate evidence |
| No selection flags | Resolves the task's declared line and uses per-target evidence |

A qualifying record has a matching config digest and the comparison base's
Git tree ID. It must be passed, clean, complete and full. The Git tree ID is
separate from the existing digest of tracked working files. Two commits of one
tree can share evidence; a completion commit that changes a record cannot be
an exact-tree match to its parent.

The pre-push quick gate uses this selector. Its independent ref, authority,
range, release-preflight and checked-out-tree checks still run. Targets that
are skipped have their names and reasons in `selection`, and no executed
result. Any skipped target makes `complete` false, including a run that skips
every target.

## Technical

The config is `.codeflow/test-config.json`; its editor schema is
`.codeflow/test-config.schema.json`. Existing configurations remain valid and
continue executing their targets until their dependencies are declared.

| Target field | Meaning |
|---|---|
| `narrow` | Tracked input globs; empty declarations or patterns with no tracked match cannot establish equality |
| `input_class` | `content`, `history`, `remote`, `time` or `external`; omitted means unclassified. Only `content` can be skipped |
| `evidence.tools` | A map from tool names to executable-and-argument arrays that report identity, such as `["rustc", "--version", "--verbose"]` |
| `evidence.environment` | A map of all relevant inherited or configured variable names to the classifications below |
| `evidence.environment_complete` | `true` records an audit of the command and everything it invokes; `false` always executes |
| `evidence.membership` | An executable-and-argument array that reports test membership, or `[]` when no membership applies |
| `evidence.covers_modes` | Other modes, `quick` or `essential`, covered by this target's full command. The command must be identical; matching membership and other dependencies are still required |
| `requires` and `outputs` | Producer relationships and their output files. A missing or changed producer output executes the consumer and its prerequisites |

Tool and membership probes run in the target's working directory with its
configured environment. Their output is hashed for equality. Commands that
use Git history, remote state, time or other external sources declare that
input class and always run. File extensions alone do not establish a content
audit. Declared inputs include scripts, configuration and every file read
transitively by the check.

| Environment classification | Recorded evidence |
|---|---|
| `audited_non_secret` | A digest; this classification explicitly permits equality evidence that can be tested against guessed values |
| `fingerprint` or `sensitive` | HMAC-SHA256 under an owner-only key in the CodeFlow home, outside every repository and run record |
| `unclassified` | No equality credit; the target runs and names the missing classification |

No declared environment value is recorded verbatim. A missing safe key makes
the target run. Key creation currently requires Unix ownership and permission
checks; native Windows runs targets with sensitive dependencies. The key is
never copied into gate evidence. A variable supplied through target `env`
also needs a classification. Gate-generated environment inputs, such as the
run ID and start time, always require execution when declared relevant.

Targets with environment inputs retain gate start and completion messages,
but their raw child progress is not relayed. Captured stdout, stderr and
reports are redacted before display or retention. This protects declared
values; transformed or encoded secrets remain the producing command's duty.

Selection compares the same mode and command unless the identical-command
mode relation is declared. It also compares tools, OS, architecture,
environment fingerprints, test membership, producer outputs and file inputs.
The credited target's before and after observations must match. A full run
does not implicitly cover a quick-only or essential-only command.

Untracked, renamed, deleted, unmatched or submodule inputs widen execution.
Changed shared infrastructure in `execution.run_everything` selects every
eligible target. Failed, dirty, partial, corrupt or legacy records without a
Git tree ID cannot establish a skip. Each fallback names its reason.

The decision is recorded in
[ADR-0085](decisions/ADR-0085-gate-selection-requires-evidence-for-the-exact-c.md).
