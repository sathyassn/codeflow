//! Secret detection over staged content (charter §6.3 layer 2).
//!
//! Pattern-based scan of *added* lines in a unified diff, plus the
//! staged-.env check. Tuned for the pre-commit hot path: a fixed set of
//! high-signal credential shapes with a placeholder filter to keep false
//! positives out of the developer's way.

use std::sync::OnceLock;

use regex::Regex;

/// A secret found in staged content.
#[derive(Debug, Clone)]
pub struct SecretHit {
    /// File the secret was added to.
    pub file: String,
    /// 1-based line number in the new file (0 when unknown).
    pub line: u32,
    /// Human-readable name of the matched pattern.
    pub pattern: &'static str,
}

fn secret_res() -> &'static [(Regex, &'static str)] {
    static RES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    RES.get_or_init(|| {
        vec![
            (
                Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY( BLOCK)?-----").expect("valid"),
                "private key block",
            ),
            (
                Regex::new(r"\bAKIA[0-9A-Z]{16}\b").expect("valid"),
                "AWS access key id",
            ),
            (
                Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{36,255}\b").expect("valid"),
                "GitHub token",
            ),
            (
                Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{36,255}\b").expect("valid"),
                "GitHub fine-grained token",
            ),
            (
                Regex::new(r"\bxox[baprs]-[0-9A-Za-z-]{10,}\b").expect("valid"),
                "Slack token",
            ),
            (
                Regex::new(r"\bsk-(ant-)?[A-Za-z0-9_-]{20,}\b").expect("valid"),
                "API secret key (sk-…)",
            ),
            (
                Regex::new(r"\bAIza[0-9A-Za-z_-]{35}\b").expect("valid"),
                "Google API key",
            ),
            (
                Regex::new(
                    r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b",
                )
                .expect("valid"),
                "JSON Web Token",
            ),
            (
                Regex::new(
                    r#"(?i)\b(api[_-]?key|secret[_-]?key|client[_-]?secret|auth[_-]?token|access[_-]?token|password)\b\s*[:=]\s*["'][^"']{12,}["']"#,
                )
                .expect("valid"),
                "credential assignment",
            ),
        ]
    })
}

/// A value that looks like docs/templates, not a live credential. Applied ONLY
/// to the assigned value (the quoted string), never the whole line — a comment
/// like `# example account` must not suppress a real secret (codex pre-flip
/// review: secret-scan bypass).
///
/// Template-interpolation markers (`<`, `${`, `$(`) are never a raw secret, so
/// they count anywhere. Placeholder *words* count only as a leading marker, so a
/// live value that merely contains "example"/"xxxx" mid-string is NOT suppressed
/// (codex round-2: over-broad substring match).
fn is_placeholder_value(value: &str) -> bool {
    let lower = value.to_lowercase();
    if ["<", "${", "$("].iter().any(|m| lower.contains(m)) {
        return true;
    }
    let core = lower.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    [
        "your-", "your_", "changeme", "placeholder", "example", "xxxx", "redacted", "dummy",
        "sample", "insert", "todo", "fixme",
    ]
    .iter()
    .any(|p| core.starts_with(p))
}

/// Every quoted value assigned in a `key = "value"` credential line (a line can
/// carry more than one, e.g. flow-style JSON/YAML).
fn assigned_values(line: &str) -> Vec<&str> {
    static VALUE_RE: OnceLock<Regex> = OnceLock::new();
    let re = VALUE_RE.get_or_init(|| {
        Regex::new(
            r#"(?i)\b(?:api[_-]?key|secret[_-]?key|client[_-]?secret|auth[_-]?token|access[_-]?token|password)\b\s*[:=]\s*["']([^"']{12,})["']"#,
        )
        .expect("valid")
    });
    re.captures_iter(line)
        .filter_map(|c| c.get(1).map(|m| m.as_str()))
        .collect()
}

/// Scan a single content line for secrets. Returns the pattern name on a hit.
///
/// Limitation: the generic credential-assignment detector requires a *quoted*
/// value, so an unquoted `PASSWORD=liveSecret123` in a non-`.env` file is not
/// matched here — that would need an entropy heuristic the low-false-positive
/// hot path deliberately avoids. Specific token shapes (AKIA…, ghp_…, sk-…) are
/// caught regardless of quoting, and `.env` files are blocked wholesale by
/// [`is_env_file`].
#[must_use]
pub fn scan_line(line: &str) -> Option<&'static str> {
    let hit = secret_res()
        .iter()
        .find(|(re, _)| re.is_match(line))
        .map(|(_, name)| *name)?;
    // The generic assignment defers to the placeholder filter — but only on the
    // assigned VALUE(s), and it suppresses only when EVERY value is a
    // placeholder, so a placeholder assignment cannot shield a live one that
    // follows on the same line. Specific token shapes (AKIA…, ghp_…) always count.
    if hit == "credential assignment" {
        let values = assigned_values(line);
        if !values.is_empty() && values.iter().all(|v| is_placeholder_value(v)) {
            return None;
        }
    }
    Some(hit)
}

