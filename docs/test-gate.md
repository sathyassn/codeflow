# Test gate selection

## Concept

Every gate run executes every eligible target of its mode, unless `--only`
limits it to named targets. No local run record earns selection credit
(ADR-0089): the records under `~/.codeflow/gate-runs` and the build folder
can be written by any process of the user, an agent session and the
candidate's own tests included, so a record cannot prove that a check
passed. Records are reports: pull requests cite their run ids, and nothing
in CodeFlow reads one to decide what runs.

## Architecture

| Control | Behavior |
|---|---|
| `--since <rev>` | Accepted and narrows nothing; the output says so |
| `--all` | Executes every eligible target; landing candidates use this control |
| `--only <targets>` | Executes named targets and their prerequisites; never creates complete gate evidence |
| `--config <path>` | Reads the targets from that file instead of `.codeflow/test-config.json`, with no fallback to it or to stack detection; evidence binds to that file's bytes. A missing path, a symbolic link, or a file that is not regular or readable is refused before any target starts. CI can use it to run a trusted copy of the config against a candidate checkout (TSK-1020) |
| No selection flags | Executes every eligible target of the mode |

The pre-push hook runs no quick target (TSK-449), so cite
`codeflow test --mode quick` when the pull request is made ready. A run
records its selection with an empty `skipped` list; a target CI skips
(`ci_skip`) is listed in `ci_skipped` and makes the run incomplete.

Credit can return only from a recorder whose identity no agent session and
no candidate test can use, such as an attestation made by hosted CI. A
same-user key, an ownership or mode check, or a home-directory lookup is
not such a recorder. Before 3.2.0 a passed, clean, complete full record of
the base tree could skip a content target; such a record, and any written
since, is read by nothing.

### Where evidence is written

A run writes its record and each target's logs and report copies into
`<target>/codeflow-gate/<run id>/`, and a full run with a revision copies
that folder to `~/.codeflow/gate-runs/<repo>/<run id>/`. Every one of these
writes, and the gate's lock files, walks from a held handle of a trusted
root and never follows a symbolic link or a Windows junction or other
reparse point at any component below it. The trusted roots are the
project folder (for `target/` or a relative `CARGO_TARGET_DIR`), the
parent of an absolute `CARGO_TARGET_DIR`, and the parent of the CodeFlow
home, so a link at the build-output folder or at the home itself is
refused. Point `CARGO_TARGET_DIR` or `CODEFLOW_HOME` at the real folder
rather than a link to it. The run record and every durable file are
created exclusively, so a name planted there, a link or hard link
included, is refused rather than written through; each target's logs and
report copies are replaced atomically, never written through a link. The
copy refuses a link, a pipe or other special file, and a file with more
than one name inside the run folder. A target's report is opened through
the same walk from the project or the build-output folder and must be a
regular file with one name; a report outside both is refused. A refusal
names the path and fails the gate; nothing is written outside the
evidence folders.

The default full gate and both `codeflow integrate` modes execute every
target, as every run now does.
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
a requested target was skipped. Explicit caller exclusions through `--only` or mode
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
| `input_class` | `content`, `history`, `remote`, `time` or `external`; omitted means unclassified. Recorded only; no class is skipped (ADR-0089) |
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
| `unclassified` | No equality evidence; the record names the missing classification |

Environment equality evidence never stores raw values. A missing safe key
leaves the fingerprint out of the record. Key creation currently requires Unix ownership and permission
checks; native Windows records no fingerprint for sensitive dependencies. The key is
never copied into gate evidence. A variable supplied through target `env`
also needs a classification. Gate-generated environment inputs, such as the
run ID and start time, have no stable identity in a record.

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

The fields above are recorded as evidence in each run's record; they no
longer remove a target from any run (ADR-0089).

The evidence fields are recorded in
[ADR-0085](decisions/ADR-0085-gate-selection-requires-evidence-for-the-exact-c.md);
the withdrawal of selection credit in
[ADR-0089](decisions/ADR-0089-local-gate-runs-earn-no-selection-credit.md).
