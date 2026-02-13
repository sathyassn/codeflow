#!/usr/bin/env bash
# Test: bash-file-readers-lib.sh
# Location: .codeflow/testing/scripts/security/lib/test-bash-file-readers-lib.sh
# Compatibility: macOS/Linux (bash 3.2+)
#
# Comprehensive tests for the bash file readers detection library.
# Covers all 9 detection categories, evasion detection, command substitution,
# extract_target_file(), normalize_command(), and negative cases.

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
MODULE="$LIB_DIR/bash-file-readers-lib.sh"

export REPO_ROOT LIB_DIR

# Source the module to test functions
# shellcheck source=/dev/null
source "$MODULE"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

echo "=== Testing bash-file-readers-lib.sh ==="
echo ""

# ============================================================================
# Structure and metadata tests
# ============================================================================
echo "--- Structure and metadata ---"

if [[ -f "$MODULE" ]]; then pass "Module file exists"; else fail "Module file not found"; fi

if [[ -x "$MODULE" ]]; then pass "Module is executable"; else fail "Module not executable"; fi

if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$MODULE" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

if grep -q "Purpose:" "$MODULE" && grep -q "Usage:" "$MODULE"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

if grep -q "Location:" "$MODULE"; then
    pass "Has Location header"
else
    fail "Missing Location header"
fi

if grep -q "Compatibility:" "$MODULE"; then
    pass "Has Compatibility header"
else
    fail "Missing Compatibility header"
fi

if grep -q '_BASH_FILE_READERS_LIB_LOADED' "$MODULE"; then
    pass "Has multiple-source guard"
else
    fail "Should have multiple-source guard"
fi

if grep -q 'BASH_FILE_READERS_LIB_VERSION' "$MODULE"; then
    pass "Has version constant"
else
    fail "Should have version constant"
fi

if grep -q 'BASH_SOURCE\[0\].*==.*\${0' "$MODULE" || grep -q 'This is a library file' "$MODULE"; then
    pass "Has library guard (prevent direct execution)"
else
    fail "Missing library guard"
fi

# ============================================================================
# Function existence tests
# ============================================================================
echo ""
echo "--- Function existence ---"

for fn in is_file_reading_command detect_indirect_read extract_target_file \
          get_detection_category is_cat_family is_text_processor is_search_tool \
          is_shell_builtin is_diff_tool is_encoding_tool is_archive_reader \
          is_network_fetcher is_utility_tool is_path_evasion normalize_command; do
    if declare -f "$fn" &>/dev/null; then
        pass "Has $fn function"
    else
        fail "Missing $fn function"
    fi
done

# ============================================================================
# Category 1: Cat family (positive + negative)
# ============================================================================
echo ""
echo "--- Category 1: Cat family ---"

if is_cat_family "cat /etc/passwd"; then pass "cat detected"; else fail "cat not detected"; fi
if is_cat_family "bat file.txt"; then pass "bat detected"; else fail "bat not detected"; fi
if is_cat_family "head -n 10 file.txt"; then pass "head detected"; else fail "head not detected"; fi
if is_cat_family "tail -f /var/log/syslog"; then pass "tail detected"; else fail "tail not detected"; fi
if is_cat_family "less file.txt"; then pass "less detected"; else fail "less not detected"; fi
if is_cat_family "more file.txt"; then pass "more detected"; else fail "more not detected"; fi
if is_cat_family "nl file.txt"; then pass "nl detected"; else fail "nl not detected"; fi
if is_cat_family "tac file.txt"; then pass "tac detected"; else fail "tac not detected"; fi
if is_cat_family "rev file.txt"; then pass "rev detected"; else fail "rev not detected"; fi
if is_cat_family "fold -w 80 file.txt"; then pass "fold detected"; else fail "fold not detected"; fi
if is_cat_family "shuf file.txt"; then pass "shuf detected"; else fail "shuf not detected"; fi

# Negative cases
if ! is_cat_family "echo hello"; then pass "echo NOT cat_family"; else fail "echo should not be cat_family"; fi
if ! is_cat_family "ls -la"; then pass "ls NOT cat_family"; else fail "ls should not be cat_family"; fi
if ! is_cat_family "mkdir /tmp/test"; then pass "mkdir NOT cat_family"; else fail "mkdir should not be cat_family"; fi