/// Scan unified-diff text (e.g. `git diff --cached -U0`) and report secrets
/// on added lines, with file and new-file line numbers.
#[must_use]
#[allow(clippy::missing_panics_doc)] // static regex compile cannot fail
pub fn scan_diff(diff: &str) -> Vec<SecretHit> {
    static HUNK_RE: OnceLock<Regex> = OnceLock::new();
    let hunk_re =
        HUNK_RE.get_or_init(|| Regex::new(r"^@@ -\d+(?:,\d+)? \+(\d+)").expect("valid"));

    let mut hits = Vec::new();
    let mut file = String::new();
    let mut new_line: u32 = 0;

    for raw in diff.lines() {
        if let Some(path) = raw.strip_prefix("+++ ") {
            file = path
                .strip_prefix("b/")
                .unwrap_or(path)
                .trim()
                .to_string();
            continue;
        }
        if let Some(caps) = hunk_re.captures(raw) {
            new_line = caps[1].parse().unwrap_or(0);
            continue;
        }
        if let Some(content) = raw.strip_prefix('+') {
            if let Some(pattern) = scan_line(content) {
                hits.push(SecretHit {
                    file: file.clone(),
                    line: new_line,
                    pattern,
                });
            }
            new_line = new_line.saturating_add(1);
        } else if !raw.starts_with('-') && !raw.starts_with('\\') {
            // Context line advances the new-file counter too.
            new_line = new_line.saturating_add(1);
        }
    }
    hits
}

