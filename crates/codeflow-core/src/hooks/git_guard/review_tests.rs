//! The review rule (TSK-1348 AC-19), judged through the whole guard.

use super::super::owned_fixture::evaluate;
use super::super::GuardContext;
use crate::hooks::policy::{GitPolicy, PolicyLevel};

fn ctx(policy: &GitPolicy) -> GuardContext<'_> {
    GuardContext {
        policy,
        current_branch: "feat/x",
        integrate_token: false,
        pr_base_lookup: None,
        dir_target_lookup: None,
        alias_lookup: None,
        branch_lookup: None,
        discard_lookup: Some(&|_| Ok(None)),
        root_checkout: None,
    }
}

fn rules(command: &str, policy: &GitPolicy) -> Vec<String> {
    evaluate(command, &ctx(policy))
        .into_iter()
        .map(|violation| violation.rule)
        .collect()
}

fn refused(command: &str) -> bool {
    rules(command, &GitPolicy::default())
        .iter()
        .any(|rule| rule == "git.pr_review_approval")
}

/// The tokens a session may carry: the PR App's user token, the agent App's
/// installation token, and none the guard can see.
const TOKENS: [&str; 3] = [
    "GH_TOKEN=$(codeflow remote pr-token) ",
    "GH_TOKEN=$(codeflow remote agent-token) ",
    "",
];

const APPROVALS: &[&str] = &[
    "gh pr review 12 --approve",
    "gh pr review --approve -b 'looks good'",
    "gh pr review 12 -a",
    "gh pr review 12 -ca",
    "gh pr review https://github.com/o/r/pull/12 --approve=true",
    "gh api -X POST repos/o/r/pulls/12/reviews -f event=APPROVE",
    "gh api repos/{owner}/{repo}/pulls/12/reviews -f body=ok -f event=approve",
    "gh api --method POST /repos/o/r/pulls/12/reviews/9/events -fevent=APPROVE",
    "gh api repos/o/r/pulls/12/reviews --raw-field=event=APPROVE",
    "gh api graphql -f query='mutation { addPullRequestReview(input: {pullRequestId: \"x\", event: APPROVE}) { clientMutationId } }'",
    "gh api graphql -f query='mutation($id: ID!, $e: PullRequestReviewEvent!) { submitPullRequestReview(input: {pullRequestReviewId: $id, event: $e}) { clientMutationId } }' -F e=APPROVE",
];

const DISMISSALS: &[&str] = &[
    "gh api -X PUT repos/o/r/pulls/12/reviews/9/dismissals -f message=stale",
    "gh api graphql -f query='mutation { dismissPullRequestReview(input: {pullRequestReviewId: \"x\", message: \"m\"}) { clientMutationId } }'",
];

const CURL: &[&str] = &[
    "curl -X POST -H 'Authorization: Bearer TOKEN' https://api.github.com/repos/o/r/pulls/12/reviews -d '{\"event\":\"APPROVE\"}'",
    "curl --request POST --data='{\"event\": \"APPROVE\"}' --url https://api.github.com/repos/o/r/pulls/12/reviews",
    "curl -sS https://api.github.com/repos/o/r/pulls/12/reviews/9/events --json '{\"event\":\"APPROVE\"}'",
    "curl -X PUT https://api.github.com/repos/o/r/pulls/12/reviews/9/dismissals -d '{\"message\":\"m\"}'",
    "curl -X POST https://api.github.com/graphql -d '{\"query\":\"mutation { addPullRequestReview(input: {pullRequestId: \\\"x\\\", event: APPROVE}) { clientMutationId } }\"}'",
];

/// AC-19: every parsed approval and dismissal refuses, with the PR token,
/// with the agent App's token and with no token visible, naming the rule and
/// its policy key.
#[test]
fn every_parsed_approval_and_dismissal_refuses_whatever_the_token() {
    for token in TOKENS {
        for command in APPROVALS.iter().chain(DISMISSALS).chain(CURL) {
            let line = format!("{token}{command}");
            let violations = evaluate(&line, &ctx(&GitPolicy::default()));
            let review = violations
                .iter()
                .find(|violation| violation.rule == "git.pr_review_approval")
                .unwrap_or_else(|| panic!("not refused: {line}: {violations:?}"));
            assert_eq!(review.level, PolicyLevel::Block, "{line}");
            assert!(
                review.message.contains("policy key git.pr_review_approval"),
                "{line}: {}",
                review.message
            );
            assert!(
                review.remedy.contains("git.pr_review_approval"),
                "{line}: {}",
                review.remedy
            );
        }
    }
    let curl = format!(
        "H=\"Authorization: Bearer $(codeflow remote pr-token)\"; {}",
        CURL[0]
    );
    assert!(refused(&curl), "{curl}");
}

