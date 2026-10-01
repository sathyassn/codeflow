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
for run in ac3-native-run ac3-native-run-2; do
  mkdir -p "$TMP/docs/verification/evidence/tsk-115/$run"
  printf 'api_key = "%s%s"\n' '7d3f9a1c5e8b2d4f6a0c9e7b1d3f5a8c' '2e4b6d8f0a1c3e5b7d9f1a3c5e7b9d2f' \
    >"$TMP/docs/verification/evidence/tsk-115/$run/present-state.txt"
done

"$GITLEAKS" detect --source "$TMP" --no-git --no-banner --redact \
  --config "$TMP/.gitleaks.toml" --report-format json \
  --report-path "$TMP/report.json" --exit-code 0 >/dev/null

python3 - "$TMP/report.json" <<'PY'
import json
import sys

findings = json.load(open(sys.argv[1], encoding="utf-8"))
rules = sorted(finding.get("RuleID") for finding in findings)
expected = sorted(["aws-access-token", "generic-api-key", "generic-api-key", "generic-api-key", "private-key"])
if rules != expected:
    raise SystemExit(f"expected novel credential findings {expected}, got {rules}")
PY

printf '%s\n' "gitleaks allowlist canary passed"
