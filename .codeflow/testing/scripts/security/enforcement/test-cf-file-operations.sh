#!/usr/bin/env bash
# Test: cf-file-operations.sh
# Tests the file operations enforcement module (Sections 9-10)
# Includes structural checks and functional (sourced-module) tests.

set -uo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../../lib/test-isolation.sh"
ENFORCEMENT_DIR="$REAL_REPO_ROOT/.codeflow/scripts/security/enforcement"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
HOOK="${HOOK_OVERRIDE:-$ENFORCEMENT_DIR/cf-file-operations.sh}"

export REPO_ROOT LIB_DIR

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

# Run module in subshell with given COMMAND, check exit code
run_module() {
  local test_command="$1"
  local expected_exit="$2"
  local actual_exit
  (
    set -euo pipefail
    export COMMAND="$test_command"
    export LIB_DIR REPO_ROOT
    PROTECTED_PATHS=(".claude/hooks" ".claude/settings.json" ".codeflow/scripts/security")
    export PROTECTED_PATHS
    export INDIRECT_WRITE_CMDS="(cp|dd|tee|rsync|scp|install|ln)"
    unset _SECURITY_LIB_SOURCED
    source "$HOOK"
  ) >/dev/null 2>&1
  actual_exit=$?
  if [[ "$actual_exit" -eq "$expected_exit" ]]; then
    return 0
  else
    return 1
  fi
}

echo "=== Testing cf-file-operations.sh ==="
echo ""
echo "--- Part 1: Structural Checks ---"
if [[ -f "$HOOK" ]]; then pass "Module file exists"; else fail "Module not found"; fi
if [[ -x "$HOOK" ]]; then pass "Module is executable"; else fail "Module not executable"; fi
if command -v shellcheck &>/dev/null; then
  if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then pass "Passes shellcheck"; else fail "Fails shellcheck"; fi
else
  pass "Shellcheck not available (skipped)"
