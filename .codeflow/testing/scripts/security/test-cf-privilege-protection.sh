#!/usr/bin/env bash
# Test: cf-privilege-protection.sh
# Location: .codeflow/testing/scripts/security/test-cf-privilege-protection.sh
#
# Tests the privilege escalation protection module
# Coverage: shell chaining, direct escalation, script bypass, env manipulation

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
MODULE="$ENFORCEMENT_DIR/cf-privilege-protection.sh"

export REPO_ROOT LIB_DIR

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Helper function to test command blocking
test_blocks_command() {
    local command="$1"
    local description="$2"
    local output

    output=$(COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" \
       bash -c "source '$MODULE'" 2>&1 || true)

    if echo "$output" | grep -q "BLOCKED"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected block"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Helper function to test command allowing
test_allows_command() {
    local command="$1"
    local description="$2"

    if COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" \
       bash -c "source '$MODULE'" 2>/dev/null; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected allow"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

echo "=== Testing cf-privilege-protection.sh ==="
echo ""

# =========================================================================
# SECTION 1: DIRECT PRIVILEGE ESCALATION
# =========================================================================
echo "--- Direct Privilege Escalation ---"

test_blocks_command "sudo apt install vim" "Block sudo with args"
test_blocks_command "sudo" "Block bare sudo"
test_blocks_command "su -" "Block su -"
test_blocks_command "su" "Block bare su"
test_blocks_command "doas command" "Block doas"
test_blocks_command "pkexec /usr/bin/command" "Block pkexec"
test_blocks_command "runuser -u user command" "Block runuser"

echo ""

# =========================================================================
# SECTION 2: SHELL CHAINING CHECKS
# =========================================================================
echo "--- Shell Chaining ---"

test_blocks_command "echo hello && sudo su" "Block && sudo chaining"
test_blocks_command "ls || sudo rm -rf" "Block || sudo chaining"
test_blocks_command "echo test; sudo apt update" "Block ; sudo chaining"
test_blocks_command "echo password | sudo -S command" "Block pipe to sudo"
test_blocks_command "cat file | su -c command" "Block pipe to su"
test_blocks_command "echo test && doas command" "Block && doas chaining"
test_blocks_command "cmd1; pkexec cmd2" "Block ; pkexec chaining"

echo ""

# =========================================================================
# SECTION 3: SCRIPT EXECUTION BYPASS
# =========================================================================
echo "--- Script Execution Bypass ---"

test_blocks_command "bash -c 'malicious command'" "Block bash -c single quotes"
test_blocks_command 'bash -c "malicious command"' "Block bash -c double quotes"
test_blocks_command "sh -c 'dangerous'" "Block sh -c"
test_blocks_command "zsh -c 'dangerous'" "Block zsh -c"
test_blocks_command "eval rm -rf /tmp/important" "Block eval with rm"
test_blocks_command "eval sudo apt install" "Block eval with sudo"
test_blocks_command "source /etc/profile" "Block source command"
test_blocks_command ". /etc/profile" "Block dot source"
test_blocks_command ". ./script.sh" "Block dot source with relative path"

echo ""

# =========================================================================
# SECTION 4: ENVIRONMENT VARIABLE MANIPULATION
# =========================================================================
echo "--- Environment Variable Manipulation ---"

test_blocks_command "LD_PRELOAD=/evil.so command" "Block LD_PRELOAD"
test_blocks_command "LD_LIBRARY_PATH=/evil/lib command" "Block LD_LIBRARY_PATH"
test_blocks_command "PATH=/usr/bin:/tmp command" "Block PATH with /tmp"

echo ""

# =========================================================================
# SECTION 5: SAFE COMMANDS (should NOT block)
# =========================================================================
echo "--- Safe Commands (should allow) ---"

test_allows_command "ls -la" "Allow ls"
test_allows_command "npm install package" "Allow npm install"
test_allows_command "git status" "Allow git status"
test_allows_command "pip install package" "Allow pip install"
test_allows_command "python3 script.py" "Allow python3"
test_allows_command "echo hello world" "Allow echo"
test_allows_command "eval echo hello" "Allow eval with safe command"
test_allows_command "grep -r sudo README.md" "Allow grep for word sudo"
test_allows_command "cat /etc/sudoers.bak" "Allow cat with sudo in filename"
test_allows_command "PATH=/usr/local/bin:/usr/bin command" "Allow PATH without /tmp"
test_allows_command "bash script.sh" "Allow bash without -c"
test_allows_command "sh script.sh" "Allow sh without -c"

echo ""

# =========================================================================
# SECTION 6: SUBSTRING FALSE POSITIVES (should NOT block)
# =========================================================================
echo "--- Substring False Positives ---"

test_allows_command "visudo" "Allow visudo (contains sudo substring)"
test_allows_command "visudo /etc/sudoers" "Allow visudo with args"
test_allows_command "echo sudo is a powerful command" "Allow sudo as non-command word"
test_allows_command "pseudocode review" "Allow pseudocode (contains sudo substring)"
test_allows_command "echo && visudo /etc/sudoers" "Allow chained visudo (not sudo)"
test_allows_command "echo 'use sudo carefully'" "Allow sudo in quoted string"

echo ""

# =========================================================================
# SECTION 7: SCRIPT BYPASS EDGE CASES
# =========================================================================
echo "--- Script Bypass Edge Cases ---"

test_blocks_command ". .bashrc" "Block dot source with dotfile"
test_blocks_command ". ./relative/script.sh" "Block dot source with nested relative"
test_blocks_command "eval runuser -u nobody cmd" "Block eval with runuser"
test_blocks_command "eval doas command" "Block eval with doas"
test_allows_command "bash -c command_no_quotes" "Allow bash -c without quotes"
test_allows_command "eval echo safe output" "Allow eval with non-dangerous command"
test_allows_command "zsh script.zsh" "Allow zsh without -c"

echo ""

# =========================================================================
# SECTION 8: CHAINING EDGE CASES
# =========================================================================
echo "--- Chaining Edge Cases ---"

test_blocks_command "echo test && runuser -u user cmd" "Block && runuser chaining"
test_blocks_command "echo test; runuser -u user cmd" "Block ; runuser chaining"
test_blocks_command "echo | runuser -u user cmd" "Block pipe to runuser"
test_blocks_command "echo test || doas command" "Block || doas chaining"
test_blocks_command "cat | doas command" "Block pipe to doas"
test_blocks_command "echo test; doas command" "Block ; doas chaining"
test_allows_command "echo '&&' sudo test" "Allow chain operators in quotes"

echo ""

# =========================================================================
# SECTION 9: ENV VAR EDGE CASES
# =========================================================================
echo "--- Env Var Edge Cases ---"

test_blocks_command "export LD_PRELOAD=/evil.so" "Block export LD_PRELOAD"
test_blocks_command "cmd LD_PRELOAD=/lib.so other" "Block LD_PRELOAD mid-command"
test_blocks_command "export LD_LIBRARY_PATH=/evil" "Block export LD_LIBRARY_PATH"
test_allows_command "echo LD_PRELOAD is dangerous" "Allow LD_PRELOAD in echo text"
test_allows_command "grep LD_PRELOAD config.txt" "Allow grep for LD_PRELOAD"

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
