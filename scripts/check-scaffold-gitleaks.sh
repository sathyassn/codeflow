#!/bin/sh
# What adopters get from the scaffold passes their secret scan (TSK-206,
# sathyassn/codeflow#13), and a pull request cannot pass it by exempting
# its own leak (TSK-210, sathyassn/codeflow#20). With the pinned gitleaks:
#   1. the shipped scaffold sources under assets/ hold nothing gitleaks'
#      default rules report; given a codeflow binary as the second
#      argument, fresh `codeflow init` output at each tier is scanned too;
#   2. the adopter CI template's own gitleaks step, run whole with bash -e as
#      GitHub runs it, drops only the 3.0.0 pipeline prose in its two
#      managed paths and still fails on a credential on that same line, the
#      same prose anywhere else, and every finding an adopter's own
#      configuration makes, wherever gitleaks reads that configuration from;
#      a scan that does not complete fails the step;
#   3. that step reads every exemption from the trusted commit: an ignore
#      entry, an allowlist, a .gitleaks.json beside .gitleaks.toml, an
#      inline gitleaks:allow comment or an edited extended file that a pull
#      request adds fails it, and the same exemption on the trusted commit
#      passes; an unreadable trusted commit fails it before it scans; links
#      the pull request plants are never written through or followed, and
#      no Python module or .gitattributes it commits steers the scan; a
#      secret added in a merge resolution or in a file that replaces a link
#      is reported, as is one in a file git judges binary, under its own
#      path, whatever git configuration the runner inherits; merges report
#      nothing twice; in the history HEAD holds and the trusted commit does
#      not, an octopus merge and a path gitleaks cannot read reliably (a
#      backslash, a double quote or a control character), in a commit or on
#      either side of a merge, are refused with the commit and its refs
#      named, as is a git older than 2.41; spaced and non-ASCII names pass;
#      branches and tags HEAD does not reach neither refuse nor fail it;
#   4. this repository's own gitleaks step is the template's, and in both
#      workflows the secret-scan job runs only the checkout before it.
# The step's download is served from a local archive of the gitleaks under
# test, so no network is needed; its pinned checksum is the Linux release's
# and is not checked here.
# Usage: check-scaffold-gitleaks.sh [gitleaks] [codeflow]
# CODEFLOW_CI_TEMPLATE and CODEFLOW_CI_OWN name another template and own
# workflow to check, for example earlier revisions to show the cases they
# fail; every case still runs and each failure is listed at the end.
set -eu
# The finding lists below are word-split on purpose.
# shellcheck disable=SC2086

GITLEAKS=${1:-gitleaks}
CODEFLOW=${2:-}
absolute() { case $1 in /*) printf '%s' "$1" ;; */*) printf '%s/%s' "$(pwd)" "$1" ;; *) command -v "$1" ;; esac; }
GITLEAKS=$(absolute "$GITLEAKS")
[ -z "$CODEFLOW" ] || CODEFLOW=$(absolute "$CODEFLOW")
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
. "$ROOT/scripts/fixture-git-env.sh"
TEMPLATE=${CODEFLOW_CI_TEMPLATE:-$ROOT/assets/base/ci/codeflow-ci.yml}
OWN=${CODEFLOW_CI_OWN:-$ROOT/.github/workflows/codeflow-ci.yml}
TMP=$(mktemp -d "${TMPDIR:-/tmp}/scaffold-gitleaks.XXXXXX")
trap 'rm -rf "$TMP"' EXIT HUP INT TERM
FAILURES="$TMP/failures"
: >"$FAILURES"
failed() { # case name, then what went wrong
  printf '%s: %s\n' "$1" "$2" >>"$FAILURES"
  printf 'template secret scan, %s: %s\n' "$1" "$2" >&2
}

printf '[extend]\nuseDefault = true\n' >"$TMP/default.toml"
no_findings() { # label, then the directory to scan with the default rules
  "$GITLEAKS" detect --no-git --source "$2" --config "$TMP/default.toml" \
    --no-banner --redact --report-format json --report-path "$TMP/plain.json" \
    --exit-code 0 >/dev/null 2>&1
  python3 - "$TMP/plain.json" "$1" <<'PY'
import json
import sys

findings = json.load(open(sys.argv[1], encoding="utf-8"))
if findings:
    found = sorted((f.get("RuleID"), f.get("File"), f.get("StartLine")) for f in findings)
    raise SystemExit(f"{sys.argv[2]} fails gitleaks' default rules: {found}")
PY
}
no_findings "shipped scaffold sources" "$ROOT/assets"
if [ -n "$CODEFLOW" ]; then
  for tier in minimal standard full; do
    mkdir "$TMP/init-$tier"
    git -C "$TMP/init-$tier" init -q -b main
    (cd "$TMP/init-$tier" && "$CODEFLOW" init "--$tier" --yes >/dev/null 2>&1)
    no_findings "a fresh --$tier scaffold" "$TMP/init-$tier"
  done
fi

# The template's gitleaks step, whole. It must read the trusted commit
# through env, never interpolate it into the script, and match this
# repository's own step. In both workflows the secret-scan job runs nothing
# before it but the checkout: an earlier step that ran checkout code could
# write GITHUB_ENV or GITHUB_PATH, and the scan would honour them.
python3 - "$TEMPLATE" "$OWN" "$TMP/step.sh" <<'PY' >>"$FAILURES"
import re
import sys

template, own, out = sys.argv[1:]