# ============================================================================
# Category 2: Text processors (positive + negative)
# ============================================================================
echo ""
echo "--- Category 2: Text processors ---"

if is_text_processor "awk '{print \$1}' file.txt"; then pass "awk detected"; else fail "awk not detected"; fi
if is_text_processor "gawk '{print \$1}' file.txt"; then pass "gawk detected"; else fail "gawk not detected"; fi
if is_text_processor "sed 's/foo/bar/g' file.txt"; then pass "sed detected"; else fail "sed not detected"; fi
if is_text_processor "gsed 's/foo/bar/g' file.txt"; then pass "gsed detected"; else fail "gsed not detected"; fi
if is_text_processor "perl -p -e 's/foo/bar/g' file.txt"; then pass "perl detected"; else fail "perl not detected"; fi
if is_text_processor "ruby file.rb"; then pass "ruby detected"; else fail "ruby not detected"; fi
if is_text_processor "cut -d: -f1 file.txt"; then pass "cut detected"; else fail "cut not detected"; fi
if is_text_processor "sort file.txt"; then pass "sort detected"; else fail "sort not detected"; fi
if is_text_processor "uniq file.txt"; then pass "uniq detected"; else fail "uniq not detected"; fi
if is_text_processor "paste file1 file2"; then pass "paste detected"; else fail "paste not detected"; fi
if is_text_processor "join file1 file2"; then pass "join detected"; else fail "join not detected"; fi
if is_text_processor "column -t file.txt"; then pass "column detected"; else fail "column not detected"; fi

# Python with file ops
if is_text_processor "python3 -c 'open(\"/etc/passwd\").read()'"; then
    pass "python3 -c with open() detected"
else
    fail "python3 -c with open() not detected"
fi

# Python without file ops should NOT match
if ! is_text_processor "python3 -c 'print(42)'"; then
    pass "python3 -c without file ops NOT detected"
else
    fail "python3 -c without file ops should not match"
fi

# Negative cases
if ! is_text_processor "echo hello"; then pass "echo NOT text_processor"; else fail "echo should not be text_processor"; fi
if ! is_text_processor "cp file1 file2"; then pass "cp NOT text_processor"; else fail "cp should not be text_processor"; fi

# ============================================================================
# Category 3: Search tools (positive + negative)
# ============================================================================
echo ""
echo "--- Category 3: Search tools ---"

if is_search_tool "grep 'pattern' file.txt"; then pass "grep detected"; else fail "grep not detected"; fi
if is_search_tool "egrep 'pattern' file.txt"; then pass "egrep detected"; else fail "egrep not detected"; fi
if is_search_tool "fgrep 'string' file.txt"; then pass "fgrep detected"; else fail "fgrep not detected"; fi
if is_search_tool "rg 'pattern' /path"; then pass "rg detected"; else fail "rg not detected"; fi
if is_search_tool "ag 'pattern' /path"; then pass "ag detected"; else fail "ag not detected"; fi
if is_search_tool "ack 'pattern' /path"; then pass "ack detected"; else fail "ack not detected"; fi

# find with -exec cat
if is_search_tool "find /tmp -name '*.txt' -exec cat {} ;"; then
    pass "find -exec cat detected"
else
    fail "find -exec cat not detected"
fi

# find without -exec should NOT match
if ! is_search_tool "find /tmp -name '*.txt'"; then
    pass "find without -exec NOT detected"
else
    fail "find without -exec should not match"
fi

# Negative cases
if ! is_search_tool "ls -la"; then pass "ls NOT search_tool"; else fail "ls should not be search_tool"; fi

# ============================================================================
# Category 4: Shell builtins (positive + negative)
# ============================================================================
echo ""
echo "--- Category 4: Shell builtins ---"

if is_shell_builtin "source /etc/profile"; then pass "source detected"; else fail "source not detected"; fi
if is_shell_builtin ". /etc/profile"; then pass "dot-source detected"; else fail "dot-source not detected"; fi
if is_shell_builtin "read line < /etc/passwd"; then pass "read with redirect detected"; else fail "read with redirect not detected"; fi
if is_shell_builtin "mapfile lines < /etc/passwd"; then pass "mapfile detected"; else fail "mapfile not detected"; fi
if is_shell_builtin "readarray lines < /etc/passwd"; then pass "readarray detected"; else fail "readarray not detected"; fi
if is_shell_builtin "exec 3< /etc/passwd"; then pass "exec with redirect detected"; else fail "exec with redirect not detected"; fi

