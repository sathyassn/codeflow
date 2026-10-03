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
#      no Python module or .gitattributes it commits steers the scan;
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

# The prose alone passes, in a history of its own (gitleaks reads every
# branch).
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

# A scan in which git fails reads no commit and exits 0 with an empty
# report; the step fails it.
STEP_ENV="GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=diff.algorithm GIT_CONFIG_VALUE_0=invalid"
MESSAGE="gitleaks did not complete a scan (exit 0, 0 commits read)"
expect git-failure 1

# Git that writes the whole history and then fails: commits are read and no
# error line is logged, but the scan is not trusted.
mkdir "$TMP/failing-git"
cat >"$TMP/failing-git/git" <<SH
#!/bin/sh
"$(command -v git)" "\$@"
status=\$?
case " \$* " in *" log "*) exit 1 ;; esac
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