def before_scan(path):
    """Problems with what the secret-scan job runs before its gitleaks step."""
    lines = open(path, encoding="utf-8").read().split("\n")
    start = lines.index("  secret-scan:")
    end = next((i for i in range(start + 1, len(lines)) if re.match(r"  \S", lines[i])), len(lines))
    job = lines[start:end]
    if "      - name: gitleaks" not in job:
        return [f"{path}: the secret-scan job has no gitleaks step"]
    before = job[:job.index("      - name: gitleaks")]
    steps = [line for line in before if line.startswith("      - ")]
    problems = []
    if steps != ["      - uses: actions/checkout@v6"]:
        problems.append(f"{path}: the secret-scan job runs {steps} before gitleaks, not the checkout alone")
    if any(line.strip().startswith(("run:", "shell:", "env:")) for line in before):
        problems.append(f"{path}: the secret-scan job sets a run, shell or env before gitleaks")
    return problems


def step(path):
    lines = open(path, encoding="utf-8").read().split("\n")
    start = lines.index("      - name: gitleaks")
    body = []
    for line in lines[start + 1:]:
        if line.strip() and not line.startswith(" " * 8):
            break
        body.append(line)
    while body and not body[-1].strip():
        body.pop()
    run = body.index("        run: |")
    return body[:run], "\n".join(line[10:] for line in body[run + 1:]) + "\n"


env, script = step(template)
if '"$report"' not in script:
    raise SystemExit("the template's gitleaks step reads no report, so the "
                     "3.0.0 pipeline line fails every adopter's secret scan")
open(out, "w", encoding="utf-8").write(script)
trusted = "          TRUSTED_SHA: ${{ github.event.pull_request.base.sha || github.sha }}"
if env != ["        env:", trusted]:
    print(f"step: the trusted commit is not passed as env {trusted.strip()!r}: {env}")
if "${{" in script:
    print("step: the script interpolates an expression instead of reading env")
if step(own) != (env, script):
    print("own workflow: its gitleaks step differs from the template's")
for path in (template, own):
    for problem in before_scan(path):
        print(f"job: {problem}")
    # Nor does the step itself run anything from the checkout.
    text = step(path)[1]
    if re.search(r"python3(?! -I )", text):
        print(f"step: {path}: a Python helper runs without -I, so the checkout is on its import path")
    if re.search(r"""(^|[\s"'])(\./|scripts/)""", text, re.M):
        print(f"step: {path}: the step runs a path from the checkout")
PY
if [ -s "$FAILURES" ]; then
  cat "$FAILURES" >&2
fi

# The step downloads the release archive and checks it; here the download is
# the gitleaks under test, packed the same way.
mkdir "$TMP/archive" "$TMP/fake-bin"
cp "$GITLEAKS" "$TMP/archive/gitleaks"
tar -czf "$TMP/gitleaks.tar.gz" -C "$TMP/archive" gitleaks
cat >"$TMP/fake-bin/curl" <<SH
#!/bin/sh
while [ \$# -gt 0 ]; do
  case \$1 in -o) out=\$2; shift ;; esac
  shift
done
cp "$TMP/gitleaks.tar.gz" "\$out"
SH
cat >"$TMP/fake-bin/sha256sum" <<'SH'
#!/bin/sh
cat >/dev/null
echo "gitleaks archive: OK"
SH
chmod +x "$TMP/fake-bin/curl" "$TMP/fake-bin/sha256sum"
FAKE_PATH="$TMP/fake-bin:$PATH"

PROSE=$(printf 'Cover secret/PII exposure, authz gaps, %s%s, general vuln classes' \
  'vulnerable/' 'malicious deps')
KEY=$(printf '%s%s' '9fK2pQ7xLm4R' 't8Wz1Vb6Nc3H')
PLANTED="api_key = \"$KEY\""
init_repo() { # a fresh repository at $REPO that never commits a download
  mkdir -p "$REPO"
  git -C "$REPO" init -q
  printf '/gitleaks\n/gitleaks_*.tar.gz\n' >>"$REPO/.git/info/exclude"
}
commit() {
  git -C "$REPO" add -A
  git -C "$REPO" -c user.name=canary -c user.email=canary@example.invalid \
    -c commit.gpgsign=false commit -q -m "$1"
}
tip() { git -C "$REPO" rev-parse HEAD; }

REPO="$TMP/adopter"
for path in .claude/workflows .codeflow/.baseline/.claude/workflows; do
  mkdir -p "$REPO/$path"
  # Line 2 is the 3.0.0 prose; line 3 is the same prose with a credential
  # on the same line.
  printf '// seeded by CodeFlow 3.0.0\n%s\n%s %s\n' "$PROSE" "$PROSE" "$PLANTED" \
    >"$REPO/$path/pipeline.workflow.js"
done
printf '%s\n' "$PROSE" >"$REPO/notes.md"
init_repo
commit scaffold