/// AC-19: a comment or a request-changes review passes, even one whose text
/// mentions approval, and so do reads of reviews.
#[test]
fn comment_and_request_changes_reviews_pass() {
    for command in [
        "gh pr review 12 --comment -b 'I approve of the idea, but see below'",
        "gh pr review 12 --request-changes --body '--approve is not right here'",
        "gh pr review 12 -c -b approve",
        "gh pr review 12 -r -F notes.md",
        "gh api -X POST repos/o/r/pulls/12/reviews -f event=COMMENT -f body='approve later'",
        "gh api repos/o/r/pulls/12/reviews -f event=REQUEST_CHANGES",
        "gh api repos/o/r/pulls/12/reviews",
        "gh api repos/o/r/pulls/12/reviews/9",
        "gh api repos/o/r/pulls/12/comments -f body=APPROVE",
        "gh api graphql -f query='query { viewer { login } }'",
        "curl -X POST https://api.github.com/repos/o/r/pulls/12/reviews -d '{\"event\":\"REQUEST_CHANGES\",\"body\":\"approve?\"}'",
        "curl https://api.github.com/repos/o/r/pulls/12/reviews",
        "gh pr create -t 'feat: approve flow' -b 'Adds the approve step.'",
    ] {
        assert!(!refused(command), "{command}");
    }
}

