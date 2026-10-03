#!/bin/sh
# What adopters get from the scaffold passes their secret scan (TSK-206,
# sathyassn/codeflow#13). With the pinned gitleaks:
#   1. the shipped scaffold sources under assets/ hold nothing gitleaks'
#      default rules report; given a codeflow binary as the second
#      argument, fresh `codeflow init` output at each tier is scanned too;
#   2. the adopter CI template's own gitleaks step, run with bash -e as
#      GitHub runs it, drops only the 3.0.0 pipeline prose in its two
#      managed paths and still fails on a credential on that same line, the
#      same prose anywhere else, and every finding an adopter's own
#      configuration makes, wherever gitleaks reads that configuration from;
#      a scan that does not complete fails the step.
# Usage: check-scaffold-gitleaks.sh [gitleaks] [codeflow]
set -eu
# The finding lists below are word-split on purpose.
# shellcheck disable=SC2086

GITLEAKS=${1:-gitleaks}
CODEFLOW=${2:-}
absolute() { case $1 in /*) printf '%s' "$1" ;; */*) printf '%s/%s' "$(pwd)" "$1" ;; *) printf '%s' "$1" ;; esac; }
GITLEAKS=$(absolute "$GITLEAKS")
[ -z "$CODEFLOW" ] || CODEFLOW=$(absolute "$CODEFLOW")
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

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

# The template step after its download lines, with the downloaded binary
# replaced by the one under test.
python3 - "$ROOT/assets/base/ci/codeflow-ci.yml" "$TMP/step.sh" <<'PY'
import sys

lines = open(sys.argv[1], encoding="utf-8").read().split("\n")
start = lines.index("      - name: gitleaks")
assert lines[start + 1] == "        run: |", lines[start + 1]
body = []
for line in lines[start + 2:]:
    if line.strip() and not line.startswith(" " * 10):
        break
    body.append(line[10:])
script = "\n".join(body)
marker = 'tar -xzf "$archive" gitleaks\n'
assert marker in script, script
script = script.split(marker, 1)[1]
if '"$report"' not in script:
    raise SystemExit("the template's gitleaks step reads no report, so the "
                     "3.0.0 pipeline line fails every adopter's secret scan")
assert script.count("./gitleaks detect") == 1, script
open(sys.argv[2], "w", encoding="utf-8").write(script.replace("./gitleaks detect", '"$GITLEAKS" detect'))
PY

PROSE=$(printf 'Cover secret/PII exposure, authz gaps, %s%s, general vuln classes' \
  'vulnerable/' 'malicious deps')
KEY=$(printf '%s%s' '9fK2pQ7xLm4R' 't8Wz1Vb6Nc3H')
PLANTED="api_key = \"$KEY\""
REPO="$TMP/adopter"
for path in .claude/workflows .codeflow/.baseline/.claude/workflows; do
  mkdir -p "$REPO/$path"
  # Line 2 is the 3.0.0 prose; line 3 is the same prose with a credential
  # on the same line.
  printf '// seeded by CodeFlow 3.0.0\n%s\n%s %s\n' "$PROSE" "$PROSE" "$PLANTED" \
    >"$REPO/$path/pipeline.workflow.js"
done
printf '%s\n' "$PROSE" >"$REPO/notes.md"
git -C "$REPO" init -q
commit() {
  git -C "$REPO" add -A
  git -C "$REPO" -c user.name=canary -c user.email=canary@example.invalid \
    -c commit.gpgsign=false commit -q -m "$1"
}
commit scaffold

# One case: a name, the exit status the step must end with, then the
# "file:line" findings it must name, exactly. Extra environment is passed
# through STEP_ENV.
expect() {
  name=$1
  status=$2
  shift 2
  set +e
  (cd "$REPO" && env RUNNER_TEMP="$TMP" GITLEAKS="$GITLEAKS" ${STEP_ENV:-} \
    bash -e "$TMP/step.sh") >"$TMP/$name.out" 2>&1
  rc=$?
  set -e
  python3 - "$TMP/$name.out" "$name" "$status" "$rc" "$KEY" "$@" <<'PY'
import re
import sys
from urllib.parse import unquote

out = open(sys.argv[1], encoding="utf-8").read()
name, status, rc, key = sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), sys.argv[5]
expected = sorted(sys.argv[6:])
found = sorted(f"{unquote(f)}:{l}" for f, l in re.findall(r"^::error file=([^,\n]+),line=(\d+)::", out, re.M))
problems = []
if (rc == 0) != (status == 0):
    problems.append(f"exit {rc}, expected {'0' if status == 0 else 'non-zero'}")
if found != expected:
    problems.append(f"findings {found}, expected {expected}")
if key in out:
    problems.append("the log shows a credential's value")
if problems:
    raise SystemExit(f"template secret scan, {name}: " + "; ".join(problems) + "\n" + out)
PY
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
git -C "$REPO" init -q
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
git -C "$REPO" init -q
commit "near misses"
expect near-misses 1 .claude/workflows/pipeline.workflow.js:2 \
  ".claude/workflows/pipeline.workflow.js$NL:2"

# A scan in which git fails reads no commit and exits 0 with an empty
# report; the step fails it.
STEP_ENV="GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=diff.algorithm GIT_CONFIG_VALUE_0=invalid"
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
STEP_ENV="PATH=$TMP/failing-git:$PATH"
expect git-exit 1
STEP_ENV=

# The same words in scanned text are not git's failure: a configuration
# title, and file content gitleaks decodes, both reach the debug log, and a
# complete scan with them passes.
ABORTED="hello command aborted harmless text"
REPO="$TMP/aborted-words"
mkdir -p "$REPO"
printf 'blob = "%s"\n' "$(printf '%s' "$ABORTED" | base64)" >"$REPO/encoded.txt"
git -C "$REPO" init -q
commit "aborted words"
in_debug_log() {
  grep -q "$ABORTED" "$TMP/gitleaks.log" || {
    echo "template secret scan, $1: the words never reached the debug log" >&2
    exit 1
  }
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
expect broken-config 1

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
git -C "$REPO" init -q
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

printf '%s\n' "scaffold secret scan passed"