# One case: a name, the exit status the step must end with, then the
# "file:line" findings it must name, exactly. The step runs in $REPO with a
# runner temp directory of its own. The trusted commit is $TRUSTED when set
# (a pull request's base), else the tip (a push). Extra environment is
# passed through STEP_ENV; MESSAGE is text the output must hold, required
# when the step must fail without a finding; NO_SCAN
# says gitleaks must not have run; after_step checks the checkout before
# it is restored.
after_step() { :; }
expect() {
  name=$1
  status=$2
  shift 2
  runner="$TMP/runner-$name"
  mkdir "$runner"
  set +e
  (cd "$REPO" && env PATH="$FAKE_PATH" RUNNER_TEMP="$runner" \
    TRUSTED_SHA="${TRUSTED-$(tip)}" ${STEP_ENV:-} bash -e "$TMP/step.sh") \
    >"$TMP/$name.out" 2>&1
  rc=$?
  set -e
  if ! problem=$(python3 - "$TMP/$name.out" "$status" "$rc" "$KEY" "$@" 2>&1 <<'PY'
import re
import sys
from urllib.parse import unquote

out = open(sys.argv[1], encoding="utf-8").read()
status, rc, key = int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
expected = sorted(sys.argv[5:])
found = sorted(f"{unquote(f)}:{l}" for f, l in re.findall(r"^::error file=([^,\n]+),line=(\d+)::", out, re.M))
problems = []
if (rc == 0) != (status == 0):
    problems.append(f"exit {rc}, expected {'0' if status == 0 else 'non-zero'}")
if found != expected:
    problems.append(f"findings {found}, expected {expected}")
if key in out:
    problems.append("the log shows a credential's value")
if problems:
    raise SystemExit("; ".join(problems))
PY
  ); then
    failed "$name" "$problem"
  fi
  # A failure with no finding must be the intended one, so an unrelated
  # error cannot satisfy the case.
  if [ "$status" -ne 0 ] && [ $# -eq 0 ] && [ -z "${MESSAGE:-}" ]; then
    failed "$name" "the case expects a failure without a finding but names no diagnostic"
  fi
  if [ -n "${MESSAGE:-}" ] && ! grep -qF -- "$MESSAGE" "$TMP/$name.out"; then
    failed "$name" "the output does not say \"$MESSAGE\""
  fi
  if [ -n "${NO_SCAN:-}" ] && [ -e "$runner/gitleaks.log" ]; then
    failed "$name" "gitleaks ran"
  fi
  if ! problem=$(after_step 2>&1); then
    failed "$name" "$problem"
  fi
  if grep -q "^$name: " "$FAILURES"; then
    sed 's/^/    /' "$TMP/$name.out" >&2
  fi
  # The step deletes the exemption files from the checkout; put them back.
  git -C "$REPO" checkout -q -- .
}
PIPELINES=".claude/workflows/pipeline.workflow.js:3 .codeflow/.baseline/.claude/workflows/pipeline.workflow.js:3"

# No adopter configuration: the default rules. The prose alone is dropped;
# the credential on the same line and the prose elsewhere are not.
expect defaults 1 $PIPELINES notes.md:1

# The prose alone passes, in a history of its own.
SCAFFOLD=$REPO
REPO="$TMP/prose-only"
for path in .claude/workflows .codeflow/.baseline/.claude/workflows; do
  mkdir -p "$REPO/$path"
  printf '// seeded by CodeFlow 3.0.0\n%s\n' "$PROSE" >"$REPO/$path/pipeline.workflow.js"
done
init_repo
commit "prose only"
expect prose-only 0
# A longer credential that starts with the prose yields the same extracted
# value, and a path one character longer than an allowed one is another
# file: both are reported.
REPO="$TMP/near-misses"
LONGER="password = \"$(printf '%s%s' 'vulnerable/' 'malicious') $KEY\""
NL='
'
mkdir -p "$REPO/.claude/workflows"
printf '// x\n%s\n' "$LONGER" >"$REPO/.claude/workflows/pipeline.workflow.js"
printf '// x\n%s\n' "$PROSE" >"$REPO/.claude/workflows/pipeline.workflow.js$NL"
init_repo
commit "near misses"
expect near-misses 1 .claude/workflows/pipeline.workflow.js:2 \
  ".claude/workflows/pipeline.workflow.js$NL:2"

# A git that cannot list the paths the scan will read fails the step before
# gitleaks runs.
STEP_ENV="GIT_CONFIG_COUNT=3 GIT_CONFIG_KEY_2=diff.algorithm GIT_CONFIG_VALUE_2=invalid"
MESSAGE="cannot check the history the pull request brings; refusing to scan"
NO_SCAN=1
expect listing-failure 1
NO_SCAN=

# A scan in which gitleaks' own git fails reads no commit and exits 0 with
# an empty report; the step fails it. Only gitleaks' git log -p breaks.
mkdir "$TMP/broken-diff-git"
cat >"$TMP/broken-diff-git/git" <<SH
#!/bin/sh
case " \$* " in *" log -p "*) exec "$(command -v git)" -c diff.algorithm=invalid "\$@" ;; esac
exec "$(command -v git)" "\$@"
SH
chmod +x "$TMP/broken-diff-git/git"
STEP_ENV="PATH=$TMP/fake-bin:$TMP/broken-diff-git:$PATH"
MESSAGE="gitleaks did not complete a scan (exit 0, 0 commits read)"
expect git-failure 1

# Git that writes the whole history and then fails: commits are read and no
# error line is logged, but the scan is not trusted.
mkdir "$TMP/failing-git"
cat >"$TMP/failing-git/git" <<SH
#!/bin/sh
"$(command -v git)" "\$@"
status=\$?
case " \$* " in *" log -p "*) exit 1 ;; esac
exit \$status
SH
chmod +x "$TMP/failing-git/git"
# It runs on the clean prose-only history, which otherwise passes.
REPO="$TMP/prose-only"
STEP_ENV="PATH=$TMP/fake-bin:$TMP/failing-git:$PATH"
MESSAGE="gitleaks did not complete a scan"
expect git-exit 1
STEP_ENV=
MESSAGE=

# The same words in scanned text are not git's failure: a configuration
# title, and file content gitleaks decodes, both reach the debug log, and a
# complete scan with them passes.
ABORTED="hello command aborted harmless text"
REPO="$TMP/aborted-words"
mkdir -p "$REPO"
printf 'blob = "%s"\n' "$(printf '%s' "$ABORTED" | base64)" >"$REPO/encoded.txt"
init_repo
commit "aborted words"
in_debug_log() {
  grep -q "$ABORTED" "$TMP/runner-$1/gitleaks.log" || failed "$1" "the words never reached the debug log"
}
expect aborted-in-content 0
in_debug_log aborted-in-content
GITLEAKS_CONFIG_TOML="title = \"$ABORTED\"
[extend]
useDefault = true"
export GITLEAKS_CONFIG_TOML
expect aborted-in-config 0
in_debug_log aborted-in-config
unset GITLEAKS_CONFIG_TOML
REPO=$SCAFFOLD