# $(<file) pattern
if is_shell_builtin 'content=$(<file.txt)'; then
    pass "\$(<file) pattern detected"
else
    fail "\$(<file) pattern not detected"
fi

# while read loop
if is_shell_builtin "while read line; do echo \$line; done"; then
    pass "while read loop detected"
else
    fail "while read loop not detected"
fi

# Negative cases
if ! is_shell_builtin "echo hello"; then pass "echo NOT shell_builtin"; else fail "echo should not be shell_builtin"; fi
if ! is_shell_builtin "export FOO=bar"; then pass "export NOT shell_builtin"; else fail "export should not be shell_builtin"; fi

# ============================================================================
# Category 5: Diff tools (positive + negative)
# ============================================================================
echo ""
echo "--- Category 5: Diff tools ---"

if is_diff_tool "diff file1.txt file2.txt"; then pass "diff detected"; else fail "diff not detected"; fi
if is_diff_tool "diff3 file1 file2 file3"; then pass "diff3 detected"; else fail "diff3 not detected"; fi
if is_diff_tool "cmp file1 file2"; then pass "cmp detected"; else fail "cmp not detected"; fi
if is_diff_tool "comm file1 file2"; then pass "comm detected"; else fail "comm not detected"; fi
if is_diff_tool "sdiff file1 file2"; then pass "sdiff detected"; else fail "sdiff not detected"; fi
if is_diff_tool "colordiff file1 file2"; then pass "colordiff detected"; else fail "colordiff not detected"; fi
if is_diff_tool "delta file1 file2"; then pass "delta detected"; else fail "delta not detected"; fi

# Negative cases
if ! is_diff_tool "cp file1 file2"; then pass "cp NOT diff_tool"; else fail "cp should not be diff_tool"; fi
if ! is_diff_tool "mv file1 file2"; then pass "mv NOT diff_tool"; else fail "mv should not be diff_tool"; fi

# ============================================================================
# Category 6: Encoding tools (positive + negative)
# ============================================================================
echo ""
echo "--- Category 6: Encoding tools ---"

if is_encoding_tool "base64 file.txt"; then pass "base64 detected"; else fail "base64 not detected"; fi
if is_encoding_tool "base32 file.txt"; then pass "base32 detected"; else fail "base32 not detected"; fi
if is_encoding_tool "xxd file.bin"; then pass "xxd detected"; else fail "xxd not detected"; fi
if is_encoding_tool "od -c file.bin"; then pass "od detected"; else fail "od not detected"; fi
if is_encoding_tool "hexdump file.bin"; then pass "hexdump detected"; else fail "hexdump not detected"; fi
if is_encoding_tool "hd file.bin"; then pass "hd detected"; else fail "hd not detected"; fi
if is_encoding_tool "strings binary"; then pass "strings detected"; else fail "strings not detected"; fi
if is_encoding_tool "file something.bin"; then pass "file detected"; else fail "file not detected"; fi

# Negative cases
if ! is_encoding_tool "echo hello"; then pass "echo NOT encoding_tool"; else fail "echo should not be encoding_tool"; fi

# ============================================================================
# Category 7: Archive readers (positive + negative)
# ============================================================================
echo ""
echo "--- Category 7: Archive readers ---"

if is_archive_reader "zcat file.gz"; then pass "zcat detected"; else fail "zcat not detected"; fi
if is_archive_reader "bzcat file.bz2"; then pass "bzcat detected"; else fail "bzcat not detected"; fi
if is_archive_reader "xzcat file.xz"; then pass "xzcat detected"; else fail "xzcat not detected"; fi
if is_archive_reader "zless file.gz"; then pass "zless detected"; else fail "zless not detected"; fi

# gunzip with -c flag
if is_archive_reader "gunzip -c file.gz"; then
    pass "gunzip -c detected"
else
    fail "gunzip -c not detected"
fi
if is_archive_reader "gunzip --stdout file.gz"; then
    pass "gunzip --stdout detected"
