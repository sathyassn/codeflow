//! `codeflow feedback new | status | list`: operator feedback items, one
//! file each under `project-management/feedback/`, their ids issued by the
//! shared registry (issue 75). The rules live in
//! `codeflow_core::feedback`; this module parses arguments and prints.

use std::path::Path;

use clap::{Args, Subcommand, ValueEnum};
use codeflow_core::feedback::{self, Item, StatusChange};
use codeflow_core::ids::Kind;
use codeflow_core::scaffold::state::{FeedbackConfig, ProjectState};
use codeflow_core::scaffold::AssetSource;
use codeflow_core::workgraph::durable_work_tracking_enabled;

use super::new::Issuer;
use crate::embedded::EmbeddedAssets;

/// Arguments for `codeflow feedback`.
#[derive(Debug, Args)]
pub struct FeedbackArgs {
    #[command(subcommand)]
    pub command: FeedbackCommand,
}

/// Feedback subcommands.
#[derive(Debug, Subcommand)]
pub enum FeedbackCommand {
    /// Record a piece of operator feedback: allocate the next FB-NNN and
    /// write it from the template as `received`. Record one for feedback
    /// that sets a standing rule, declines or reorders planned work, or spans
    /// several units; an ordinary request stays with its unit.
    New {
        /// One of the project's `[feedback] topics` (.codeflow/project.toml).
        #[arg(long)]
        topic: String,
        /// Where the feedback was given.
        #[arg(long, value_enum)]
        source: SourceArg,
        /// A one-line summary, the item's title. Paste the operator's words
        /// into its Verbatim section afterwards.
        summary: String,
    },
    /// Move an item by the transition table: received to placed (`--in`),
    /// placed to closed (`--evidence`), an open item to declined
    /// (`--confirmed-by operator --reason`), and received, placed or closed
    /// to superseded (`--by`).
    Status {
        /// Feedback id (FB-NNN).
        id: String,
        /// The new status.
        status: FeedbackStatusArg,
        /// Where it is placed: a TSK-NNN or EPC-NNN record, or a repository
        /// path; repeat for several.
        #[arg(long = "in", value_name = "TSK-NNN|EPC-NNN|PATH")]
        placements: Vec<String>,
        /// What shows it was acted on: a pull request, commit or rule line.
        #[arg(long)]
        evidence: Option<String>,
        /// Who confirmed a decline; only the operator declines feedback.
        #[arg(long, value_enum)]
        confirmed_by: Option<ConfirmedByArg>,
        /// Why it was declined.
        #[arg(long)]
        reason: Option<String>,
        /// The item that supersedes this one.
        #[arg(long, value_name = "FB-NNN")]
        by: Option<String>,
    },
    /// List the items grouped by topic, open items first.
    List {
        /// Only items still received or placed.
        #[arg(long)]
        open: bool,
        /// Only items of this topic.
        #[arg(long)]
        topic: Option<String>,
        /// Print the items as JSON, with every field.
        #[arg(long, conflicts_with = "write")]
        json: bool,
        /// Write project-management/feedback/INDEX.md from every item. A
        /// written index is optional; `validate --docs` warns when it goes
        /// stale.
        #[arg(long, conflicts_with_all = ["open", "topic"])]
        write: bool,
    },
}

/// Where a piece of feedback was given.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SourceArg {
    Chat,
    Pr,
    Review,
    Issue,
}

impl SourceArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Pr => "pr",
            Self::Review => "review",
            Self::Issue => "issue",
        }
    }
}

/// Statuses `feedback status` takes; the table decides which moves are legal.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum FeedbackStatusArg {
    Received,
    Placed,
    Closed,
    Declined,
    Superseded,
}

impl FeedbackStatusArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::Placed => "placed",
            Self::Closed => "closed",
            Self::Declined => "declined",
            Self::Superseded => "superseded",
        }
    }
}

/// Who may confirm a decline.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ConfirmedByArg {
    Operator,
}

/// Run `codeflow feedback`.
pub fn run(args: &FeedbackArgs) -> i32 {
    let root = super::repo_root();
    match &args.command {
        FeedbackCommand::New {
            topic,
            source,
            summary,
        } => new(&root, topic, source.as_str(), summary),
        FeedbackCommand::Status {
            id,
            status,
            placements,
            evidence,
            confirmed_by,
            reason,
            by,
        } => {
            let change = StatusChange {
                target: status.as_str().to_string(),
                placements: placements.clone(),
                evidence: evidence.clone(),
                confirmed_by: confirmed_by.map(|_| "operator".to_string()),
                reason: reason.clone(),
                by: by.clone(),
            };
            match feedback::set_status(&root, id, &change) {
                Ok(outcome) => {
                    println!(
                        "{id}  {} -> {}  {}",
                        outcome.from,
                        outcome.to,
                        outcome.path.display()
                    );
                    0
                }
                Err(error) => {
                    eprintln!("error: {error}");
                    1
                }
            }
        }
        FeedbackCommand::List {
            open,
            topic,
            json,
            write,
        } => list(&root, *open, topic.as_deref(), *json, *write),
    }
}