# An adopter configuration is read as gitleaks reads it, whatever its
# shape: a path chain two levels deep, ordinary and rule-targeted
# allowlists, and disabled rules.
cat >"$REPO/organization.toml" <<'TOML'
[extend]
useDefault = true
TOML
cat >"$REPO/.gitleaks.toml" <<TOML
[extend]
path = "organization.toml"

[[allowlists]]
description = "adopter fixtures"
paths = ['''^docs/fixtures/''']

[[allowlists]]
description = "adopter's accepted sample key"
targetRules = ["generic-api-key"]
regexTarget = "secret"
regexes = ['''^${KEY}$''']
TOML
mkdir -p "$REPO/docs/fixtures"
printf '%s\n' "$PLANTED" >"$REPO/docs/fixtures/sample.txt"
printf 'token = "%s"\n' "$KEY" >"$REPO/accepted.txt"
printf 'password = "%s%s"\n' 'Zq8vR2mN6k' 'T4wX1pL9sB' >"$REPO/real.txt"
commit "adopter config"
# The rule-targeted allowlist accepts the sample key wherever it appears,
# the pipeline line included; the path allowlist covers the fixture.
expect adopter-config 1 notes.md:1 real.txt:1

# A configuration named by GITLEAKS_CONFIG wins over .gitleaks.toml, as it
# does for gitleaks itself: its own rule reports its canary.
cat >"$TMP/env.toml" <<'TOML'
[extend]
useDefault = true

[[rules]]
id = "adopter-canary"
regex = '''ADOPTER_CANARY_[0-9]{6}'''
TOML
printf 'ADOPTER_CANARY_%s\n' '424242' >"$REPO/canary.txt"
commit canary
# None of .gitleaks.toml's allowlists apply then.
STEP_ENV="GITLEAKS_CONFIG=$TMP/env.toml"
expect env-config 1 $PIPELINES notes.md:1 \
  real.txt:1 accepted.txt:1 docs/fixtures/sample.txt:1 canary.txt:1
STEP_ENV=

# A configuration gitleaks cannot load fails the step; it never passes.
printf '[extend\n' >"$REPO/.gitleaks.toml"
commit "broken config"
MESSAGE="unable to load gitleaks config"
expect broken-config 1
MESSAGE=

# Exemptions come from the trusted commit (TSK-210). Each history starts
# with a clean commit; a case named pr-* is a pull request on the commit
# before it, and the trusted-* case after it is a later pull request once
# the first has merged.
new_repo() {
  REPO="$TMP/$1"
  init_repo
  printf 'clean\n' >"$REPO/README.md"
  commit base
  BASE=$(tip)
}
on_base() { # the trusted commit, then the case
  TRUSTED=$1
  shift
  expect "$@"
  unset TRUSTED
}
later_fixture() { # a later pull request: one more leak at the allowed path
  BASE=$(tip)
  mkdir -p "$REPO/fixtures"
  printf '%s\n' "$PLANTED" >"$REPO/fixtures/later.txt"
  commit "later fixture"
}
ALLOW_FIXTURES="[extend]
useDefault = true

[[allowlists]]
description = \"fixtures\"
paths = ['''^fixtures/''']"

# A .gitleaksignore entry for the pull request's own leak.
new_repo ignore-file
printf '%s\n' "$PLANTED" >"$REPO/leak.txt"
commit leak
printf '%s:leak.txt:generic-api-key:1\n' "$(tip)" >"$REPO/.gitleaksignore"
commit "ignore the leak"
on_base "$BASE" pr-ignore-file 1 leak.txt:1
BASE=$(tip)
printf 'more\n' >>"$REPO/README.md"
commit later
on_base "$BASE" trusted-ignore-file 0

# A .gitleaks.toml allowlist for it.
new_repo allowlist
mkdir "$REPO/fixtures"
printf '%s\n' "$PLANTED" >"$REPO/fixtures/leak.txt"
printf '%s\n' "$ALLOW_FIXTURES" >"$REPO/.gitleaks.toml"
commit "leak with an allowlist"
on_base "$BASE" pr-allowlist 1 fixtures/leak.txt:1
later_fixture
on_base "$BASE" trusted-allowlist 0

# A .gitleaks.json beside .gitleaks.toml: gitleaks reads it first, as TOML.
new_repo json-beside
printf '[extend]\nuseDefault = true\n' >"$REPO/.gitleaks.toml"
commit config
BASE=$(tip)
mkdir "$REPO/fixtures"
printf '%s\n' "$PLANTED" >"$REPO/fixtures/leak.txt"
printf '%s\n' "$ALLOW_FIXTURES" >"$REPO/.gitleaks.json"
commit "leak with a json allowlist"
on_base "$BASE" pr-json-beside 1 fixtures/leak.txt:1
later_fixture
on_base "$BASE" trusted-json-beside 0

# An inline gitleaks:allow comment.
new_repo inline
printf '%s # gitleaks:allow\n' "$PLANTED" >"$REPO/inline.txt"
commit "inline allow"
on_base "$BASE" pr-inline 1 inline.txt:1
BASE=$(tip)
printf 'more\n' >>"$REPO/README.md"
commit later
on_base "$BASE" trusted-inline 0

