#!/bin/sh
# What adopters get from the scaffold passes their secret scan (TSK-206,
# sathyassn/codeflow#13). Two checks, with the pinned gitleaks:
#   1. the shipped scaffold sources under assets/ hold nothing gitleaks'
#      default rules report, with no allowance of ours in play;
#   2. the adopter CI template's own gitleaks step, run on a history that
#      holds the 3.0.0 pipeline line, allows that exact prose in the two
#      managed paths and still reports a planted secret beside it, the same
#      prose anywhere else, and an adopter's own findings.
set -eu

GITLEAKS=${1:-gitleaks}
case $GITLEAKS in /*) ;; */*) GITLEAKS="$(pwd)/$GITLEAKS" ;; esac
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

printf '[extend]\nuseDefault = true\n' >"$TMP/default.toml"
"$GITLEAKS" detect --no-git --source "$ROOT/assets" --config "$TMP/default.toml" \
  --no-banner --redact --report-format json --report-path "$TMP/assets.json" \
  --exit-code 0 >/dev/null
python3 - "$TMP/assets.json" <<'PY'
import json
import sys

findings = json.load(open(sys.argv[1], encoding="utf-8"))
if findings:
    found = sorted((f.get("RuleID"), f.get("File"), f.get("StartLine")) for f in findings)
    raise SystemExit(f"shipped scaffold sources fail gitleaks' default rules: {found}")
PY

# The template step after its download lines, scanning with a report.
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
marker = 'config="$RUNNER_TEMP'
if marker not in script:
    raise SystemExit("the template's gitleaks step builds no scan config, so the "
                     "3.0.0 pipeline line fails every adopter's secret scan")
script = script[script.index(marker):]
old = "./gitleaks detect --source . --config \"$config\" --redact --no-banner --exit-code 1"
assert old in script, script
script = script.replace(old, "\"$GITLEAKS\" detect --source . --config \"$config\" --redact "
                        "--no-banner --exit-code 0 --report-format json --report-path \"$REPORT\"")
open(sys.argv[2], "w", encoding="utf-8").write("set -eu\n" + script + "\n")
PY

LINE_209=$(printf 'Cover secret/PII exposure, authz gaps, %s%s, general vuln classes' \
  'vulnerable/' 'malicious deps')
PLANTED=$(printf 'api_key = "%s%s"' '9fK2pQ7xLm4R' 't8Wz1Vb6Nc3H')
REPO="$TMP/adopter"
for path in .claude/workflows .codeflow/.baseline/.claude/workflows; do
  mkdir -p "$REPO/$path"
  printf '// seeded by CodeFlow 3.0.0\n%s\n%s\n' "$LINE_209" "$PLANTED" \
    >"$REPO/$path/pipeline.workflow.js"
done
printf '%s\n' "$LINE_209" >"$REPO/notes.md"
git -C "$REPO" init -q
git -C "$REPO" add -A
git -C "$REPO" -c user.name=canary -c user.email=canary@example.invalid \
  -c commit.gpgsign=false commit -q -m "scaffold"

scan() {
  (cd "$REPO" && RUNNER_TEMP="$TMP" GITLEAKS="$GITLEAKS" REPORT="$TMP/$1.json" sh "$TMP/step.sh")
}
expect() { # report name, then the expected "file:line" findings
  report=$1
  shift
  python3 - "$TMP/$report.json" "$@" <<'PY'
import json
import sys

findings = json.load(open(sys.argv[1], encoding="utf-8"))
found = sorted(f"{f.get('File')}:{f.get('StartLine')}" for f in findings)
expected = sorted(sys.argv[2:])
if found != expected:
    raise SystemExit(f"template secret scan: expected {expected}, got {found}")
PY
}

# No adopter config: the default rules plus the allowance.
scan defaults
expect defaults \
  .claude/workflows/pipeline.workflow.js:3 \
  .codeflow/.baseline/.claude/workflows/pipeline.workflow.js:3 \
  notes.md:1

# An adopter config is extended, never replaced: its own allowance holds
# and its own findings are still reported.
cat >"$REPO/.gitleaks.toml" <<'TOML'
[extend]
useDefault = true

[[allowlists]]
description = "adopter fixtures"
paths = ['''^docs/fixtures/''']
TOML
mkdir -p "$REPO/docs/fixtures"
printf '%s\n' "$PLANTED" >"$REPO/docs/fixtures/sample.txt"
printf '%s\n' "$PLANTED" >"$REPO/real.txt"
git -C "$REPO" add -A
git -C "$REPO" -c user.name=canary -c user.email=canary@example.invalid \
  -c commit.gpgsign=false commit -q -m "adopter config"
scan adopter
expect adopter \
  .claude/workflows/pipeline.workflow.js:3 \
  .codeflow/.baseline/.claude/workflows/pipeline.workflow.js:3 \
  notes.md:1 \
  real.txt:1

printf '%s\n' "scaffold secret scan passed"