else
    fail "gunzip --stdout not detected"
fi

# gunzip WITHOUT -c should NOT match
if ! is_archive_reader "gunzip file.gz"; then
    pass "gunzip without -c NOT detected"
else
    fail "gunzip without -c should not match"
fi

# tar with -O flag
if is_archive_reader "tar -xOf archive.tar file.txt"; then
    pass "tar -xOf detected"
else
    fail "tar -xOf not detected"
fi
if is_archive_reader "tar --to-stdout -xf archive.tar"; then
    pass "tar --to-stdout detected"
else
    fail "tar --to-stdout not detected"
fi

# tar without -O should NOT match
if ! is_archive_reader "tar -xf archive.tar"; then
    pass "tar -xf without -O NOT detected"
else
    fail "tar -xf without -O should not match"
fi

# unzip with -p flag
if is_archive_reader "unzip -p archive.zip file.txt"; then
    pass "unzip -p detected"
else
    fail "unzip -p not detected"
fi

# unzip without -p should NOT match
if ! is_archive_reader "unzip archive.zip"; then
    pass "unzip without -p NOT detected"
else
    fail "unzip without -p should not match"
fi

# Negative cases
if ! is_archive_reader "gzip file.txt"; then pass "gzip NOT archive_reader"; else fail "gzip should not be archive_reader"; fi
if ! is_archive_reader "zip archive.zip file"; then pass "zip NOT archive_reader"; else fail "zip should not be archive_reader"; fi

# ============================================================================
# Category 8: Network fetchers (positive + negative)
# ============================================================================
echo ""
echo "--- Category 8: Network fetchers ---"

if is_network_fetcher "curl file:///etc/passwd"; then
    pass "curl file:// detected"
else
    fail "curl file:// not detected"
fi
if is_network_fetcher "wget file:///etc/passwd"; then
    pass "wget file:// detected"
else
    fail "wget file:// not detected"
fi

# curl/wget WITHOUT file:// should NOT match
if ! is_network_fetcher "curl https://example.com"; then
    pass "curl https:// NOT detected"
else
    fail "curl https:// should not match"
fi
if ! is_network_fetcher "wget https://example.com"; then
    pass "wget https:// NOT detected"
else
    fail "wget https:// should not match"
fi

# Non-network commands should NOT match
if ! is_network_fetcher "ls -la"; then pass "ls NOT network_fetcher"; else fail "ls should not be network_fetcher"; fi

# ============================================================================
# Category 9: Utility tools (positive + negative)
# ============================================================================
echo ""
echo "--- Category 9: Utility tools ---"

if is_utility_tool "dd if=/dev/sda of=/tmp/backup"; then
    pass "dd if= detected"
else
    fail "dd if= not detected"
fi

# dd without if= should NOT match
if ! is_utility_tool "dd of=/dev/null bs=1M count=100"; then
    pass "dd without if= NOT detected"
else
    fail "dd without if= should not match"
fi

if is_utility_tool "tee output.log"; then pass "tee detected"; else fail "tee not detected"; fi

if is_utility_tool "xargs cat"; then
    pass "xargs cat detected"
else
    fail "xargs cat not detected"
fi

if is_utility_tool "xargs grep pattern"; then
    pass "xargs grep detected"
else
    fail "xargs grep not detected"
fi

# xargs without file-reading command should NOT match
if ! is_utility_tool "xargs rm"; then
    pass "xargs rm NOT detected"
else
    fail "xargs rm should not match"
fi

# Negative cases
if ! is_utility_tool "echo hello"; then pass "echo NOT utility_tool"; else fail "echo should not be utility_tool"; fi

# ============================================================================
# Evasion detection (positive + negative)
# ============================================================================
echo ""
echo "--- Evasion detection ---"

# Full path evasion
if is_path_evasion "/bin/cat /etc/passwd"; then
    pass "Full path /bin/cat detected"
else
    fail "Full path /bin/cat not detected"
fi
if is_path_evasion "/usr/bin/head -n 10 file.txt"; then
    pass "Full path /usr/bin/head detected"
else
    fail "Full path /usr/bin/head not detected"
fi
if is_path_evasion "/usr/bin/grep pattern file.txt"; then
    pass "Full path /usr/bin/grep detected"
