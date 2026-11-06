//! Where the authority App's key is stored (TSK-1347 AC-9).
//!
//! The key signs the `authority:` checks the rulesets pin to the App, so it
//! stays out of head-branch code only while the `codeflow-authority`
//! environment alone holds it. An Actions secret of the same name at
//! repository level, shared from the organization, or in another
//! environment reaches workflows outside the authority job, which could
//! then post `authority:` checks under the App's id. This fails on each
//! such secret, and notes, never passes, each list it cannot read.
//!
//! Secret names are all doctor reads; a list never carries a value. The
//! key stored under another name is beyond what doctor can see.

use serde_json::Value;

use super::{Perimeter, AUTHORITY_ENVIRONMENT};
use crate::remote::POLICY_WORKFLOW;

/// The secret that holds the authority App's private key.
const AUTHORITY_KEY_SECRET: &str = "CODEFLOW_AUTHORITY_APP_KEY";

/// Judge every secret list besides the authority environment's own.
pub(super) fn judge(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    nwo: &str,
    repo: &Value,
    out: &mut Perimeter,
) {
    let mut lists = vec![(
        format!("repos/{nwo}/actions/secrets"),
        format!("the repository {nwo}"),
    )];
    // The organization's secrets that reach this repository; the endpoint
    // refuses a repository a user owns.
    if repo.pointer("/owner/type").and_then(Value::as_str) == Some("Organization") {
        lists.push((
            format!("repos/{nwo}/actions/organization-secrets"),
            format!("the organization (shared with {nwo})"),
        ));
    }
    match names(gh, &format!("repos/{nwo}/environments"), "environments") {
        Ok(environments) => lists.extend(
            environments
                .iter()
                .filter(|name| name.as_str() != AUTHORITY_ENVIRONMENT)
                .map(|name| {
                    (
                        format!("repos/{nwo}/environments/{}/secrets", segment(name)),
                        format!("the environment {name}"),
                    )
                }),
        ),
        Err(why) => out.unread.push(format!(
            "the environments of {nwo} are not readable ({why}), so doctor cannot confirm that no other environment holds {AUTHORITY_KEY_SECRET}"
        )),
    }
    for (url, place) in lists {
        match names(gh, &url, "secrets") {
            Ok(secrets) => {
                if secrets
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(AUTHORITY_KEY_SECRET))
                {
                    out.fails.push(format!(
                        "an Actions secret named {AUTHORITY_KEY_SECRET} is set in {place}, where workflows other than the authority job in {POLICY_WORKFLOW} can read it; delete it there and keep the key only in the environment {AUTHORITY_ENVIRONMENT}"
                    ));
                }
            }
            Err(why) => out.unread.push(format!(
                "the Actions secrets of {place} are not readable ({why})"
            )),
        }
    }
}

/// The `name` of every row of a paged list, or why not. A row without a
/// name is not a list doctor can judge.
fn names(
    gh: &dyn Fn(&[&str]) -> Result<String, String>,
    url: &str,
    key: &str,
) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for row in super::paged(gh, url, key)? {
        let Some(name) = row["name"].as_str() else {
            return Err(format!("a row of {key} has no name"));
        };
        names.push(name.to_string());
    }
    Ok(names)
}

/// `name` as one path segment: every byte outside the unreserved set is
/// percent-encoded.
fn segment(name: &str) -> String {
    name.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::segment;

    #[test]
    fn an_environment_name_is_one_path_segment() {
        assert_eq!(segment("deploy"), "deploy");
        assert_eq!(segment("prod/eu west"), "prod%2Feu%20west");
        assert_eq!(segment("é"), "%C3%A9");
    }
}
