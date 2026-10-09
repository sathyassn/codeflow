//! Durable work-record creation and status verbs. The CLI writes independent
//! `EPC-NNN`, `SPC-NNN`, and `TSK-NNN` ids in the canonical flat layout, and
//! changes a record's status only through the lifecycle judge (SPC-013 R-34).
//! Where durable work is tracked, each id is issued from the shared registry
//! with the record's hidden `uid` (SPC-013 R-12).

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use codeflow_core::ids::issue::{self, Request};
use codeflow_core::ids::{Kind, RegId, Standing};
use codeflow_core::scaffold::AssetSource;
use codeflow_core::workgraph::light_paths;
use codeflow_core::workgraph::record_template::{self, TemplateSource};
use codeflow_core::workgraph::status_verb::{set_status, StatusChange};
use codeflow_core::workgraph::{
    allocate, durable_work_tracking_enabled, is_valid_epic_format_id, NewRecord, RecordKind,
    StoreError,
};

use crate::embedded::EmbeddedAssets;

/// Arguments for `codeflow epic`.
#[derive(Debug, Args)]
pub struct EpicArgs {
    #[command(subcommand)]
    pub command: EpicCommand,
}

/// Epic subcommands.
#[derive(Debug, Subcommand)]
pub enum EpicCommand {
    /// Allocate the next `EPC-NNN` and scaffold the epic from the template.
    New {
        /// Also cut `integration/EPC-NNN-<slug>` from main (or master) and
        /// push it to origin when that remote exists.
        #[arg(long)]
        integration: bool,
        /// Epic title.
        title: String,
    },
    /// Record an epic terminal act, judged by the epic close rules.
    Status {
        /// Epic id.
        id: String,
        /// The terminal act to record.
        status: EpicStatusArg,
        #[command(flatten)]
        details: StatusDetails,
    },
}

/// Statuses `task status` writes. `in_progress` is never written.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TaskStatusArg {
    Todo,
    Blocked,
    Complete,
    Cancelled,
}

/// Epic terminal acts.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum EpicStatusArg {
    Complete,
    Cancelled,
    Archived,
}

/// Statuses `spec status` writes; `implemented` is derived. `draft` is
/// never written: it is accepted only so the lifecycle judge refuses it with
/// the route an approved spec takes (TSK-169).
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SpecStatusArg {
    Approved,
    Superseded,
    #[value(hide = true)]
    Draft,
}

/// What a transition records beside the status.
#[derive(Debug, Clone, Default, Args)]
pub struct StatusDetails {
    /// Blocker, cancellation or reopen reason.
    #[arg(long)]
    pub reason: Option<String>,
    /// Who owns the blocker.
    #[arg(long)]
    pub owner: Option<String>,
    /// The event that revisits the blocker.
    #[arg(long)]
    pub revisit: Option<String>,
    /// Where a cancelled record's scope went.
    #[arg(long)]
    pub scope: Option<String>,
    /// A file holding the acceptance block (the YAML between the fences) to
    /// add to the Closeout on completion.
    #[arg(long, value_name = "FILE")]
    pub acceptance: Option<PathBuf>,
}

/// Arguments for `codeflow task`.
#[derive(Debug, Args)]
pub struct TaskArgs {
    #[command(subcommand)]
    pub command: TaskCommand,
}

/// Task subcommands.
#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// Allocate the next independent `TSK-NNN` and scaffold it.
    New {
        /// Parent epic id. Mutually exclusive with --standalone-reason.
        #[arg(long, short, value_name = "EPC-NNN")]
        epic: Option<String>,
        /// Why this durable task does not belong to an epic; may run on its task branch.
        #[arg(long, value_name = "REASON")]
        standalone_reason: Option<String>,
        /// Existing local or remote-tracking non-task branch this task will integrate into.
        #[arg(long = "into", value_name = "BRANCH")]
        integration_target: Option<String>,
        /// File a follow-up of this task: records `follow_up_of` and inherits
        /// its target. An epic task's follow-up inherits the epic and is filed
        /// on a plan/ branch. A standalone task never uses a plan/ branch and
        /// the command refuses one: its follow-up is a standalone task, filed
        /// on a task branch cut from the target, with its record filled in and
        /// committed, then claimed with `work claim`.
        #[arg(
            long,
            value_name = "TSK-NNN",
            conflicts_with_all = ["epic", "standalone_reason", "integration_target"]
        )]
        follow_up_of: Option<String>,
        /// Write the record of a reservation whose write was interrupted.
        #[arg(long, value_name = "TSK-NNN", conflicts_with_all = ["epic", "standalone_reason", "integration_target", "follow_up_of", "title"])]
        resume: Option<String>,
        /// Task title.
        #[arg(required_unless_present = "resume")]
        title: Option<String>,
    },
    /// Change a task's status through the transition rules.
    Status {
        /// Task id.
        id: String,
        /// The new status.
        status: TaskStatusArg,
        #[command(flatten)]
        details: StatusDetails,
    },
}