# An [extend] path is read from the trusted commit, never the checkout: a
# pull request that widens the extended file fails, and once merged it
# holds. GITLEAKS_CONFIG names a file in the repository the same way.
new_repo extended
mkdir "$REPO/ci"
printf '[extend]\nuseDefault = true\n' >"$REPO/ci/base.toml"
printf '[extend]\npath = "ci/base.toml"\n' >"$REPO/.gitleaks.toml"
commit config
BASE=$(tip)
mkdir "$REPO/fixtures"
printf '%s\n' "$PLANTED" >"$REPO/fixtures/leak.txt"
printf '%s\n' "$ALLOW_FIXTURES" >"$REPO/ci/base.toml"
commit "leak with a wider extended file"
on_base "$BASE" pr-extended 1 fixtures/leak.txt:1
STEP_ENV="GITLEAKS_CONFIG=ci/base.toml"
on_base "$BASE" pr-named-config 1 fixtures/leak.txt:1
STEP_ENV=
later_fixture
on_base "$BASE" trusted-extended 0
# A file the trusted commit does not hold fails the step, and an absolute
# path into the checkout is refused.
printf '[extend]\npath = "ci/absent.toml"\n' >"$REPO/.gitleaks.toml"
commit "extend a missing file"
MESSAGE="which trusted commit"
expect extend-missing 1
printf '[extend]\npath = "%s/ci/base.toml"\n' "$(cd "$REPO" && pwd -P)" >"$REPO/.gitleaks.toml"
commit "extend by absolute path"
MESSAGE="a file in the checkout"
expect extend-absolute 1
MESSAGE=

# Without a readable trusted commit the step fails before gitleaks runs.
new_repo no-base
NO_SCAN=1
MESSAGE="trusted commit 0123456789abcdef0123456789abcdef01234567 is not in the checkout; refusing to scan"
on_base 0123456789abcdef0123456789abcdef01234567 absent-base 1
MESSAGE="no trusted commit to read exemptions from; refusing to scan"
on_base "" empty-base 1
on_base origin/main ref-name-base 1
MESSAGE=
NO_SCAN=

# Links the pull request commits at the names the step reads or writes are
# neither followed nor written through, and nothing lands in the checkout.
new_repo links
mkdir "$TMP/link-targets"
printf 'link target\n' >"$TMP/link-targets/ignore"
printf '%s\n' "$ALLOW_FIXTURES" >"$TMP/link-targets/config.toml"
printf 'link target\n' >"$TMP/link-targets/archive"
printf 'link target\n' >"$TMP/link-targets/binary"
(cd "$TMP/link-targets" && cksum ignore config.toml archive binary) >"$TMP/link-targets.before"
ln -s "$TMP/link-targets/ignore" "$REPO/.gitleaksignore"
ln -s "$TMP/link-targets/config.toml" "$REPO/.gitleaks.toml"
ln -s "$TMP/link-targets/archive" "$REPO/gitleaks_8.30.1_linux_x64.tar.gz"
ln -s "$TMP/link-targets/binary" "$REPO/gitleaks"
mkdir "$REPO/fixtures"
printf '%s\n' "$PLANTED" >"$REPO/fixtures/leak.txt"
git -C "$REPO" add -f gitleaks gitleaks_8.30.1_linux_x64.tar.gz
commit "plant links"
after_step() {
  (cd "$TMP/link-targets" && cksum ignore config.toml archive binary) |
    cmp -s - "$TMP/link-targets.before" || { echo "a link target was written"; return 1; }
  for link in .gitleaksignore .gitleaks.toml; do
    if [ -e "$REPO/$link" ] || [ -L "$REPO/$link" ]; then
      echo "$link is still in the checkout"
      return 1
    fi
  done
  added=$(git -C "$REPO" status --porcelain --ignored --untracked-files=all | grep -v '^ D ' || true)
  [ -z "$added" ] || { echo "the step wrote into the checkout: $added"; return 1; }
}
on_base "$BASE" pr-links 1 fixtures/leak.txt:1
after_step() { :; }