/// The reason `feedback new` refuses at this project's tier, or `None`
/// when the project tracks durable work.
fn tier_refusal(root: &Path) -> Option<String> {
    match durable_work_tracking_enabled(root) {
        Ok(true) => None,
        Ok(false) => {
            let tier = ProjectState::load(root).map_or_else(
                |_| "unscaffolded".to_string(),
                |state| state.tier.as_str().to_string(),
            );
            Some(format!(
                "operator feedback records live in project-management/, which the full tier installs; at the {tier} tier, feedback stays with its unit in the harness's task tools, and nothing was written"
            ))
        }
        Err(error) => Some(codeflow_core::workgraph::work_start::tracking_state_message(error)),
    }
}

/// The project's `project-management/templates/feedback.md` when usable,
/// else the embedded template, with a warning naming why.
fn template(root: &Path) -> Option<String> {
    let embedded = EmbeddedAssets
        .read("base/pm/feedback.md.tmpl")
        .and_then(|bytes| String::from_utf8(bytes).ok())?;
    match std::fs::read_to_string(root.join(feedback::PROJECT_TEMPLATE)) {
        Ok(own) => match feedback::check_template(&own) {
            Ok(()) => Some(own),
            Err(reason) => {
                eprintln!(
                    "warning: {} is not usable ({reason}); the shipped template is used",
                    feedback::PROJECT_TEMPLATE
                );
                Some(embedded)
            }
        },
        Err(_) => Some(embedded),
    }
}

fn new(root: &Path, topic: &str, source: &str, summary: &str) -> i32 {
    if let Some(refusal) = tier_refusal(root) {
        eprintln!("error: {refusal}");
        return 1;
    }
    let topics = match feedback::topics(root) {
        Ok(topics) => topics,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    if let Some(refusal) = feedback::topic_refusal(topic, &topics) {
        eprintln!("error: {refusal}");
        return 1;
    }
    let Some(template) = template(root) else {
        eprintln!("error: feedback template unavailable");
        return 1;
    };
    match FeedbackConfig::write_defaults(root) {
        Ok((_, true)) => eprintln!(
            "feedback: wrote the default topics to [feedback] in .codeflow/project.toml; edit the list to suit the project"
        ),
        Ok((_, false)) => {}
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    }
    let request = serde_json::json!({
        "kind": "feedback",
        "topic": topic,
        "source": source,
        "title": summary,
    });
    let Some(mut issuer) = Issuer::new(root, Kind::Fb, summary, request) else {
        return 1;
    };
    let result = if issuer.registry {
        feedback::create_with(
            root,
            &template,
            topic,
            source,
            summary,
            &topics,
            &mut |target| issuer.allocate(target),
        )
    } else {
        let next = feedback::next_visible_id(root);
        feedback::create_with(
            root,
            &template,
            topic,
            source,
            summary,
            &topics,
            &mut |_| Ok((next.clone(), codeflow_core::ids::new_uid())),
        )
    };
    let Some(record) = issuer.settle(result) else {
        return 1;
    };
    println!("{}  {}", record.id, record.path.display());
    0
}

fn list(root: &Path, open: bool, topic: Option<&str>, json: bool, write: bool) -> i32 {
    let topics = match feedback::topics(root) {
        Ok(topics) => topics,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let loaded = feedback::load(root);
    for (path, reason) in &loaded.unreadable {
        eprintln!("warning: {path} cannot be read: {reason}");
    }
    if write {
        if !feedback::tracked(root) {
            eprintln!(
                "error: {} does not exist; record an item with `codeflow feedback new` first",
                feedback::FEEDBACK_DIR
            );
            return 1;
        }
        if !loaded.unreadable.is_empty() {
            eprintln!("error: fix the unreadable items before writing the index");
            return 1;
        }
        let index = feedback::render_index(&loaded.items, &topics);
        return match std::fs::write(root.join(feedback::INDEX_PATH), index) {
            Ok(()) => {
                println!("wrote {}", feedback::INDEX_PATH);
                0
            }
            Err(error) => {
                eprintln!("error: cannot write {}: {error}", feedback::INDEX_PATH);
                1
            }
        };
    }
    let selected: Vec<Item> = loaded
        .items
        .into_iter()
        .filter(|item| !open || item.is_open())
        .filter(|item| topic.is_none_or(|topic| item.topic == topic))
        .collect();
    let groups = feedback::grouped(&selected, &topics);
    if json {
        let value: Vec<serde_json::Value> = groups
            .iter()
            .map(|(topic, members)| serde_json::json!({ "topic": topic, "items": members }))
            .collect();
        match serde_json::to_string_pretty(&value) {
            Ok(text) => println!("{text}"),
            Err(error) => {
                eprintln!("error: {error}");
                return 1;
            }
        }
        return 0;
    }
    if !feedback::tracked(root) {
        println!("feedback: none ({} does not exist)", feedback::FEEDBACK_DIR);
        return 0;
    }
    if groups.is_empty() {
        println!("feedback: no item matches");
        return 0;
    }
    for (topic, members) in groups {
        println!("{topic}");
        for item in members {
            let placed = if item.placed_in.is_empty() {
                String::new()
            } else {
                format!("  in {}", item.placed_in.join(", "))
            };
            println!("  {}  {:<10}  {}{placed}", item.id, item.status, item.title);
        }
    }
    0
}