fi
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then pass "Has proper header"; else fail "Missing proper header"; fi
if grep -q "set -euo pipefail" "$HOOK"; then pass "Uses strict mode"; else fail "Missing strict mode"; fi
if grep -q "source.*security-lib.sh" "$HOOK"; then pass "Sources security-lib.sh"; else fail "Missing security-lib source"; fi
if grep -q "SECTION 8" "$HOOK"; then fail "Section 8 should be removed"; else pass "Section 8 removed"; fi
if grep -q "EXECUTION_BLOCKED_PATHS" "$HOOK"; then fail "EXECUTION_BLOCKED_PATHS should be removed"; else pass "EXECUTION_BLOCKED_PATHS removed"; fi
if grep -q "SECTION 9" "$HOOK"; then pass "Has Section 9"; else fail "Missing Section 9"; fi
if grep -q "SECTION 10" "$HOOK"; then pass "Has Section 10"; else fail "Missing Section 10"; fi
if grep -q "Sections 9-11" "$HOOK"; then pass "Header says Sections 9-11"; else fail "Header should say Sections 9-11"; fi
if grep -q "SECTION 11" "$HOOK"; then pass "Has Section 11"; else fail "Missing Section 11"; fi
if grep -q "check_interpreter_write" "$HOOK"; then pass "Has interpreter detection"; else fail "Missing interpreter detection"; fi
if grep -q "PROTECTED_PATHS" "$HOOK"; then pass "Uses PROTECTED_PATHS"; else fail "Missing PROTECTED_PATHS"; fi
if grep -q "block_command" "$HOOK"; then pass "Uses block_command"; else fail "Missing block_command"; fi
if grep -q "INDIRECT_WRITE_CMDS" "$HOOK"; then pass "Uses INDIRECT_WRITE_CMDS"; else fail "Missing INDIRECT_WRITE_CMDS"; fi
if grep -q "of=" "$HOOK"; then pass "Handles dd of= parameter"; else fail "Missing dd of= handling"; fi
if grep -q "tee" "$HOOK"; then pass "Handles piped tee"; else fail "Missing piped tee handling"; fi
if grep -q "append" "$HOOK"; then pass "Handles cat append"; else fail "Missing cat append handling"; fi
if grep -q "return 0" "$HOOK"; then pass "Returns 0 at end"; else fail "Missing return 0"; fi
if grep -q "/tmp/claude" "$HOOK"; then pass "Has /tmp/claude skip"; else fail "Missing /tmp/claude skip"; fi
if grep -q "is_path_or_glob_targeted" "$HOOK"; then pass "Uses is_path_or_glob_targeted"; else fail "Missing is_path_or_glob_targeted"; fi
echo ""
echo "--- Part 2: Script Execution NOT Blocked ---"
if run_module "bash .claude/hooks/codeflow/stop/hook.sh" 0; then pass "bash hook -> ALLOWED"; else fail "bash hook should be ALLOWED"; fi
if run_module "python .codeflow/scripts/security/script.py" 0; then pass "python script -> ALLOWED"; else fail "python script should be ALLOWED"; fi
if run_module "sh .claude/hooks/codeflow/pre-tool-use/hook.sh" 0; then pass "sh hook -> ALLOWED"; else fail "sh hook should be ALLOWED"; fi
if run_module "./.claude/hooks/codeflow/stop/hook.sh" 0; then pass "./hook.sh -> ALLOWED"; else fail "./hook.sh should be ALLOWED"; fi
if run_module "python3 .codeflow/scripts/security/lib/security-lib.sh" 0; then pass "python3 script -> ALLOWED"; else fail "python3 script should be ALLOWED"; fi
echo ""
echo "--- Part 3: Indirect Writes BLOCKED ---"
if run_module "cp /home/user/file .claude/hooks/newfile" 2; then pass "cp to protected -> BLOCKED"; else fail "cp to protected should be BLOCKED"; fi
if run_module "cp .claude/hooks/codeflow/hook.sh /tmp/claude/backup.sh" 0; then pass "cp from protected to /tmp/claude -> ALLOWED"; else fail "cp from protected to /tmp/claude should be ALLOWED"; fi
dd_CMD="dd if=/dev/zero of=.claude/settings.json bs=1 count=10"
if run_module "$dd_CMD" 2; then pass "dd of=protected -> BLOCKED"; else fail "dd of=protected should be BLOCKED"; fi
if run_module "echo data | tee .claude/hooks/newfile" 2; then pass "tee to protected -> BLOCKED"; else fail "tee to protected should be BLOCKED"; fi
if run_module "cat somefile >> .claude/settings.json" 2; then pass "cat >> protected -> BLOCKED"; else fail "cat >> protected should be BLOCKED"; fi
if run_module "rsync -av /home/user/file .claude/hooks/dest" 2; then pass "rsync to protected -> BLOCKED"; else fail "rsync to protected should be BLOCKED"; fi
if run_module "install /home/user/file .claude/hooks/dest" 2; then pass "install to protected -> BLOCKED"; else fail "install to protected should be BLOCKED"; fi
if run_module "ln -s /home/user/file .claude/hooks/link" 2; then pass "ln to protected -> BLOCKED"; else fail "ln to protected should be BLOCKED"; fi
if run_module "scp user@host:file .claude/hooks/newfile" 2; then pass "scp to protected -> BLOCKED"; else fail "scp to protected should be BLOCKED"; fi
echo ""
echo "--- Part 4: /tmp/claude Destination Skip ---"
if run_module "cp /tmp/claude/file /tmp/claude/other" 0; then pass "cp within /tmp/claude -> ALLOWED"; else fail "cp within /tmp/claude should be ALLOWED"; fi
if run_module "rsync -av .claude/hooks/hook.sh /tmp/claude/backup" 0; then pass "rsync to /tmp/claude -> ALLOWED"; else fail "rsync to /tmp/claude should be ALLOWED"; fi
if run_module "echo data | tee /tmp/claude/outfile" 0; then pass "tee to /tmp/claude -> ALLOWED"; else fail "tee to /tmp/claude should be ALLOWED"; fi
if run_module "echo data >/tmp/claude/outfile" 0; then pass "redirect > to /tmp/claude -> ALLOWED"; else fail "redirect > to /tmp/claude should be ALLOWED"; fi
echo ""
echo "--- Part 5: Compound Command /tmp/claude Handling ---"
if run_module "cp file /tmp/claude/x && cp /tmp/claude/x .claude/settings.json" 2; then pass "compound: cp to /tmp then to protected -> BLOCKED"; else fail "compound: cp to /tmp then to protected should be BLOCKED"; fi
# Compound commands with protected paths + write cmds are blocked conservatively
# (script cannot distinguish source vs destination across compound segments)
if run_module "cp .claude/hooks/hook.sh /tmp/claude/backup.sh && ls" 2; then pass "compound: cp from protected with && -> BLOCKED (conservative)"; else fail "compound: cp from protected with && should be BLOCKED (conservative)"; fi
echo ""
echo "--- Part 6: Glob Bypass Prevention ---"
if run_module "cp /home/user/src .claude/hook?" 2; then pass "Glob ? bypass -> BLOCKED"; else fail "Glob ? bypass should be BLOCKED"; fi
# Build glob * command without literal glob in source to avoid test-time expansion
GLOB_CMD="cp /home/src .claude/hoo"
GLOB_CMD="${GLOB_CMD}*"
if run_module "$GLOB_CMD" 2; then pass "Glob * bypass -> BLOCKED"; else fail "Glob * bypass should be BLOCKED"; fi
echo ""
echo "--- Part 7: Non-Write Commands ALLOWED ---"
if run_module "ls -la /tmp/claude/somefile" 0; then pass "Safe ls -> ALLOWED"; else fail "Safe ls should be ALLOWED"; fi
if run_module "echo hello world" 0; then pass "Normal echo -> ALLOWED"; else fail "Normal echo should be ALLOWED"; fi
if run_module "cp somefile /home/user/dest" 0; then pass "cp to non-protected -> ALLOWED"; else fail "cp to non-protected should be ALLOWED"; fi
if run_module "" 0; then pass "Empty command -> ALLOWED"; else fail "Empty command should be ALLOWED"; fi
if run_module "cat .claude/settings.json" 0; then pass "cat read protected -> ALLOWED"; else fail "cat read of protected should be ALLOWED"; fi
echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"
[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
