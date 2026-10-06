#!/bin/sh
set -eu

GITLEAKS=${1:-gitleaks}
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

mkdir -p "$TMP/crates/codeflow-core/src/hooks"
cp "$ROOT/.gitleaks.toml" "$TMP/.gitleaks.toml"
{
  printf 'const KNOWN_FIXTURE: &str = "%s%s";\n' 'AKIA' 'IOSFODNN7EXAMPLF'
  printf 'const NOVEL_CREDENTIAL: &str = "%s%s";\n' 'AKIA' 'ZZZZZZZZZZZZZZZZ'
  printf 'api_key = "%s%s"\n' 'z9Qp4Lm7Vx2N' 'c8Rk5Tw3Hs6Y'
  printf '    scan_line("%s%s%s"),\n' '-----BEGIN ' 'RSA PRIVATE KEY' '-----'
  printf '    Some("private key block")\n);\nassert_eq!(\n'
  printf '    scan_line("%s%s%s"),\n' '-----BEGIN ' 'OPENSSH PRIVATE KEY' '-----'
  printf '%s%s%s\n%s\n%s%s%s\n' \
    '-----BEGIN ' 'RSA PRIVATE KEY' '-----' \
    'ZmFrZS1maXh0dXJlLW5vdC1hLXJlYWwta2V5' \
    '-----END ' 'RSA PRIVATE KEY' '-----'
} >"$TMP/crates/codeflow-core/src/hooks/scan.rs"
"$GITLEAKS" detect --source "$TMP" --no-git --no-banner --redact \
  --config "$TMP/.gitleaks.toml" --report-format json \
  --report-path "$TMP/report.json" --exit-code 0 >/dev/null

python3 - "$TMP/report.json" <<'PY'
import json
import sys

findings = json.load(open(sys.argv[1], encoding="utf-8"))
rules = sorted(finding.get("RuleID") for finding in findings)
expected = sorted(["aws-access-token", "generic-api-key", "private-key"])
if rules != expected:
    raise SystemExit(f"expected novel credential findings {expected}, got {rules}")
PY

# The TSK-115 evidence allowance is path-scoped, so it is exercised the way
# the real scan runs: on git history, with repository-relative paths. Each
# evidence file holds its recorded project key (line 1, allowed) and a novel
# 64-hex key (line 2, reported); the same recorded key elsewhere is reported.
GIT_FIXTURE="$TMP/history"
mkdir -p "$GIT_FIXTURE"
cp "$ROOT/.gitleaks.toml" "$GIT_FIXTURE/.gitleaks.toml"
KNOWN_1=$(printf '%s%s' 'b4320227e2113d521e618f67fa762713' '2d4fc816795343ecf1f67198a76c806f')
KNOWN_2=$(printf '%s%s' 'bc76c634c07de278b6d37b05a93bb3f1' '4c077dcc6263b213bf660747e1d976eb')
NOVEL=$(printf '%s%s' '7d3f9a1c5e8b2d4f6a0c9e7b1d3f5a8c' '2e4b6d8f0a1c3e5b7d9f1a3c5e7b9d2f')
for pair in "ac3-native-run:$KNOWN_1" "ac3-native-run-2:$KNOWN_2"; do
  run=${pair%%:*}
  mkdir -p "$GIT_FIXTURE/docs/verification/evidence/tsk-115/$run"
  printf '"project_key": "%s"\napi_key = "%s"\n' "${pair#*:}" "$NOVEL" \
    >"$GIT_FIXTURE/docs/verification/evidence/tsk-115/$run/present-state.txt"
done
mkdir -p "$GIT_FIXTURE/docs/other"
printf '"project_key": "%s"\n' "$KNOWN_1" >"$GIT_FIXTURE/docs/other/present-state.txt"
git -C "$GIT_FIXTURE" init -q
git -C "$GIT_FIXTURE" add -A
git -C "$GIT_FIXTURE" -c user.name=canary -c user.email=canary@example.invalid \
  -c commit.gpgsign=false commit -q -m "canary"
"$GITLEAKS" detect --source "$GIT_FIXTURE" --no-banner --redact \
  --config "$GIT_FIXTURE/.gitleaks.toml" --report-format json \
  --report-path "$TMP/history.json" --exit-code 0 >/dev/null

python3 - "$TMP/history.json" <<'PY'
import json
import sys

findings = json.load(open(sys.argv[1], encoding="utf-8"))
found = sorted((finding.get("File"), finding.get("StartLine")) for finding in findings)
evidence = "docs/verification/evidence/tsk-115/{}/present-state.txt"
expected = sorted([
    (evidence.format("ac3-native-run"), 2),
    (evidence.format("ac3-native-run-2"), 2),
    ("docs/other/present-state.txt", 1),
])
if found != expected:
    raise SystemExit(f"expected evidence-allowance findings {expected}, got {found}")
PY

printf '%s\n' "gitleaks allowlist canary passed"
