# Test gate selection

## Concept

Local test runs skip a check only when a full candidate run proves the check that
would execute now. Unclassified checks continue to run. Selection does not
reuse a previous result as a new execution.

## Architecture

For quick and essential runs on a task branch, the default comparison is the
merge base of HEAD and the task record's `integration_target`. The output names
the commit and its source.
Detached HEAD, an unknown task or an unresolved line selects every eligible
target. The selector never substitutes `main` or the previous commit.

| Control | Behavior |
|---|---|
| `--since <rev>` | Resolves this explicit comparison to a commit and requires it to be an ancestor of HEAD |
| `--all` | Executes every eligible target; landing candidates use this control |
| `--only <targets>` | Executes named targets and their prerequisites; never creates complete gate evidence |
| No selection flags | Full executes every target; quick and essential resolve the task's declared line and use per-target evidence |

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

Skip credit is a local convenience, never landing evidence. Run records remain
mutable by their owner. Records must be owned by the effective user and must
not be group- or world-writable. Where those protections cannot be checked,
selection grants no credit. Setting `CODEFLOW_HOME` also disables selection
credit. On Unix the credit store is resolved from the effective user's account
database entry (`getpwuid_r(geteuid())`, the reentrant account lookup), and a
`HOME` different from that account home also disables credit with a printed
reason. Windows and other non-Unix platforms currently have neither the trusted
account-home lookup nor the record-ownership check: selection grants no credit,
prints `selection credit denied: account home unavailable`, and executes the
checks. `USERPROFILE` alone is mutable environment input and cannot establish
that boundary. A native profile lookup would also need a trusted record-owner
and write-permission check before credit could be enabled. A MAC under a sandbox `denyRead` key was considered and deferred;
these checks do not authenticate the record's contents.

The default full gate and both `codeflow integrate` modes execute every target.
An explicit `--since` can select a local full run; any skip leaves that run
incomplete and unusable as landing evidence.
Integration refuses failures, skips and incomplete runs. Its existing exception
applies when no target is eligible to run in full mode: no config and no
detectable stack, an empty target list, or declared targets that are all disabled
or have no full mode. Both integration modes then land with the loud warning
`WARNING: no tests ran; this merge is NOT test-verified`. No target was skipped
using candidate evidence in this case. For an explicit landing
command, use `codeflow test --mode full --all` and require a complete run.
`--only` remains a partial run for split CI jobs and cannot be landing evidence
on its own. Summaries show executed and skipped counts and say "not complete"
when any target was skipped or nothing executed. Successful quick and essential
runs with no skips say "not full-gate evidence". `--strict` exits non-zero if no target executed or
candidate evidence skipped any requested target. Explicit caller exclusions through `--only` or mode
membership are not skips and do not fail strict mode. In particular,
`--strict --all --only X` passes when its requested checks run successfully;
that split run remains incomplete as evidence for a full landing gate.

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

Environment equality evidence never stores raw values. A missing safe key makes
the target run. Key creation currently requires Unix ownership and permission
checks; native Windows runs targets with sensitive dependencies. The key is
never copied into gate evidence. A variable supplied through target `env`
also needs a classification. Gate-generated environment inputs, such as the
run ID and start time, always require execution when declared relevant.

Only non-empty values classified as `fingerprint` or `sensitive` suppress
raw child progress and are redacted from captured output and gate-owned report
copies. This includes configured `env` entries with those classifications.
Audited non-secret and unclassified values are left unchanged. Redaction covers
the following exact forms:

- Literal values and JSON string contents, including ASCII-only `\uXXXX`,
  HTML-safe `\uXXXX` for `&<>` with an optional escaped apostrophe, Unicode-escaped
  quotes, and every character encoded as `\uXXXX`.
- XML text entities, quoted attribute entities with a raw apostrophe or
  `&apos;`, `&#39;` or `&#x27;`, and decimal or hexadecimal numeric references
  for every character, XML text (`&<>`), XML attributes (`&<>"`) or HTML
  attributes (`&<>"'`), optionally including non-ASCII characters.
  Hexadecimal digits may be upper- or lower-case.
- URL percent encoding with `/` escaped or safe, upper- or lower-case hex
  digits, spaces as `%20` or `+`, and those forms percent-encoded a second time.
- Standard and URL-safe base64, with padding.

The gate never rewrites the tool's source report. Protection of that report and
other transformations are the producing tool's duty. Gate start and completion
messages remain visible.

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
