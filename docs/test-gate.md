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

### A run with no comparison base

A quick or essential run on a branch with no task (`plan/`, `chore/`), on a
detached head, or on a task with no declared target has no comparison base.
It runs every target and prints `every target: local narrowing is
unavailable, since no local run record earns selection credit (ADR-0089)`.
A run given `--since <rev>` prints the same reason after `--since <rev>
narrows nothing`. Nothing in the output recommends `--since`, because it
cannot narrow a run. An unknown, renamed or deleted path, or a changed
config digest, also runs every target.

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
regular file with one name; a report outside both is refused. The same
walk removes a nextest target's stale report before it runs and checks
that each declared producer output exists, so a link above either is
refused instead of followed. A refusal names the path and fails the gate;
nothing is written or removed outside the evidence folders.

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
| `timeout_seconds` | Base deadline, default 600 seconds. Heavy targets have a ceiling four times this value; light targets retain a fixed deadline |
| `cleanup_grace_seconds` | Time allowed after a target stops for termination, reaping and pipe draining, default 5 seconds, from 1 through 3600. On Unix a supervised process group still running at half this time is killed; on Windows the target's job is terminated when it stops. Cleanup that does not finish is a non-pass |
| `stall_timeout_seconds` | Heavy-target inactivity window, default 300 seconds. Early stalls require valid scoped CPU accounting; otherwise the run explicitly uses its ceiling |

`timeout_seconds` and `stall_timeout_seconds` accept integers from 1 through 4611686018427386 seconds.
The loader and schema reject zero, fractional values and values beyond that
bound. A stall window greater than the ceiling is valid; the ceiling wins.
New stdout/stderr or increasing scoped CPU counts as activity. Process
existence alone does not. Repeating logs and computing loops cannot extend
the absolute ceiling. Every five minutes an unfinished heavy target prints
tests done (or unknown) and CPU state; these gate-generated lines do not
refresh its activity deadline.

Windows uses its target job's cumulative CPU accounting. Linux creates a
fresh per-target cgroup only within existing writable cgroup v2 delegation,
placing the launch child there before workload execution. Its CPU counter
covers short-lived and auto-reaped work. Early stalls use that accounting
without an additional containment launcher. If the group becomes unpopulated,
a population notification occurs while the launch child remains live, or that
child leaves its group, the run reports the anomaly and uses ceiling-only
monitoring. Unreadable accounting does the same. Tests are assumed not to
migrate cgroups; an undetected migration can cause a false stall, which fails
the gate. No delegation is provisioned and no host configuration changes.
Without delegation the status says `early stall unavailable: no delegated
cgroup`. macOS is ceiling-only, an accepted 4.0.0 limitation reported as
`early stall unavailable: macOS complete descendant accounting unavailable`.
Without a cgroup, Unix confirmation uses the direct child's exit, watcher
completion and pipe EOF. An uninterruptible group member with closed pipes
can outlive those observations; this is a pre-existing known limit, not a
claim of complete kernel accounting.

A stall reports `stalled: no progress for N s (workers W of M)`. The ceiling
reports `timed out at the ceiling after S s`. Both fail the gate with target
exit 124; dependents retain the prerequisite's stall or timeout cause.
An exit first observed after the ceiling cannot pass. Termination and pipe
collection have a cleanup grace, `cleanup_grace_seconds`, default five
seconds, including a descendant that keeps a pipe open. On Unix, at half the
grace a supervised group that has not ended is killed by the gate itself, so a
slow watcher on a loaded host cannot turn a passing target into a failure. On
Windows the target's job is terminated when the target stops. Incomplete cleanup remains a failure. Target summaries
and saved `result.json` reports retain monitoring mode, deadline cause and
`termination_unconfirmed`. Once membership or population evidence becomes
uncertain, a direct-child exit, closed pipes and an empty original group do
not prove escaped descendants ended. The run retains that uncertainty and
fails with unconfirmed termination. CPU-counter unavailability alone is a
separate ceiling fallback.

Group-record bookkeeping shares the remaining cleanup deadline across its
mutex and all record locks. Deferred filesystem work is not joined after
that deadline; an unconfirmed write remains a cleanup failure. Missed writes
leave stale records conservatively; a late completion cannot turn the result
into a pass. Related task TSK-1279 retains allocation and lease
for an unconfirmed target until termination is confirmed or the gate exits.

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

### Coverage rules and their defaults

A full run reads each target's coverage artifact and judges it by rules.
The rules come from one of two places:

```
target has a coverage block?
  no  -> nothing is measured; while defaults.coverage is not empty and the
         target is a test target, one warning names it and the missing
         producer, and its verdict is unchanged
  yes -> block sets rules?
           yes -> its own rules alone
           no  -> the rules in defaults.coverage, marked
                  "(rules from defaults.coverage)"; with empty defaults
                  the block only collects
```

- Defaults carry rules only. The producer, meaning the artifact's format
  and path and the full-mode command that writes it, is always the
  target's own `coverage` block.
- Inherited rules enforce exactly as a target's own rules do: a shortfall
  fails the gate, and so does a missing or unreadable artifact.
- A test target is one whose `runner` names a test runner (anything but
  `custom`) or that declares a test `report`.
- A `changed_files` rule needs a comparison base that standalone
  `codeflow test` lacks, so the loader refuses one in a target's own rules
  and in defaults that a target would inherit.
- Quick and essential runs measure no coverage, so neither the rules nor
  the warning apply there.

`codeflow test setup` writes a coverage block for each target it detects,
only when it creates the config. The detected target's full command runs
the stack's producer, and the block sets no rules:

| Stack | Producer | Artifact |
|---|---|---|
| Rust (`cargo`) | cargo-llvm-cov | `target/lcov.info` (lcov) |
| Python (`pytest`) | coverage.py | `coverage.xml` (cobertura) |
| Node (`vitest`) | `@vitest/coverage-v8` | `coverage/lcov.info` (lcov) |
| Node (`jest`) | Jest's own coverage | `coverage/lcov.info` (lcov) |
| Go | `go test -coverprofile` | `coverage.out` (go-cover) |

cargo-llvm-cov does not create its output's folder; cargo creates
`target/` for its build while `CARGO_TARGET_DIR` is unset, so with that
variable pointing elsewhere, move the Rust path and `--output-path` under
it. `codeflow update` never rewrites the test config, and the gate's own
detection for a project with no config keeps the plain test commands. A
full gate cannot pass until the producer its full command runs is
installed, so `codeflow doctor` prints one line naming a missing producer
and the command that installs it.