/// Arguments for `codeflow spec`.
#[derive(Debug, Args)]
pub struct SpecArgs {
    #[command(subcommand)]
    pub command: SpecCommand,
}

/// Spec subcommands.
#[derive(Debug, Subcommand)]
pub enum SpecCommand {
    /// Allocate the next `SPC-NNN`, scaffold it, and link the consuming work item.
    New {
        /// Epic or task that consumes this specification; repeat the flag
        /// (or separate ids with commas) for several consumers.
        #[arg(
            long = "for",
            value_name = "EPC-NNN|TSK-NNN",
            required = true,
            value_delimiter = ','
        )]
        work_item: Vec<String>,
        /// Specification title.
        title: String,
    },
    /// Approve a draft spec, or supersede an approved one by a new revision.
    Status {
        /// Spec id.
        id: String,
        /// The new status.
        status: SpecStatusArg,
        /// The new revision that supersedes this spec.
        #[arg(long, value_name = "SPC-NNN")]
        by: Option<String>,
    },
}

/// Run `codeflow epic`.
pub fn run_epic(args: &EpicArgs) -> i32 {
    let (integration, title) = match &args.command {
        EpicCommand::New { integration, title } => (*integration, title),
        EpicCommand::Status {
            id,
            status,
            details,
        } => {
            let target = match status {
                EpicStatusArg::Complete => "complete",
                EpicStatusArg::Cancelled => "cancelled",
                EpicStatusArg::Archived => "archived",
            };
            return run_status(RecordKind::Epic, id, target, details, None);
        }
    };
    let root = super::repo_root();
    let pm = root.join("project-management");
    let Some(template) = load_template("base/pm/epic.md.tmpl") else {
        eprintln!("error: epic template unavailable");
        return 1;
    };
    let Some(mut issuer) = Issuer::new(
        &root,
        Kind::Epc,
        title,
        serde_json::json!({ "kind": "epic", "title": title }),
    ) else {
        return 1;
    };
    let result = if issuer.registry {
        allocate::create_epic_with(&pm, &template, title, &mut |target| issuer.allocate(target))
    } else {
        allocate::create_epic(&pm, &template, title)
    };
    let Some(rec) = issuer.settle(result) else {
        return 1;
    };
    report(&rec);
    if !integration {
        return 0;
    }
    match light_paths::create_integration_branch(&super::repo_root(), &rec.id, title) {
        Ok(branch) => {
            println!(
                "{}  from {}{}",
                branch.name,
                branch.from,
                if branch.pushed {
                    ", pushed to origin"
                } else {
                    ", local only (no origin remote)"
                }
            );
            0
        }
        Err(error) => {
            eprintln!("error: integration branch: {error}");
            1
        }
    }
}

