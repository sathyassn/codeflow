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

/// Values that look like docs/templates, not live credentials.
fn is_placeholder(line: &str) -> bool {
    let lower = line.to_lowercase();
    ["example", "placeholder", "changeme", "your-", "your_", "xxxx", "<", "${", "$("]
        .iter()
        .any(|p| lower.contains(p))
}

/// Scan a single content line for secrets. Returns the pattern name on a hit.
#[must_use]
pub fn scan_line(line: &str) -> Option<&'static str> {
    let hit = secret_res()
        .iter()
        .find(|(re, _)| re.is_match(line))
        .map(|(_, name)| *name)?;
    // The generic assignment pattern defers to the placeholder filter;
    // specific token shapes (AKIA…, ghp_…) always count.
    if hit == "credential assignment" && is_placeholder(line) {
        return None;
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
    use super::*;

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