/// AC-19: a REST review body the guard cannot read may approve, so it
/// refuses.
#[test]
fn an_unreadable_review_body_refuses() {
    for command in [
        "gh api repos/o/r/pulls/12/reviews --input -",
        "gh api repos/o/r/pulls/12/reviews --input /no/such/review.json",
        "gh api repos/o/r/pulls/12/reviews -F event=@/no/such/event",
        "curl https://api.github.com/repos/o/r/pulls/12/reviews -d @-",
        "curl https://api.github.com/repos/o/r/pulls/12/reviews --data-binary @/no/such/body.json",
    ] {
        assert!(refused(command), "{command}");
    }
    // A readable file is judged by its content.
    let dir = tempfile::tempdir().unwrap();
    let approve = dir.path().join("approve.json");
    let comment = dir.path().join("comment.json");
    std::fs::write(&approve, r#"{"event":"APPROVE","body":"ok"}"#).unwrap();
    std::fs::write(&comment, r#"{"event":"COMMENT","body":"I approve"}"#).unwrap();
    let approve = approve.display();
    let comment = comment.display();
    assert!(refused(&format!(
        "gh api repos/o/r/pulls/12/reviews --input {approve}"
    )));
    assert!(refused(&format!(
        "curl https://api.github.com/repos/o/r/pulls/12/reviews -d @{approve}"
    )));
    assert!(!refused(&format!(
        "gh api repos/o/r/pulls/12/reviews --input {comment}"
    )));
}

/// AC-19, review of 8acea4109 finding 1a: `.` and `..` are resolved before
/// the endpoint is judged, as GitHub resolves them, and a writing request
/// whose address the guard cannot settle refuses.
#[test]
fn dot_segments_are_resolved_and_an_unsettled_address_refuses() {
    for command in [
        "gh api -X POST repos/o/r/pulls/12/x/../reviews -f event=APPROVE",
        "gh api repos/o/r/pulls/12/./reviews -f event=APPROVE",
        "gh api -X PUT repos/o/r/pulls/12/reviews/9/x/../dismissals -f message=m",
        "curl -X POST https://api.github.com/repos/o/r/pulls/12/x/../reviews -d '{\"event\":\"APPROVE\"}'",
        "curl -X PUT https://api.github.com/repos/o/r/pulls/12/reviews/9/x/../dismissals -d '{\"message\":\"m\"}'",
        "gh api graphql/x/.. -f query='mutation { dismissPullRequestReview(input: {}) { clientMutationId } }'",
        // Not settled: a percent escape, a `..` above the root, a curl glob.
        "gh api repos/o/r/pulls/12/re%76iews -f event=APPROVE",
        "gh api ../../repos/o/r/pulls/12/reviews -f event=APPROVE",
        "curl https://api.github.com/repos/o/r/pulls/12/{reviews} -d '{\"event\":\"APPROVE\"}'",
        "curl https://api.github.com/repos/o/r/pulls/12/revie[w-w]s -d '{\"event\":\"APPROVE\"}'",
    ] {
        assert!(refused(command), "{command}");
    }
    // Reads stay allowed, and so does a settled path that is not a review.
    for command in [
        "gh api repos/o/r/pulls/12/x/../reviews",
        "gh api repos/o/r/pulls/12/re%76iews",
        "curl https://api.github.com/repos/o/r/pulls/12/{reviews}",
        "gh api -X POST repos/o/r/pulls/12/reviews/../comments -f body=APPROVE",
    ] {
        assert!(!refused(command), "{command}");
    }
}

/// AC-19, review of 14ebf326c finding 1: an owner or a repository named
/// `pulls` does not hide the review endpoint after it.
#[test]
fn an_owner_or_repository_named_pulls_hides_no_review() {
    for path in [
        "repos/pulls/r/pulls/12",
        "repos/o/pulls/pulls/12",
        "repos/pulls/pulls/pulls/12",
    ] {
        for command in [
            format!("gh api -X POST {path}/reviews -f event=APPROVE"),
            format!("gh api -X POST {path}/reviews/9/events -f event=APPROVE"),
            format!("gh api -X PUT {path}/reviews/9/dismissals -f message=m"),
            format!("curl -X POST https://api.github.com/{path}/reviews -d '{{\"event\":\"APPROVE\"}}'"),
            format!("curl -X PUT https://api.github.com/{path}/reviews/9/dismissals -d '{{\"message\":\"m\"}}'"),
        ] {
            assert!(refused(&command), "{command}");
        }
        let comment = format!("gh api -X POST {path}/reviews -f event=COMMENT");
        assert!(!refused(&comment), "{comment}");
    }
    // The sweep of review.rs for first matches: curl splits a
    // `--data-urlencode` value at its first `=` or `@` and a `-F` value at
    // its first `=`, so text after a later `@` is literal, never a file.
    let dir = tempfile::tempdir().unwrap();
    let benign = dir.path().join("benign.txt");
    std::fs::write(&benign, "nothing here").unwrap();
    let benign = benign.display();
    let reviews = "https://api.github.com/repos/o/r/pulls/12/reviews";
    for command in [
        format!("curl {reviews} --data-urlencode 'event=APPROVE@{benign}'"),
        format!("curl {reviews} -F 'event=APPROVE=@{benign}'"),
    ] {
        assert!(refused(&command), "{command}");
    }
    assert!(!refused(&format!(
        "curl {reviews} --data-urlencode 'body=fine@/no/such/file'"
    )));
}

/// AC-19, review of 8acea4109 finding 1b: a GraphQL body the guard cannot
/// read may approve, so it refuses as a REST review body does; a harmless
/// query read from standard input refuses too.
#[test]
fn an_unreadable_graphql_body_refuses() {
    for command in [
        "gh api graphql --input -",
        "gh api graphql --input /no/such/query.json",
        "gh api graphql -F query=@/no/such/query.graphql",
        "gh api graphql -F query=@-",
        "curl -d @- https://api.github.com/graphql",
        "curl https://api.github.com/graphql --data-binary @/no/such/query.json",
        "curl -T - https://api.github.com/graphql",
    ] {
        assert!(refused(command), "{command}");
    }
    // A JSON body is read as JSON, so an escaped name or event still shows.
    let escaped = r#"curl -X POST https://api.github.com/graphql -d '{"query":"mutation { addPullRequestReview(input: {pullRequestId: \"x\", event: APPROVE}) { clientMutationId } }"}'"#;
    assert!(refused(escaped), "{escaped}");
}

/// AC-19, review of 8acea4109 finding 1c and its sibling sweep: a curl
/// command that reads a config file or a variable refuses, since the guard
/// does not read them, and every other curl spelling that carries a URL or a
/// body is judged.
#[test]
fn curl_config_variables_and_other_spellings_are_judged() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("review.curlrc");
    std::fs::write(
        &config,
        "url = \"https://api.github.com/repos/o/r/pulls/12/reviews\"\ndata = \"{\\\"event\\\":\\\"APPROVE\\\"}\"\n",
    )
    .unwrap();
    let config = config.display();
    let approve = dir.path().join("reviews");
    std::fs::write(&approve, r#"{"event":"APPROVE"}"#).unwrap();
    let upload = approve.display();
    for command in [
        format!("curl -K {config}"),
        format!("curl --config {config}"),
        format!("curl -sK {config}"),
        format!("curl --config={config}"),
        "curl -K - https://example.com".to_string(),
        "curl --variable 'e@/no/such/file' --expand-data '{{e}}' https://api.github.com/repos/o/r/pulls/12/reviews".to_string(),
        "curl --expand-url 'https://api.github.com/{{p}}' -d x".to_string(),
        // Attached and clustered short options.
        "curl -sSd '{\"event\":\"APPROVE\"}' https://api.github.com/repos/o/r/pulls/12/reviews".to_string(),
        "curl -XPOST -d'{\"event\":\"APPROVE\"}' https://api.github.com/repos/o/r/pulls/12/reviews".to_string(),
        "curl -sXPUT https://api.github.com/repos/o/r/pulls/12/reviews/9/dismissals -d '{\"message\":\"m\"}'".to_string(),
        // An upload is a body, and names the URL's last segment when it ends in `/`.
        format!("curl -X POST -T {upload} https://api.github.com/repos/o/r/pulls/12/reviews"),
        format!("curl -X POST --upload-file {upload} https://api.github.com/repos/o/r/pulls/12/"),
        // The request target replaces the URL's path.
        "curl --request-target /repos/o/r/pulls/12/reviews https://api.github.com/ -d '{\"event\":\"APPROVE\"}'".to_string(),
        "curl --url https://api.github.com/repos/o/r/pulls/12/reviews --json '{\"event\":\"APPROVE\"}'".to_string(),
        format!("curl https://api.github.com/repos/o/r/pulls/12/reviews --data-urlencode body@{upload}"),
        "curl https://api.github.com/repos/o/r/pulls/12/reviews -F 'event=APPROVE'".to_string(),
        "curl https://api.github.com/repos/o/r/pulls/12/reviews --form-string 'event=APPROVE'".to_string(),
        // `--get`, the long spelling of `-G`, moves the data to the query.
        "curl --get https://api.github.com/repos/o/r/pulls/12/reviews -X POST --data event=APPROVE".to_string(),
    ] {
        assert!(refused(&command), "{command}");
    }
    // `gh api -f` never reads a file: `@` is literal text, judged as it is.
    assert!(refused(
        "gh api repos/o/r/pulls/12/reviews -f event=APPROVE -f body=@notes.md"
    ));
    assert!(!refused(
        "gh api repos/o/r/pulls/12/reviews -f event=COMMENT -f body=@/no/such/file"
    ));
    // Harmless curl spellings pass.
    for command in [
        "curl -sSLo out.html https://example.com/",
        "curl -H 'Accept: application/json' https://api.github.com/repos/o/r/pulls/12/reviews",
        "curl -T notes.txt https://uploads.example.com/",
        "curl -X POST https://api.github.com/repos/o/r/pulls/12/reviews -d '{\"event\":\"COMMENT\"}'",
    ] {
        assert!(!refused(command), "{command}");
    }
}

/// AC-19: the rule is on when the key is absent, and a project may turn it
/// off.
#[test]
fn the_rule_is_on_by_default_and_a_project_can_turn_it_off() {
    let absent: GitPolicy = serde_json::from_str("{}").unwrap();
    assert_eq!(absent.pr_review_approval, PolicyLevel::Block);
    let off = GitPolicy {
        pr_review_approval: PolicyLevel::Off,
        ..GitPolicy::default()
    };
    for command in APPROVALS.iter().chain(DISMISSALS).chain(CURL) {
        assert!(
            !rules(command, &off)
                .iter()
                .any(|rule| rule == "git.pr_review_approval"),
            "{command}"
        );
    }
    let warn = GitPolicy {
        pr_review_approval: PolicyLevel::Warn,
        ..GitPolicy::default()
    };
    let violations = evaluate(APPROVALS[0], &ctx(&warn));
    assert_eq!(violations[0].level, PolicyLevel::Warn);
}