else
    fail "Full path /usr/bin/grep not detected"
fi

# env wrapper evasion
if is_path_evasion "env cat /etc/passwd"; then
    pass "env cat detected"
else
    fail "env cat not detected"
fi
if is_path_evasion "env VAR=val cat /etc/passwd"; then
    pass "env VAR=val cat detected"
else
    fail "env VAR=val cat not detected"
fi

# command wrapper evasion
if is_path_evasion "command cat /etc/passwd"; then
    pass "command cat detected"
else
    fail "command cat not detected"
fi
if is_path_evasion "command -v cat /etc/passwd"; then
    pass "command -v cat detected"
else
    fail "command -v cat not detected"
fi

# Negative cases - env/command with non-reading commands
if ! is_path_evasion "env ls -la"; then
    pass "env ls NOT evasion"
else
    fail "env ls should not be evasion"
fi
if ! is_path_evasion "command ls -la"; then
    pass "command ls NOT evasion"
else
    fail "command ls should not be evasion"
fi
if ! is_path_evasion "/usr/bin/ls -la"; then
    pass "/usr/bin/ls NOT evasion"
else
    fail "/usr/bin/ls should not be evasion"
fi

# ============================================================================
# Command substitution detection
# ============================================================================
echo ""
echo "--- Command substitution detection ---"

# $() syntax
if is_file_reading_command 'result=$(cat /etc/passwd)'; then
    pass "\$(cat file) detected"
else
    fail "\$(cat file) not detected"
fi
if is_file_reading_command 'var=$(grep pattern file.txt)'; then
    pass "\$(grep pattern file) detected"
else
    fail "\$(grep pattern file) not detected"
fi

# Backtick syntax
# shellcheck disable=SC2016
if is_file_reading_command 'result=`cat /etc/passwd`'; then
    pass "Backtick cat detected"
else
    fail "Backtick cat not detected"
fi
# shellcheck disable=SC2016
if is_file_reading_command 'result=`head -1 file.txt`'; then
    pass "Backtick head detected"
else
    fail "Backtick head not detected"
fi

# Pipeline with tee
if is_file_reading_command "echo hello | tee output.log"; then
    pass "Pipeline with tee detected"
else
    fail "Pipeline with tee not detected"
fi

# Pipeline with xargs cat
if is_file_reading_command "find . -name '*.txt' | xargs cat"; then
    pass "Pipeline with xargs cat detected"
else
    fail "Pipeline with xargs cat not detected"
fi

# ============================================================================
# normalize_command() tests
# ============================================================================
echo ""
echo "--- normalize_command ---"

result=$(normalize_command "cat /etc/passwd")
if [[ "$result" == "cat" ]]; then pass "normalize: cat => cat"; else fail "normalize: expected cat, got $result"; fi

result=$(normalize_command "/usr/bin/grep pattern file")
if [[ "$result" == "grep" ]]; then pass "normalize: /usr/bin/grep => grep"; else fail "normalize: expected grep, got $result"; fi

result=$(normalize_command "env cat /etc/passwd")
if [[ "$result" == "cat" ]]; then pass "normalize: env cat => cat"; else fail "normalize: expected cat, got $result"; fi

result=$(normalize_command "env VAR=val head -n 5 file")
if [[ "$result" == "head" ]]; then pass "normalize: env VAR=val head => head"; else fail "normalize: expected head, got $result"; fi

result=$(normalize_command "command grep pattern")
if [[ "$result" == "grep" ]]; then pass "normalize: command grep => grep"; else fail "normalize: expected grep, got $result"; fi

result=$(normalize_command "command -p sed 's/x/y/' file")
if [[ "$result" == "sed" ]]; then pass "normalize: command -p sed => sed"; else fail "normalize: expected sed, got $result"; fi

# ============================================================================
# extract_target_file() tests
# ============================================================================
echo ""
echo "--- extract_target_file ---"

# Quoted path
result=$(extract_target_file 'cat "/etc/passwd"')
if [[ "$result" == "/etc/passwd" ]]; then
    pass "extract: quoted /etc/passwd"
else
    fail "extract: expected /etc/passwd, got ${result:-<empty>}"
fi

# file:// URL
result=$(extract_target_file "curl file:///etc/shadow")
if [[ "$result" == "/etc/shadow" ]]; then
    pass "extract: file:///etc/shadow"
