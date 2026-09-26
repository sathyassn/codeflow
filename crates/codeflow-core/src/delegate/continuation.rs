//! Task-notification continuations of an accepted delegate turn.
//!
//! Claude Code runs a backgrounded tool call (a Workflow, a background Bash
//! command) past the end of the turn that launched it. When the task ends,
//! Claude Code submits a notice as a new `UserPromptSubmit` so the model can
//! act on the result. That notice is not an armed prompt, so it is admitted
//! only as a continuation of the turn that launched the task, and only when
//! the session's own transcript shows that turn making the tool call that
//! returned the task id.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::DelegateError;

/// Largest session transcript read to verify a launch.
const MAX_TRANSCRIPT_BYTES: u64 = 512 * 1024 * 1024;

const OPEN: &str = "<task-notification>\n";
const CLOSE: &str = "\n</task-notification>";

/// The identifiers a task notice names in its first two lines.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct TaskNotice<'a> {
    pub task_id: &'a str,
    pub tool_use_id: &'a str,
}

/// Parse a prompt whose entire body is exactly one Claude Code task notice.
///
/// The accepted grammar is `<task-notification>` LF, `<task-id>ID</task-id>`
/// LF, `<tool-use-id>toolu_ID</tool-use-id>` LF, any further lines, then LF
/// and `</task-notification>` with nothing after it. The task id is 1 to 64
/// lowercase ASCII letters or digits; the tool use id is `toolu_` and 1 to
/// 128 ASCII letters or digits. Any other text around the envelope, a second
/// envelope, or either tag appearing again inside it does not parse.
pub(super) fn parse_task_notice(prompt: &str) -> Option<TaskNotice<'_>> {
    let body = prompt.strip_prefix(OPEN)?.strip_suffix(CLOSE)?;
    if body.contains("<task-notification>") || body.contains("</task-notification>") {
        return None;
    }
    let mut lines = body.split('\n');
    let task_id = lines
        .next()?
        .strip_prefix("<task-id>")?
        .strip_suffix("</task-id>")?;
    let tool_use_id = lines
        .next()?
        .strip_prefix("<tool-use-id>")?
        .strip_suffix("</tool-use-id>")?;
    let task_ok = (1..=64).contains(&task_id.len())
        && task_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    let tool_ok = tool_use_id.strip_prefix("toolu_").is_some_and(|rest| {
        (1..=128).contains(&rest.len()) && rest.bytes().all(|byte| byte.is_ascii_alphanumeric())
    });
    (task_ok && tool_ok).then_some(TaskNotice {
        task_id,
        tool_use_id,
    })
}

/// What the session transcript shows about one task notice.
#[derive(Debug)]
pub(super) struct TranscriptView {
    /// The `promptId` of the turn whose tool call returned the task id.
    pub launched_by: String,
    /// The first transcript line on which each `promptId` appears.
    pub prompt_order: HashMap<String, usize>,
}