/// Run `codeflow task`.
pub fn run_task(args: &TaskArgs) -> i32 {
    let (epic, standalone_reason, integration_target, title) = match &args.command {
        TaskCommand::New {
            follow_up_of: Some(source),
            title,
            ..
        } => return run_follow_up(source, title.as_deref().unwrap_or_default()),
        TaskCommand::New {
            resume: Some(id), ..
        } => return resume_task(id),
        TaskCommand::New {
            epic,
            standalone_reason,
            integration_target,
            title,
            ..
        } => (
            epic.clone(),
            standalone_reason.clone(),
            integration_target.clone(),
            title.clone().unwrap_or_default(),
        ),
        TaskCommand::Status {
            id,
            status,
            details,
        } => {
            let target = match status {
                TaskStatusArg::Todo => "todo",
                TaskStatusArg::Blocked => "blocked",
                TaskStatusArg::Complete => "complete",
                TaskStatusArg::Cancelled => "cancelled",
            };
            return run_status(RecordKind::Task, id, target, details, None);
        }
    };
    if epic
        .as_deref()
        .is_some_and(|value| !is_valid_epic_format_id(value))
    {
        eprintln!(
            "error: invalid epic id '{}' (expected EPC-NNN)",
            epic.as_deref().unwrap_or_default()
        );
        return 2;
    }
    match (epic.as_deref(), standalone_reason.as_deref()) {
        (Some(_), Some(_)) => {
            eprintln!("error: --epic and --standalone-reason are mutually exclusive");
            return 2;
        }
        (None, None) => {
            eprintln!("error: task new requires --epic or --standalone-reason");
            return 2;
        }
        _ => {}
    }
    new_task(
        epic.as_deref(),
        standalone_reason.as_deref(),
        integration_target.as_deref(),
        &title,
    )
}

/// Write a new task record, its id issued from the registry where durable
/// work is tracked.
fn new_task(
    epic: Option<&str>,
    standalone_reason: Option<&str>,
    integration_target: Option<&str>,
    title: &str,
) -> i32 {
    let root = super::repo_root();
    let pm = root.join("project-management");
    if epic.is_some_and(|value| !allocate::epic_exists(&pm, value)) {
        eprintln!(
            "error: epic {} not found under {}",
            epic.unwrap_or_default(),
            pm.display()
        );
        return 1;
    }
    let Some(template) = load_template("base/pm/task.md.tmpl") else {
        eprintln!("error: task template unavailable");
        return 1;
    };
    let request = serde_json::json!({
        "kind": "task",
        "epic": epic,
        "standalone_reason": standalone_reason,
        "into": integration_target,
        "title": title,
    });
    let Some(mut issuer) = Issuer::new(&root, Kind::Tsk, title, request) else {
        return 1;
    };
    let result = if issuer.registry {
        allocate::create_task_with(
            &pm,
            &template,
            epic,
            standalone_reason,
            integration_target,
            title,
            &mut |target| issuer.allocate(target),
        )
    } else {
        allocate::create_task(
            &pm,
            &template,
            epic,
            standalone_reason,
            integration_target,
            title,
        )
    };
    issuer.finish(result)
}