else
    fail "extract: expected /etc/shadow, got ${result:-<empty>}"
fi

# Redirect
result=$(extract_target_file "read line < /etc/passwd")
if [[ "$result" == "/etc/passwd" ]]; then
    pass "extract: redirect < /etc/passwd"
else
    fail "extract: expected /etc/passwd, got ${result:-<empty>}"
fi

# Last arg as absolute path
result=$(extract_target_file "cat /var/log/syslog")
if [[ "$result" == "/var/log/syslog" ]]; then
    pass "extract: last arg /var/log/syslog"
else
    fail "extract: expected /var/log/syslog, got ${result:-<empty>}"
fi

# Relative path starting with ./
result=$(extract_target_file "cat ./config.txt")
if [[ "$result" == "./config.txt" ]]; then
    pass "extract: relative ./config.txt"
else
    fail "extract: expected ./config.txt, got ${result:-<empty>}"
fi

# Empty command returns failure
if ! extract_target_file "" &>/dev/null; then
    pass "extract: empty command returns 1"
else
    fail "extract: empty command should return 1"
fi

# Command path not confused for file path
if ! extract_target_file "cat" &>/dev/null; then
    pass "extract: bare command returns 1"
else
    fail "extract: bare command should return 1"
fi

# ============================================================================
# get_detection_category() tests
# ============================================================================
echo ""
echo "--- get_detection_category ---"

category=$(get_detection_category "cat file.txt" || echo "none")
if [[ "$category" == "cat_family" ]]; then pass "category: cat => cat_family"; else fail "category: expected cat_family, got $category"; fi

category=$(get_detection_category "awk '{print}' file" || echo "none")
if [[ "$category" == "text_processor" ]]; then pass "category: awk => text_processor"; else fail "category: expected text_processor, got $category"; fi

category=$(get_detection_category "grep pattern file.txt" || echo "none")
if [[ "$category" == "search_tool" ]]; then pass "category: grep => search_tool"; else fail "category: expected search_tool, got $category"; fi

category=$(get_detection_category "source /etc/profile" || echo "none")
if [[ "$category" == "shell_builtin" ]]; then pass "category: source => shell_builtin"; else fail "category: expected shell_builtin, got $category"; fi

category=$(get_detection_category "diff file1 file2" || echo "none")
if [[ "$category" == "diff_tool" ]]; then pass "category: diff => diff_tool"; else fail "category: expected diff_tool, got $category"; fi

category=$(get_detection_category "base64 file" || echo "none")
if [[ "$category" == "encoding_tool" ]]; then pass "category: base64 => encoding_tool"; else fail "category: expected encoding_tool, got $category"; fi

category=$(get_detection_category "zcat file.gz" || echo "none")
if [[ "$category" == "archive_reader" ]]; then pass "category: zcat => archive_reader"; else fail "category: expected archive_reader, got $category"; fi

category=$(get_detection_category "curl file:///etc/passwd" || echo "none")
if [[ "$category" == "network_fetcher" ]]; then pass "category: curl file:// => network_fetcher"; else fail "category: expected network_fetcher, got $category"; fi

category=$(get_detection_category "tee output.log" || echo "none")
if [[ "$category" == "utility_tool" ]]; then pass "category: tee => utility_tool"; else fail "category: expected utility_tool, got $category"; fi

# Note: /bin/cat matches cat_family first (path stripped), not path_evasion
category=$(get_detection_category "/bin/cat file" || echo "none")
if [[ "$category" == "cat_family" ]]; then pass "category: /bin/cat => cat_family (path stripped)"; else fail "category: expected cat_family, got $category"; fi

# env wrapper triggers path_evasion since env is not in any other category
category=$(get_detection_category "env cat file" || echo "none")
if [[ "$category" == "path_evasion" ]]; then pass "category: env cat => path_evasion"; else fail "category: expected path_evasion, got $category"; fi

# No match
if ! get_detection_category "ls -la" &>/dev/null; then
    pass "category: ls => no match (returns 1)"
else
    fail "category: ls should not match any category"
fi

# ============================================================================
# Combined detection (is_file_reading_command) - cross-category
# ============================================================================
echo ""
echo "--- Combined detection (is_file_reading_command) ---"