/// `true` when a staged path is a dotenv-style secrets file
/// (charter §6.1 `secret_scan`: staged-.env block). Documentation variants
/// (`.env.example`, `.env.sample`, `.env.template`) are exempt.
#[must_use]
pub fn is_env_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let is_env = name == ".env"
        || name.starts_with(".env.")
        || std::path::Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("env"));
    if !is_env {
        return false;
    }
    !["example", "sample", "template", "dist"]
        .iter()
        .any(|suffix| name.ends_with(&format!(".{suffix}")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_comment_cannot_suppress_secret() {
        // codex pre-flip review: a placeholder word in a COMMENT must not
        // suppress a real credential value.
        let live = r#"api_key = "a1B2c3D4e5F6g7H8i9J0kL" # example account"#;
        assert!(scan_line(live).is_some(), "comment must not suppress");
        // a genuine placeholder VALUE is still allowed
        let ph = r#"api_key = "your-key-goes-here-xxxx""#;
        assert!(scan_line(ph).is_none(), "placeholder value allowed");
        let ex = r#"password = "changeme-please-now""#;
        assert!(scan_line(ex).is_none());
    }

    use super::*;

    // codex round-2: a placeholder assignment must not shield a live credential
    // that follows on the same line (flow-style JSON/YAML, minified config).
    #[test]
    fn test_placeholder_does_not_shield_later_live_secret() {
        // Live value built at runtime so the source carries no literal secret
        // (mirrors the runtime-built values above). `key="val"` syntax, two on
        // one line (shell `export`): the placeholder must not shield the live one.
        let secret = format!("R3al{}", "LiveSecretValue99");
        let line = format!(r#"export api_key="your-key-here-xx" password="{secret}""#);
        assert!(scan_line(&line).is_some(), "later live secret must be caught");
        // both placeholders → still suppressed
        let all_ph = r#"api_key = "your-key-here-xx" password = "changeme-now-please""#;
        assert!(scan_line(all_ph).is_none(), "all-placeholder line stays clean");
    }

    // codex round-2: a live value that merely CONTAINS a placeholder word
    // mid-string must still be flagged (substring match was over-broad). Values
    // built at runtime so the source carries no literal credential assignment.
    #[test]
    fn test_midvalue_placeholder_word_still_flagged() {
        let mid_example = format!("a1{}9Z8yLiveKey24", "examp".to_string() + "le");
        let mid_xxxx = format!("p4ss{}liveSecret77", "XX".to_string() + "XX");
        let mid_dummy = format!("real{}LookingButLive9", "dum".to_string() + "my");
        for val in [mid_example, mid_xxxx, mid_dummy] {
            let line = format!(r#"api_key = "{val}""#);
            assert!(scan_line(&line).is_some(), "must flag live value: {line}");
        }
    }

    #[test]
    fn test_scan_line_aws_key() {
        assert_eq!(
            scan_line("aws_key = AKIAIOSFODNN7EXAMPLF"),
            Some("AWS access key id")
        );
    }

    #[test]
    fn test_scan_line_private_key() {
        assert_eq!(
            scan_line("-----BEGIN RSA PRIVATE KEY-----"),
            Some("private key block")
        );
        assert_eq!(
            scan_line("-----BEGIN OPENSSH PRIVATE KEY-----"),
            Some("private key block")
        );
    }

    #[test]
    fn test_scan_line_github_tokens() {
        assert_eq!(
            scan_line("token: ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            Some("GitHub token")
        );
    }

    // The secret values below are BUILT at runtime, so the source lines carry no
    // matching literal for the pre-commit secret scan (which scans staged source);
    // `scan_line` still receives the full value and validates the detector.
    #[test]
    fn test_scan_line_github_fine_grained_token() {
        // `github_pat_` + a 38-char body → the fine-grained-token detector.
        let line = format!("token: github_pat_{}", "A".repeat(38));
        assert_eq!(scan_line(&line), Some("GitHub fine-grained token"));
    }

    #[test]
    fn test_scan_line_openai_keys() {
        let sk = format!("key = sk-{}", "A".repeat(24));
        assert_eq!(scan_line(&sk), Some("API secret key (sk-…)"));
        let sk_ant = format!("key = sk-ant-{}", "A".repeat(24));
        assert_eq!(scan_line(&sk_ant), Some("API secret key (sk-…)"));
    }

    #[test]
    fn test_scan_line_google_api_key() {
        // `AIza` + exactly 35 trailing chars.
        let line = format!("AIza{}", "A".repeat(35));
        assert_eq!(scan_line(&line), Some("Google API key"));
    }

    #[test]
    fn test_scan_line_jwt() {
        // Three base64url segments joined at runtime; no segment alone is a JWT.
        let jwt = format!(
            "{}.{}.{}",
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9", "eyJzdWIiOiIxMjM0NTY3ODkwIn0", "dummyFakeSignature0123"
        );
        assert_eq!(scan_line(&jwt), Some("JSON Web Token"));
    }

    #[test]
    fn test_scan_line_benign_no_false_positive() {
        // Names the token families but carries no real credential; guards
        // against a future over-broad edit to any detector.
        assert_eq!(
            scan_line("Docs mention sk- prefixes, AIza keys, and JWT eyJ headers."),
            None
        );
    }

    #[test]
    fn test_scan_line_generic_assignment() {
        assert_eq!(
            scan_line(r#"api_key = "sUp3rS3cretValu3-9000""#),
            Some("credential assignment")
        );
    }

    #[test]
    fn test_scan_line_placeholder_filtered() {
        assert_eq!(scan_line(r#"api_key = "your-api-key-here-now""#), None);
        assert_eq!(scan_line(r#"password = "<insert-password-here>""#), None);
        assert_eq!(scan_line(r#"secret_key = "${SECRET_FROM_ENV_VAR}""#), None);
    }

    #[test]
    fn test_scan_line_clean_code() {
        assert_eq!(scan_line("let api_key_name = config.lookup(key);"), None);
        assert_eq!(scan_line("fn parse_password_policy() {}"), None);
    }

    #[test]
    fn test_scan_diff_reports_file_and_line() {
        let diff = "diff --git a/src/config.rs b/src/config.rs\n\
                    --- a/src/config.rs\n\
                    +++ b/src/config.rs\n\
                    @@ -0,0 +10,2 @@\n\
                    +let x = 1;\n\
                    +let key = \"AKIAIOSFODNN7EXAMPLF\";\n";
        let hits = scan_diff(diff);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "src/config.rs");
        assert_eq!(hits[0].line, 11);
        assert_eq!(hits[0].pattern, "AWS access key id");
    }

    #[test]
    fn test_scan_diff_ignores_removed_lines() {
        let diff = "+++ b/a.txt\n@@ -1,1 +1,1 @@\n-AKIAIOSFODNN7EXAMPLF\n+clean line\n";
        assert!(scan_diff(diff).is_empty());
    }

    #[test]
    fn test_scan_diff_multiple_files() {
        let diff = "+++ b/a.txt\n@@ -0,0 +1 @@\n+fine\n\
                    +++ b/b.txt\n@@ -0,0 +1 @@\n+xoxb-123456789012-abcdef\n";
        let hits = scan_diff(diff);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "b.txt");
        assert_eq!(hits[0].pattern, "Slack token");
    }

    #[test]
    fn test_is_env_file() {
        assert!(is_env_file(".env"));
        assert!(is_env_file(".env.production"));
        assert!(is_env_file("config/.env.local"));
        assert!(is_env_file("deploy/prod.env"));
        assert!(!is_env_file(".env.example"));
        assert!(!is_env_file(".env.sample"));
        assert!(!is_env_file(".env.template"));
        assert!(!is_env_file("src/environment.rs"));
        assert!(!is_env_file("envoy.yaml"));
    }
}
