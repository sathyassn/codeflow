#!/usr/bin/env bash
# Test: cf-dangerous-commands.sh
# Location: .codeflow/testing/scripts/security/test-cf-dangerous-commands.sh
#
# Tests the dangerous command detection module

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
MODULE="$ENFORCEMENT_DIR/cf-dangerous-commands.sh"

# Set security-specific LIB_DIR
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
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

echo "=== Testing cf-dangerous-commands.sh ==="
echo ""

# --- System destruction ---
echo "--- System Destruction ---"
test_blocks_command "rm -rf /" "Should block rm -rf /"
test_blocks_command "rm -rf /*" "Should block rm -rf /*"
test_blocks_command "rm -rf ~" "Should block rm -rf ~"
test_blocks_command "rm -rf ~/*" "Should block rm -rf ~/*"
test_blocks_command "rm -r /" "Should block rm -r / (without -f)"
test_blocks_command "rm  -rf  /" "Should block rm -rf / with extra spaces"
test_blocks_command "rm -rR /" "Should block rm with -rR flags"

# --- System directory protection ---
echo ""
echo "--- System Directory Protection ---"
test_blocks_command "rm -rf /etc" "Should block rm -rf /etc"
test_blocks_command "rm -rf /var" "Should block rm -rf /var"
test_blocks_command "rm -rf /usr" "Should block rm -rf /usr"
test_blocks_command "rm -rf /bin" "Should block rm -rf /bin"
test_blocks_command "rm -rf /sbin" "Should block rm -rf /sbin"
test_blocks_command "rm -rf /boot" "Should block rm -rf /boot"

# --- Disk operations ---
echo ""
echo "--- Disk Operations ---"
test_blocks_command "dd if=/dev/zero of=/dev/sda" "Should block dd to sda"
test_blocks_command "dd if=/dev/random of=file" "Should block dd from /dev/random"
test_blocks_command "dd if=/dev/zero of=/dev/nvme0n1" "Should block dd to nvme"
test_blocks_command "mkfs.ext4 /dev/sda1" "Should block mkfs"
test_blocks_command "mke2fs /dev/sda1" "Should block mke2fs"
test_blocks_command "mkswap /dev/sda2" "Should block mkswap"
test_blocks_command "> /dev/sda" "Should block redirect to /dev/sda"

# --- Dangerous permissions ---
echo ""
echo "--- Dangerous Permissions ---"
test_blocks_command "chmod 777 /" "Should block chmod 777 /"
test_blocks_command "chmod -R 777 /" "Should block chmod -R 777 /"
test_blocks_command "chmod 777 /etc" "Should block chmod 777 /etc"
test_blocks_command "chmod 777 /usr" "Should block chmod 777 /usr"
test_blocks_command "chmod 777 /var" "Should block chmod 777 /var"
test_blocks_command "chmod -R 755 /" "Should block chmod -R on root"
test_blocks_command "chown -R root:root /" "Should block chown -R on root"

# --- Fork bombs ---
echo ""
echo "--- Fork Bombs ---"
test_blocks_command ":(){:|:&};:" "Should block fork bomb (no spaces)"
test_blocks_command ":(){ :|:& };:" "Should block fork bomb (with spaces)"
test_blocks_command ".(){.|.&};." "Should block dot variant fork bomb"

# --- False positives (MUST allow) ---
echo ""
echo "--- False Positives (must allow) ---"
test_allows_command "rm -rf ./build/" "Should allow rm -rf ./build/"
test_allows_command "rm -rf node_modules" "Should allow rm -rf node_modules"
test_allows_command "rm -rf /tmp/claude/test" "Should allow rm -rf /tmp/claude/test"
test_allows_command "rm -f /tmp/testfile" "Should allow safe rm"
test_allows_command "ls -la" "Should allow ls"
test_allows_command "chmod +x script.sh" "Should allow chmod +x on files"
test_allows_command "chmod 644 file.txt" "Should allow chmod 644"
test_allows_command "chmod 777 ./mydir" "Should allow chmod 777 on project dir"
test_allows_command "chmod 777 myfile.txt" "Should allow chmod 777 on relative file"
test_allows_command "dd if=disk.img of=backup.img" "Should allow dd between files"

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