# A Python module the pull request commits at the checkout's root is never
# imported by the step's helpers. Each one marks that it ran and, given the
# report, empties it and exits 0, which would hide the leak.
mkdir "$TMP/shadow-marks"
shadow() {
  cat >"$REPO/$1.py" <<PY
import sys
with open("$TMP/shadow-marks/$1", "a") as mark:
    mark.write("imported\n")
if len(sys.argv) > 1 and sys.argv[1].endswith(".json"):
    with open(sys.argv[1], "w") as report:
        report.write("[]")
    print("[]")
    raise SystemExit(0)
PY
}
after_step() {
  marks=$(ls "$TMP/shadow-marks")
  [ -z "$marks" ] || { echo "the step imported the checkout's" $marks; return 1; }
}
new_repo shadow-json
shadow json
printf '%s\n' "$PLANTED" >"$REPO/leak.txt"
commit "leak with a json module"
on_base "$BASE" pr-shadow-json 1 leak.txt:1
rm -f "$TMP/shadow-marks"/*
new_repo shadow-modules
for module in os re subprocess tomllib; do
  shadow "$module"
done
printf '%s\n' "$PLANTED" >"$REPO/leak.txt"
commit "leak with stdlib modules"
on_base "$BASE" pr-shadow-modules 1 leak.txt:1
after_step() { :; }

# A .gitattributes the pull request commits does not hide its changes from
# git: attributes come from the trusted commit.
new_repo attributes
printf 'leak.txt -diff\n' >"$REPO/.gitattributes"
printf '%s\n' "$PLANTED" >"$REPO/leak.txt"
commit "leak marked -diff"
on_base "$BASE" pr-attributes 1 leak.txt:1

# History the pull request writes cannot hide a secret from git log: one
# added only in a merge resolution, and one in a file that replaces a link.
merge() { # the branch to merge, then git merge options
  branch=$1
  shift
  git -C "$REPO" -c user.name=canary -c user.email=canary@example.invalid \
    -c commit.gpgsign=false merge -q --no-ff "$@" "$branch" >/dev/null 2>&1
}
new_repo evil-merge
git -C "$REPO" checkout -q -b side
printf 'side\n' >"$REPO/side.txt"
commit side
git -C "$REPO" checkout -q -
printf 'main\n' >"$REPO/main.txt"
commit main
merge side --no-commit
printf '%s\n' "$PLANTED" >"$REPO/evil.txt"
commit "merge side"
on_base "$BASE" pr-evil-merge 1 evil.txt:1
new_repo type-change
ln -s README.md "$REPO/link.txt"
commit "a link"
BASE=$(tip)
rm "$REPO/link.txt"
printf '%s\n' "$PLANTED" >"$REPO/link.txt"
commit "the link becomes a file"
on_base "$BASE" pr-type-change 1 link.txt:1

# Reading merges reports nothing twice. Main merged a branch whose commits
# hold a secret its .gitleaksignore exempts and one marked gitleaks:allow;
# a pull request that merges main in reports only its own leak, once.
new_repo merges
FIRST=$(tip)
git -C "$REPO" checkout -q -b side
printf '%s\n' "$PLANTED" >"$REPO/ignored.txt"
commit "an ignored secret"
IGNORED=$(tip)
printf '%s # gitleaks:allow\n' "$PLANTED" >"$REPO/inline.txt"
commit "an inline allow"
git -C "$REPO" checkout -q -
printf 'main\n' >"$REPO/main.txt"
commit main
merge side -m "merge side"
printf '%s:ignored.txt:generic-api-key:1\n' "$IGNORED" >"$REPO/.gitleaksignore"
commit "ignore the merged secret"
BASE=$(tip)
git -C "$REPO" checkout -q -b pr "$FIRST"
printf '%s\n' "$PLANTED" >"$REPO/own-leak.txt"
commit "the pull request's own leak"
merge "$BASE" -m "merge main"
on_base "$BASE" pr-merges-main 1 own-leak.txt:1

# A git that cannot read .gitattributes from the trusted commit (before
# 2.41) is refused before gitleaks runs.
mkdir "$TMP/git-2.40"
cat >"$TMP/git-2.40/git" <<SH
#!/bin/sh
case "\$1" in version | --version) echo "git version 2.40.4"; exit 0 ;; esac
exec "$(command -v git)" "\$@"
SH
chmod +x "$TMP/git-2.40/git"
new_repo old-git
STEP_ENV="PATH=$TMP/fake-bin:$TMP/git-2.40:$PATH"
MESSAGE="git version 2.40.4 cannot read .gitattributes from the trusted commit; upgrade git to 2.41 or later; refusing to scan"
NO_SCAN=1
expect old-git 1
STEP_ENV=
MESSAGE=
NO_SCAN=

# git cannot show what an octopus merge adds. One the trusted commit does
# not hold is refused before gitleaks runs, even when a later commit
# deletes what it added. One the trusted commit holds is not refused; its
# own pull request met the check.
as_canary() { git -C "$REPO" -c user.name=canary -c user.email=canary@example.invalid \
  -c commit.gpgsign=false "$@"; }
new_repo octopus
MAIN=$(git -C "$REPO" symbolic-ref --short HEAD)
for branch in one two; do
  git -C "$REPO" checkout -q -b "$branch" "$BASE"
  printf '%s\n' "$branch" >"$REPO/$branch.txt"
  commit "$branch"
done
git -C "$REPO" checkout -q "$MAIN"
as_canary merge -q --no-ff -m octopus one two >/dev/null 2>&1
printf '%s\n' "$PLANTED" >"$REPO/octopus.txt"
git -C "$REPO" add -A
as_canary commit -q --amend -m octopus
git -C "$REPO" rm -q octopus.txt
commit "remove what the octopus added"
NO_SCAN=1
MESSAGE="(in HEAD, refs/heads/$MAIN) is not in the trusted commit, and git cannot show what it adds"
on_base "$BASE" pr-octopus 1
NO_SCAN=
MESSAGE=
on_base "$(tip)" trusted-octopus 0
# A local stash also has three parents; it is not refused.
new_repo stash
printf 'changed\n' >>"$REPO/README.md"
printf 'untracked\n' >"$REPO/untracked.txt"
as_canary stash push -q -u
on_base "$BASE" local-stash 0
# Branches and tags that HEAD does not reach are not the pull request's: an
# octopus merge, an odd name and a leak on them neither refuse nor fail it.
new_repo unrelated-ref
MAIN=$(git -C "$REPO" symbolic-ref --short HEAD)
for branch in one two; do
  git -C "$REPO" checkout -q -b "$branch" "$BASE"
  printf '%s\n' "$branch" >"$REPO/$branch.txt"
  commit "$branch"
done
git -C "$REPO" checkout -q -b stale "$BASE"
as_canary merge -q --no-ff -m octopus one two >/dev/null 2>&1
printf 'odd\n' >"$REPO/stale\\"
printf '%s\n' "$PLANTED" >"$REPO/stale-leak.txt"
commit "an odd name and a leak"
git -C "$REPO" tag stale-tag
git -C "$REPO" checkout -q "$MAIN"
printf 'more\n' >>"$REPO/README.md"
commit "the pull request"
on_base "$BASE" unrelated-ref 0

# A NUL byte, which makes git call a file binary, does not hide its text,
# in a commit or in what a merge adds.
new_repo binary
printf '\000\n%s\n' "$PLANTED" >"$REPO/nul.txt"
commit "a NUL byte first"
on_base "$BASE" pr-binary 1 nul.txt:2
new_repo binary-merge
MAIN=$(git -C "$REPO" symbolic-ref --short HEAD)
git -C "$REPO" checkout -q -b side
printf 'side\n' >"$REPO/side.txt"
commit side
git -C "$REPO" checkout -q "$MAIN"
printf 'main\n' >"$REPO/main.txt"
commit main
merge side --no-commit
printf '\000\n%s\n' "$PLANTED" >"$REPO/nul.txt"
commit "merge side"
on_base "$BASE" pr-binary-merge 1 nul.txt:2

# A conflicted merge is reported under its file's own path, so a trusted
# exemption for a real b/ path does not cover it, and still covers the
# real b/ file.
new_repo conflict-path
MAIN=$(git -C "$REPO" symbolic-ref --short HEAD)
mkdir "$REPO/b"
printf 'one\ntwo\n%s\n' "$PLANTED" >"$REPO/b/f.txt"
printf 'one\n' >"$REPO/f.txt"
printf 'b/f.txt:generic-api-key:3\n' >"$REPO/.gitleaksignore"
commit "a real b/f.txt and its exemption"
BASE=$(tip)
git -C "$REPO" checkout -q -b side
printf 'side\n' >"$REPO/f.txt"
commit side
git -C "$REPO" checkout -q "$MAIN"
printf 'main\n' >"$REPO/f.txt"
commit main
as_canary merge -q side >/dev/null 2>&1 || true
printf 'main\nside\n%s\n' "$PLANTED" >"$REPO/f.txt"
commit "resolve with a secret"
on_base "$BASE" pr-conflict-path 1 f.txt:3

# git configuration the runner sets does not change the patch gitleaks reads.
# With diff.noprefix, gitleaks would drop dir/ and the trusted exemption for
# a top-level leak.txt would cover dir/leak.txt.
new_repo inherited-config
printf 'leak.txt:generic-api-key:1\n' >"$REPO/.gitleaksignore"
commit "exempt a top-level leak.txt"
BASE=$(tip)
mkdir "$REPO/dir"
printf '%s\n' "$PLANTED" >"$REPO/dir/leak.txt"
commit "a leak one directory down"
inherit() { # STEP_ENV that sets each key=value as the runner's git configuration
  count=2
  STEP_ENV=
  for pair in "$@"; do
    STEP_ENV="$STEP_ENV GIT_CONFIG_KEY_$count=${pair%%=*} GIT_CONFIG_VALUE_$count=${pair#*=}"
    count=$((count + 1))
  done
  STEP_ENV="GIT_CONFIG_COUNT=$count$STEP_ENV"
}
inherit diff.noprefix=true
on_base "$BASE" inherited-noprefix 1 dir/leak.txt:1
# The other settings that change the patch's prefixes, names, colour or
# commit headers, together.
inherit diff.noprefix=true diff.mnemonicPrefix=true diff.srcPrefix=x/ diff.dstPrefix=y/ \
  core.quotePath=false color.ui=always color.diff=always format.pretty=oneline log.date=relative \
  log.decorate=full log.abbrevCommit=true log.showSignature=true log.showRoot=false
on_base "$BASE" inherited-config 1 dir/leak.txt:1
STEP_ENV=

# gitleaks' patch parser misreads some names that git quotes. A path with a
# backslash, a double quote or a control character in a commit the trusted
# commit does not hold is refused before gitleaks runs.
ALLOW_B="[extend]
useDefault = true

[[allowlists]]
description = \"a real b/ directory\"
paths = ['''^b/''']"
REWRITE="gitleaks cannot read a backslash, double quote or control character in a path reliably; rewrite the pull request's commits so that none uses the name"
REBASE="rebase the pull request onto the trusted commit instead of merging it in; refusing to scan"
new_repo backslash-name
printf '%s\n' "$PLANTED" >"$REPO/leak\\"
commit "a name that ends in a backslash"
rm "$REPO/leak\\"
commit "delete it"
NO_SCAN=1
MESSAGE=$REWRITE
on_base "$BASE" pr-backslash-name 1
# A newline in the name of a file both sides change: the conflicted merge's
# header would span lines, and the trusted ^b/ allowlist would then cover
# the secret its resolution adds.
new_repo newline-conflict
MAIN=$(git -C "$REPO" symbolic-ref --short HEAD)
printf '%s\n' "$ALLOW_B" >"$REPO/.gitleaks.toml"
printf 'one\n' >"$REPO/f${NL}x.txt"
commit "an odd name and a b/ allowlist"
BASE=$(tip)
git -C "$REPO" checkout -q -b side
printf 'side\n' >"$REPO/f${NL}x.txt"
commit side
git -C "$REPO" checkout -q "$MAIN"
printf 'main\n' >"$REPO/f${NL}x.txt"
commit main
as_canary merge -q side >/dev/null 2>&1 || true
printf 'main\nside\n%s\n' "$PLANTED" >"$REPO/f${NL}x.txt"
commit "resolve with a secret"
on_base "$BASE" pr-newline-conflict 1
# The same for a rename conflict whose new name holds a newline.
new_repo rename-newline
MAIN=$(git -C "$REPO" symbolic-ref --short HEAD)
printf '%s\n' "$ALLOW_B" >"$REPO/.gitleaks.toml"
printf 'one\ntwo\n' >"$REPO/orig.txt"
commit "a file and a b/ allowlist"
BASE=$(tip)
git -C "$REPO" checkout -q -b side
git -C "$REPO" mv orig.txt "new${NL}name.txt"
commit "rename it oddly"
git -C "$REPO" checkout -q "$MAIN"
git -C "$REPO" mv orig.txt main.txt
commit "rename it plainly"
as_canary merge -q side >/dev/null 2>&1 || true
rm -f "$REPO/new${NL}name.txt" "$REPO/orig.txt"
printf 'one\ntwo\n%s\n' "$PLANTED" >"$REPO/main.txt"
commit "resolve with a secret"
on_base "$BASE" pr-rename-newline 1
# The same when the odd name is the trusted side's: main renames a file to a
# name with two newlines, the pull request renames it plainly, and its merge
# of main deletes both and puts the secret at the old name. The conflict's
# header names the trusted file, and the trusted ^b/ allowlist would cover
# the secret.
new_repo trusted-rename-newline
MAIN=$(git -C "$REPO" symbolic-ref --short HEAD)
printf '%s\n' "$ALLOW_B" >"$REPO/.gitleaks.toml"
printf 'one\ntwo\n' >"$REPO/orig.txt"
commit "a file and a b/ allowlist"
FIRST=$(tip)
git -C "$REPO" mv orig.txt "trusted${NL}123456${NL}x.txt"
commit "rename it oddly on main"
BASE=$(tip)
git -C "$REPO" checkout -q -b pr "$FIRST"
git -C "$REPO" mv orig.txt side.txt
commit "rename it plainly"
as_canary merge -q "$MAIN" >/dev/null 2>&1 || true
git -C "$REPO" rm -q --cached -r . >/dev/null
rm -f "$REPO/side.txt" "$REPO/trusted${NL}123456${NL}x.txt"
printf 'one\ntwo\n%s\n' "$PLANTED" >"$REPO/orig.txt"
commit "resolve with a secret at the old name"
MESSAGE=$REBASE
on_base "$BASE" pr-trusted-rename-newline 1
NO_SCAN=
MESSAGE=
# An ordinary rename conflict is scanned, and its resolution's secret is
# reported under its own path, out of reach of the trusted ^b/ allowlist.
new_repo rename-conflict
MAIN=$(git -C "$REPO" symbolic-ref --short HEAD)
printf '%s\n' "$ALLOW_B" >"$REPO/.gitleaks.toml"
printf 'one\ntwo\n' >"$REPO/orig.txt"
commit "a file and a b/ allowlist"
BASE=$(tip)
git -C "$REPO" checkout -q -b side
git -C "$REPO" mv orig.txt side.txt
commit "rename it one way"
git -C "$REPO" checkout -q "$MAIN"
git -C "$REPO" mv orig.txt main.txt
commit "rename it another way"
as_canary merge -q side >/dev/null 2>&1 || true
rm -f "$REPO/side.txt" "$REPO/orig.txt"
printf 'one\ntwo\n%s\n' "$PLANTED" >"$REPO/main.txt"
commit "resolve with a secret"
on_base "$BASE" pr-rename-conflict 1 main.txt:3
# Odd names the trusted commit already holds do not refuse the scan, and a
# pull request may delete one.
new_repo trusted-odd-name
printf 'trusted\n' >"$REPO/old\\"
printf 'trusted\n' >"$REPO/say\"hi\".txt"
commit "odd names"
BASE=$(tip)
rm "$REPO/old\\"
printf '%s\n' "$PLANTED" >"$REPO/leak.txt"
commit "delete one and leak"
on_base "$BASE" trusted-odd-name 1 leak.txt:1
# Nor does a merge of main whose sides leave such a name alone.
new_repo odd-name-merge
MAIN=$(git -C "$REPO" symbolic-ref --short HEAD)
printf 'trusted\n' >"$REPO/say\"hi\".txt"
commit "an odd name"
FIRST=$(tip)
printf 'more\n' >>"$REPO/README.md"
commit "later on main"
BASE=$(tip)
git -C "$REPO" checkout -q -b pr "$FIRST"
printf '%s\n' "$PLANTED" >"$REPO/leak.txt"
commit leak
merge "$MAIN" -m "merge main"
on_base "$BASE" odd-name-merge 1 leak.txt:1
# Names with spaces or non-ASCII letters pass, and a leak in one is
# reported under its name.
CAFE="caf$(printf '\303\251').txt"
new_repo spaced-names
mkdir "$REPO/dir with space"
printf 'plain\n' >"$REPO/dir with space/a b.txt"
printf 'plain\n' >"$REPO/$CAFE"
commit "spaced and non-ASCII names"
on_base "$BASE" spaced-names 0
printf '%s\n' "$PLANTED" >>"$REPO/dir with space/a b.txt"
printf '%s\n' "$PLANTED" >>"$REPO/$CAFE"
commit "leaks in them"
on_base "$BASE" spaced-name-leaks 1 "dir with space/a b.txt:2" "$CAFE:2"

# The README's recipe for a wrapper with no configuration of its own keeps
# gitleaks' default rules: plain gitleaks with it allows the prose and still
# reports the credential on the same line.
REPO="$TMP/readme-recipe"
for path in .claude/workflows .codeflow/.baseline/.claude/workflows; do
  mkdir -p "$REPO/$path"
  printf '// seeded by CodeFlow 3.0.0\n%s\n%s %s\n%s\n' "$PROSE" "$PROSE" "$PLANTED" "$LONGER" \
    >"$REPO/$path/pipeline.workflow.js"
done
python3 - "$ROOT/assets/base/ci/README.md" "$REPO/.gitleaks.toml" <<'PY'
import re
import sys

text = open(sys.argv[1], encoding="utf-8").read()
recipe = re.search(r"\n  ```toml\n(  \[extend\]\n.*?)\n  ```\n", text, re.S)
if recipe is None:
    raise SystemExit("the CI README shows no complete gitleaks configuration for a wrapper")
lines = recipe.group(1).split("\n")
open(sys.argv[2], "w", encoding="utf-8").write("\n".join(line[2:] for line in lines) + "\n")
PY
init_repo
commit recipe
"$GITLEAKS" detect --source "$REPO" --no-banner --redact --report-format json \
  --report-path "$TMP/recipe.json" --exit-code 0 >/dev/null 2>&1
python3 - "$TMP/recipe.json" <<'PY'
import json
import sys

found = sorted(f"{f['File']}:{f['StartLine']}" for f in json.load(open(sys.argv[1], encoding="utf-8")))
expected = sorted(f"{p}pipeline.workflow.js:{n}" for p in (".claude/workflows/", ".codeflow/.baseline/.claude/workflows/") for n in (3, 4))
if found != expected:
    raise SystemExit(f"the CI README's gitleaks recipe: expected {expected}, got {found}")
PY

if [ -s "$FAILURES" ]; then
  printf '\nscaffold secret scan failed:\n' >&2
  sed 's/^/  /' "$FAILURES" >&2
  exit 1
fi
printf '%s\n' "scaffold secret scan passed"