/// `task new --resume TSK-NNN` (SPC-013 R-18): write the record of a
/// reservation whose write was interrupted, only when this clone issued it
/// and the registry binds it to the same `uid`.
fn resume_task(id: &str) -> i32 {
    let Some(parsed) = RegId::parse(id).filter(|parsed| parsed.kind() == Kind::Tsk) else {
        eprintln!("error: '{id}' is not a task id (TSK-NNN)");
        return 2;
    };
    let root = super::repo_root();
    let unwritten = match issue::resume(&root, &parsed) {
        Ok(unwritten) => unwritten,
        Err(error) => {
            eprintln!("error: {error}");
            return 1;
        }
    };
    let field = |key: &str| {
        unwritten
            .request
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let Some(template) = load_template("base/pm/task.md.tmpl") else {
        eprintln!("error: task template unavailable");
        return 1;
    };
    let pm = root.join("project-management");
    let (record_id, uid) = (unwritten.id.clone(), unwritten.uid.clone());
    let title = field("title").unwrap_or_default();
    let mut reserved = |_: &str| Ok((record_id.clone(), uid.clone()));
    // A follow-up re-derives its epic and target from its source.
    let result = match field("follow_up_of") {
        Some(source) => light_paths::create_follow_up_with(
            &root,
            &template,
            &source,
            &title,
            Some(&mut reserved),
        ),
        None => allocate::create_task_with(
            &pm,
            &template,
            field("epic").as_deref(),
            field("standalone_reason").as_deref(),
            field("into").as_deref(),
            &title,
            &mut reserved,
        ),
    };
    match result {
        Ok(record) => {
            if let Err(error) = issue::written(&root, &parsed) {
                eprintln!("warning: {error}");
            }
            report(&record)
        }
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}

/// `task new --follow-up-of`: one command, then one pull request: the
/// epic's batched amendment, or the follow-up's own task pull request when
/// its source is standalone.
/// Where durable work is tracked, its id is issued from the registry like
/// any other task (SPC-013 R-12).
fn run_follow_up(source: &str, title: &str) -> i32 {
    let Some(template) = load_template("base/pm/task.md.tmpl") else {
        eprintln!("error: task template unavailable");
        return 1;
    };
    let root = super::repo_root();
    let request = serde_json::json!({ "kind": "task", "follow_up_of": source, "title": title });
    let Some(mut issuer) = Issuer::new(&root, Kind::Tsk, title, request) else {
        return 1;
    };
    let result = if issuer.registry {
        light_paths::create_follow_up_with(
            &root,
            &template,
            source,
            title,
            Some(&mut |target| issuer.allocate(target)),
        )
    } else {
        light_paths::create_follow_up(&root, &template, source, title)
    };
    let Some(rec) = issuer.settle(result) else {
        return 1;
    };
    report(&rec);
    println!("  follow_up_of: {source}");
    0
}

/// Arguments for `codeflow adr`.
#[derive(Debug, Args)]
pub struct AdrArgs {
    #[command(subcommand)]
    pub command: AdrCommand,
}

/// ADR subcommands.
#[derive(Debug, Subcommand)]
pub enum AdrCommand {
    /// Number the next ADR and write it from the template as `proposed`.
    New {
        /// Decision title.
        title: String,
    },
}

/// Run `codeflow adr`. The project's `docs/decisions/template.md` is used
/// when present, the shipped template otherwise; where durable work is
/// tracked the number is issued from the registry (TSK-104 AC-4).
pub fn run_adr(args: &AdrArgs) -> i32 {
    let AdrCommand::New { title } = &args.command;
    let root = super::repo_root();
    let template = std::fs::read_to_string(root.join("docs/decisions/template.md"))
        .ok()
        .or_else(|| load_template("base/docs/decisions/template.md"));
    let Some(template) = template else {
        eprintln!("error: ADR template unavailable");
        return 1;
    };
    let request = serde_json::json!({ "kind": "adr", "title": title });
    let Some(mut issuer) = Issuer::new(&root, Kind::Adr, title, request) else {
        return 1;
    };
    let result = if issuer.registry {
        light_paths::create_adr_with(&root, &template, title, &mut |target| {
            issuer.allocate(target)
        })
    } else {
        light_paths::create_adr(&root, &template, title)
    };
    issuer.finish(result)
}

/// How a new record gets its id: from the shared registry where durable
/// work is tracked, else from the visible checkout.
pub(crate) struct Issuer<'a> {
    root: &'a Path,
    kind: Kind,
    title: String,
    request: serde_json::Value,
    pub(crate) registry: bool,
    reserved: Option<(RegId, Standing, Option<String>)>,
}

impl<'a> Issuer<'a> {
    pub(crate) fn new(
        root: &'a Path,
        kind: Kind,
        title: &str,
        request: serde_json::Value,
    ) -> Option<Self> {
        let registry = match durable_work_tracking_enabled(root) {
            Ok(tracked) => tracked,
            Err(error) => {
                eprintln!(
                    "error: {}",
                    codeflow_core::workgraph::work_start::tracking_state_message(error)
                );
                return None;
            }
        };
        Some(Issuer {
            root,
            kind,
            title: title.to_string(),
            request,
            registry,
            reserved: None,
        })
    }

    pub(crate) fn allocate(&mut self, target: &str) -> Result<(String, String), StoreError> {
        let mut request = Request::issue(self.kind, &self.title, target);
        request.resume = Some(self.request.clone());
        let reservation = issue::reserve(self.root, &request)?;
        self.reserved = Some((
            reservation.id.clone(),
            reservation.standing,
            reservation.note.clone(),
        ));
        Ok((reservation.id.to_string(), reservation.uid))
    }

    /// Settle the reservation against the write: report the registry
    /// standing on success, or the reserved-but-unwritten number on failure.
    pub(crate) fn settle(self, result: Result<NewRecord, StoreError>) -> Option<NewRecord> {
        match (result, self.reserved) {
            (Ok(record), reserved) => {
                if let Some((id, standing, note)) = reserved {
                    if let Err(error) = issue::written(self.root, &id) {
                        eprintln!("warning: {error}");
                    }
                    eprintln!("registry: {id} {}", standing.describe());
                    if let Some(note) = note {
                        eprintln!("registry: {note}");
                    }
                }
                Some(record)
            }
            (Err(error), Some((id, _, _))) => {
                eprintln!("error: {error}");
                eprintln!(
                    "{id} is reserved but its record was not written; fix the cause and run `codeflow task new --resume {id}` (tasks), or leave the number as a gap"
                );
                None
            }
            (Err(error), None) => {
                eprintln!("error: {error}");
                None
            }
        }
    }

    fn finish(self, result: Result<NewRecord, StoreError>) -> i32 {
        self.settle(result).map_or(1, |record| report(&record))
    }
}

/// Run `codeflow spec`.
pub fn run_spec(args: &SpecArgs) -> i32 {
    let (work_item, title) = match &args.command {
        SpecCommand::New { work_item, title } => (work_item, title),
        SpecCommand::Status { id, status, by } => {
            let target = match status {
                SpecStatusArg::Approved => "approved",
                SpecStatusArg::Superseded => "superseded",
                SpecStatusArg::Draft => "draft",
            };
            return run_status(
                RecordKind::Spec,
                id,
                target,
                &StatusDetails::default(),
                by.clone(),
            );
        }
    };
    let root = super::repo_root();
    let pm = root.join("project-management");
    for work_item in work_item {
        if !allocate::work_item_exists(&pm, work_item) {
            eprintln!(
                "error: work item {work_item} not found under {}",
                pm.display()
            );
            return 1;
        }
    }
    let Some(template) = load_template("base/pm/spec.md.tmpl") else {
        eprintln!("error: spec template unavailable");
        return 1;
    };
    let request = serde_json::json!({ "kind": "spec", "for": work_item, "title": title });
    let Some(mut issuer) = Issuer::new(&root, Kind::Spc, title, request) else {
        return 1;
    };
    let result = if issuer.registry {
        allocate::create_spec_with(&pm, &template, work_item, title, &mut |target| {
            issuer.allocate(target)
        })
    } else {
        allocate::create_spec_for(&pm, &template, work_item, title)
    };
    issuer.finish(result)
}

/// Run one status verb: exit 0 when written, 1 when refused or failed.
fn run_status(
    kind: RecordKind,
    id: &str,
    target: &str,
    details: &StatusDetails,
    by: Option<String>,
) -> i32 {
    let acceptance = match &details.acceptance {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(error) => {
                eprintln!("error: cannot read {}: {error}", path.display());
                return 1;
            }
        },
        None => None,
    };
    let change = StatusChange {
        target: target.to_string(),
        reason: details.reason.clone(),
        owner: details.owner.clone(),
        revisit: details.revisit.clone(),
        scope: details.scope.clone(),
        by,
        acceptance,
    };
    match set_status(&super::repo_root(), kind, id, &change) {
        Ok(outcome) => {
            for warning in &outcome.warnings {
                eprintln!("warning: {warning}");
            }
            println!(
                "{id}  {} -> {}  {}",
                outcome.from,
                outcome.to,
                outcome.path.display()
            );
            0
        }
        Err(error) => {
            eprintln!("error: {id}: {error}");
            1
        }
    }
}

/// Print the allocated id and the file written, then return exit code 0.
fn report(rec: &NewRecord) -> i32 {
    println!("{}  {}", rec.id, rec.path.display());
    0
}

/// The record template for `kind` (SPC-013 R-37): the project's
/// `project-management/templates/<kind>.md` when it is usable, otherwise the
/// embedded one, with a warning that names why the project's was not used.
/// Any other embedded template is returned as shipped.
fn load_template(asset: &str) -> Option<String> {
    let embedded = EmbeddedAssets
        .read(asset)
        .and_then(|bytes| String::from_utf8(bytes).ok())?;
    if !asset.starts_with("base/pm/") {
        // Not a work record (the ADR template): no project record template.
        return Some(embedded);
    }
    let kind = if asset.ends_with("epic.md.tmpl") {
        RecordKind::Epic
    } else if asset.ends_with("spec.md.tmpl") {
        RecordKind::Spec
    } else {
        RecordKind::Task
    };
    let (text, source) = record_template::load(&super::repo_root(), kind, &embedded);
    if let TemplateSource::Fallback(reason) = source {
        eprintln!("warning: {reason}");
    }
    Some(text)
}