# Positive: one from each category
if is_file_reading_command "cat /etc/passwd"; then pass "combined: cat"; else fail "combined: cat not detected"; fi
if is_file_reading_command "sed 's/x/y/' file"; then pass "combined: sed"; else fail "combined: sed not detected"; fi
if is_file_reading_command "grep pattern file"; then pass "combined: grep"; else fail "combined: grep not detected"; fi
if is_file_reading_command "source /etc/profile"; then pass "combined: source"; else fail "combined: source not detected"; fi
if is_file_reading_command "diff file1 file2"; then pass "combined: diff"; else fail "combined: diff not detected"; fi
if is_file_reading_command "xxd file.bin"; then pass "combined: xxd"; else fail "combined: xxd not detected"; fi
if is_file_reading_command "zcat file.gz"; then pass "combined: zcat"; else fail "combined: zcat not detected"; fi
if is_file_reading_command "curl file:///etc/passwd"; then pass "combined: curl file://"; else fail "combined: curl file:// not detected"; fi
if is_file_reading_command "dd if=/dev/sda of=/tmp/out"; then pass "combined: dd if="; else fail "combined: dd if= not detected"; fi
if is_file_reading_command "/usr/bin/cat /etc/passwd"; then pass "combined: /usr/bin/cat"; else fail "combined: /usr/bin/cat not detected"; fi

# Negative: non-reading commands
if ! is_file_reading_command "ls -la"; then pass "combined: ls NOT detected"; else fail "combined: ls should not be detected"; fi
if ! is_file_reading_command "echo hello"; then pass "combined: echo NOT detected"; else fail "combined: echo should not be detected"; fi
if ! is_file_reading_command "mkdir /tmp/test"; then pass "combined: mkdir NOT detected"; else fail "combined: mkdir should not be detected"; fi
if ! is_file_reading_command "rm file.txt"; then pass "combined: rm NOT detected"; else fail "combined: rm should not be detected"; fi
if ! is_file_reading_command "chmod 755 file"; then pass "combined: chmod NOT detected"; else fail "combined: chmod should not be detected"; fi
if ! is_file_reading_command ""; then pass "combined: empty string NOT detected"; else fail "combined: empty string should not be detected"; fi

# detect_indirect_read alias
if detect_indirect_read "cat file.txt"; then pass "detect_indirect_read works as alias"; else fail "detect_indirect_read should work as alias"; fi

# ============================================================================
# Alias functions
# ============================================================================
echo ""
echo "--- Alias functions ---"

if is_direct_display_command "cat file"; then pass "alias: is_direct_display_command"; else fail "alias: is_direct_display_command"; fi
if is_text_processor_command "sed 's/x/y/' file"; then pass "alias: is_text_processor_command"; else fail "alias: is_text_processor_command"; fi
if is_binary_encoder_command "xxd file"; then pass "alias: is_binary_encoder_command"; else fail "alias: is_binary_encoder_command"; fi
if is_archive_reader_command "zcat file.gz"; then pass "alias: is_archive_reader_command"; else fail "alias: is_archive_reader_command"; fi
if is_shell_file_reader "source file"; then pass "alias: is_shell_file_reader"; else fail "alias: is_shell_file_reader"; fi
if is_interpreter_file_reader "perl script.pl"; then pass "alias: is_interpreter_file_reader"; else fail "alias: is_interpreter_file_reader"; fi
if is_utility_file_reader "tee output"; then pass "alias: is_utility_file_reader"; else fail "alias: is_utility_file_reader"; fi
if is_path_evasion_attempt "/bin/cat file"; then pass "alias: is_path_evasion_attempt"; else fail "alias: is_path_evasion_attempt"; fi

# ============================================================================
# Edge cases
# ============================================================================
echo ""
echo "--- Edge cases ---"

# Commands with full paths to binaries
if is_cat_family "/usr/local/bin/bat file.txt"; then
    pass "Full path stripped: /usr/local/bin/bat"
else
    fail "Should strip full path for /usr/local/bin/bat"
fi

# Extra whitespace
if is_cat_family "  cat  file.txt  "; then
    pass "Leading whitespace handled"
else
    # This might fail since awk handles leading whitespace
    fail "Leading whitespace not handled"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