/// Find the tool call that launched the noticed task in the session
/// transcript Claude Code named in the hook payload.
///
/// The transcript must be a regular file named `<session_id>.jsonl`. The
/// launch is the `user` entry holding the `tool_result` for the notice's tool
/// use id whose recorded tool result names the task id, as `taskId` (a
/// Workflow) or `backgroundTaskId` (a background Bash command). Exactly one
/// launching prompt must be found.
///
/// # Errors
///
/// Returns an invalid-input error when the transcript is missing, misnamed,
/// not a regular file, too large, unreadable, or shows no single launch.
pub(super) fn read_launch(
    transcript: &Path,
    session_id: &str,
    notice: &TaskNotice<'_>,
) -> Result<TranscriptView, DelegateError> {
    if !transcript.is_absolute()
        || transcript.file_name().and_then(|name| name.to_str())
            != Some(format!("{session_id}.jsonl").as_str())
    {
        return Err(DelegateError::invalid(
            "task notice transcript_path is not this session's transcript",
        ));
    }
    let metadata = std::fs::symlink_metadata(transcript).map_err(|error| {
        DelegateError::invalid(format!("cannot inspect the session transcript: {error}"))
    })?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_TRANSCRIPT_BYTES {
        return Err(DelegateError::invalid(
            "the session transcript is not a bounded regular file",
        ));
    }
    let file = std::fs::File::open(transcript).map_err(|error| {
        DelegateError::invalid(format!("cannot open the session transcript: {error}"))
    })?;
    let mut prompt_order = HashMap::new();
    let mut launched_by: Option<String> = None;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|error| {
            DelegateError::invalid(format!("cannot read the session transcript: {error}"))
        })?;
        // A line still being written is not evidence of anything.
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if entry["type"] != "user" {
            continue;
        }
        if entry
            .get("sessionId")
            .is_some_and(|value| value != session_id)
        {
            continue;
        }
        let Some(prompt_id) = entry["promptId"].as_str() else {
            continue;
        };
        prompt_order.entry(prompt_id.to_string()).or_insert(index);
        if launches(&entry, notice) {
            match &launched_by {
                Some(existing) if existing != prompt_id => {
                    return Err(DelegateError::invalid(
                        "the session transcript shows the task launched by more than one prompt",
                    ));
                }
                _ => launched_by = Some(prompt_id.to_string()),
            }
        }
    }
    let launched_by = launched_by.ok_or_else(|| {
        DelegateError::invalid(
            "the session transcript shows no tool call that launched the noticed task",
        )
    })?;
    Ok(TranscriptView {
        launched_by,
        prompt_order,
    })
}

fn launches(entry: &serde_json::Value, notice: &TaskNotice<'_>) -> bool {
    let answers_call = entry["message"]["content"]
        .as_array()
        .is_some_and(|blocks| {
            blocks.iter().any(|block| {
                block["type"] == "tool_result" && block["tool_use_id"] == notice.tool_use_id
            })
        });
    let result = &entry["toolUseResult"];
    answers_call
        && (result["taskId"] == notice.task_id || result["backgroundTaskId"] == notice.task_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKFLOW: &str = "<task-notification>\n<task-id>wkj4tsqcd</task-id>\n<tool-use-id>toolu_01THy9eKYysgjuWhj9CBoYZF</tool-use-id>\n<status>completed</status>\n</task-notification>";

    #[test]
    fn parses_exactly_one_envelope() {
        assert_eq!(
            parse_task_notice(WORKFLOW),
            Some(TaskNotice {
                task_id: "wkj4tsqcd",
                tool_use_id: "toolu_01THy9eKYysgjuWhj9CBoYZF",
            })
        );
    }

    #[test]
    fn rejects_near_miss_envelopes() {
        let second = WORKFLOW.replace("wkj4tsqcd", "b64do3c7c");
        for prompt in [
            format!("x{WORKFLOW}"),
            format!("{WORKFLOW}\n"),
            format!("{WORKFLOW}x"),
            format!("\n\n{WORKFLOW}"),
            format!("{WORKFLOW}\n{second}"),
            WORKFLOW.replace("<status>", "<task-notification>\n<status>"),
            WORKFLOW.replace('\n', "\r\n"),
            WORKFLOW.replace("wkj4tsqcd", "WKJ4TSQCD"),
            WORKFLOW.replace("wkj4tsqcd", ""),
            WORKFLOW.replace("wkj4tsqcd", "wkj/tsqcd"),
            WORKFLOW.replace("wkj4tsqcd", &"a".repeat(65)),
            WORKFLOW.replace("toolu_01THy9eKYysgjuWhj9CBoYZF", "call_01THy9"),
            WORKFLOW.replace("toolu_01THy9eKYysgjuWhj9CBoYZF", "toolu_"),
            WORKFLOW.replace("<task-id>", "<task-id> "),
            WORKFLOW.replace(
                "<task-id>wkj4tsqcd</task-id>\n<tool-use-id>toolu_01THy9eKYysgjuWhj9CBoYZF</tool-use-id>",
                "<tool-use-id>toolu_01THy9eKYysgjuWhj9CBoYZF</tool-use-id>\n<task-id>wkj4tsqcd</task-id>",
            ),
        ] {
            assert_eq!(parse_task_notice(&prompt), None, "{prompt:?}");
        }
    }
}
