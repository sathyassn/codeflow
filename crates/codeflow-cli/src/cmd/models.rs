//! Read-only catalog resolution. No registry updates or native launches.

use std::collections::BTreeMap;

use clap::{Args, Subcommand};
use codeflow_core::model_catalog::{
    anchored_override, load_catalog_document, parse_override_route, Catalog, CatalogDocument,
    CatalogInputs, Exclusion, ExclusionScope, ParticipantLabel, Resolution, ResolveRequest,
};

#[derive(Debug, Args)]
pub struct ModelsArgs {
    #[command(subcommand)]
    command: ModelsCommand,
}

#[derive(Debug, Subcommand)]
enum ModelsCommand {
    /// Resolve every participant and obligation without launching or writing.
    Resolve(ResolveArgs),
}

#[derive(Debug, Args)]
struct ResolveArgs {
    /// Catalog duty whose participants and obligations must be resolved.
    #[arg(long)]
    duty: String,
    /// Supported host harness id, such as claude-code or codex-app.
    #[arg(long)]
    host: String,
    #[arg(long, default_value = "none", value_parser = ["claude", "codex", "grok", "none"])]
    author: String,
    /// Fresh native exclusion: `selector:<id>` excludes the whole version across harnesses; `bucket:<id>` excludes its usage bucket (repeatable).
    #[arg(long)]
    exclude: Vec<String>,
    /// Fresh identity canary observation: `<pinned-id>=<observed-id>`.
    #[arg(long)]
    observed: Vec<String>,
    /// Catalog trigger fact to apply to this resolution (repeatable).
    #[arg(long)]
    trigger: Vec<String>,
    /// Canonical task id for this resolution; required with --override.
    #[arg(long)]
    task: Option<String>,
    /// Task id of the anchored `OPERATOR_OVERRIDE`; must equal --task.
    #[arg(long = "override")]
    override_id: Option<String>,
    /// Exact override invocation route: `<seat-or-line>@<harness>`.
    #[arg(long)]
    route: Option<String>,
    /// Exact override invocation effort.
    #[arg(long)]
    effort: Option<String>,
    /// Print the resolution as JSON; input errors go to stderr with exit 2.
    #[arg(long)]
    json: bool,
}

pub fn run(args: &ModelsArgs) -> i32 {
    let ModelsCommand::Resolve(args) = &args.command;
    let result = match resolve(args) {
        Ok(result) => result,
        Err(reason) => {
            eprintln!("models resolve: {reason}");
            return 2;
        }
    };
    if args.json {
        match serde_json::to_string_pretty(&result) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("models resolve: {error}");
                return 2;
            }
        }
    } else {
        render_text(&result);
    }
    i32::from(result.is_open())
}

fn resolve(args: &ResolveArgs) -> Result<Resolution, String> {
    if args.override_id.is_some() && args.task.is_none() {
        return Err("--override requires --task".into());
    }
    let root = super::repo_root();
    let CatalogDocument::Current(catalog) = load_catalog_document(&root)? else {
        return Err("models resolve requires managed catalog schema 5; schema 4 remains supported by doctor".into());
    };
    if args.duty == "test-authoring" {
        return Err(
            "test-authoring is not resolved separately; use the unit's implementation duty".into(),
        );
    }
    if !catalog.duties.contains_key(&args.duty) {
        return Err(format!("unknown duty {}", args.duty));
    }
    let home = codeflow_core::registry::codeflow_home();
    let mut inputs = CatalogInputs::load(*catalog, &root, home.as_deref())?;
    if !inputs
        .catalog
        .families
        .iter()
        .any(|f| f.harnesses.contains(&args.host))
    {
        return Err("unsupported host harness".into());
    }
    for exclusion in &args.exclude {
        inputs
            .exclusions
            .extend(parse_exclusion(&inputs.catalog, exclusion)?);
    }
    let mut observed = BTreeMap::new();
    for fact in &args.observed {
        let (pinned, actual) = fact
            .split_once('=')
            .ok_or("observed must be pinned-id=observed-id")?;
        if pinned.trim().is_empty() || actual.trim().is_empty() || actual.contains('=') {
            return Err("observed must contain two nonempty identities".into());
        }
        if !inputs
            .catalog
            .lines
            .iter()
            .flat_map(|l| &l.versions)
            .any(|v| v.pinned_id == pinned)
        {
            return Err(format!("unknown observed pinned id {pinned}"));
        }
        if observed.insert(pinned.into(), actual.into()).is_some() {
            return Err(format!("duplicate observation for {pinned}"));
        }
    }
    let task = args.task.as_deref().unwrap_or("");
    if args.task.is_some() && !codeflow_core::workgraph::is_canonical_task_format_id(task) {
        return Err("task must be a canonical TSK id".into());
    }
    let (route, record) = if let Some(id) = &args.override_id {
        let route = parse_override_route(
            &inputs.catalog,
            args.route.as_deref().ok_or("override requires --route")?,
            args.effort.as_deref().ok_or("override requires --effort")?,
        )?;
        let record = anchored_override(&root, &inputs.catalog, task, id, &args.duty, &route)?;
        (Some(route), Some(record))
    } else {
        if args.route.is_some() || args.effort.is_some() {
            return Err("--route and --effort require --override".into());
        }
        (None, None)
    };
    inputs.catalog.resolve(&ResolveRequest {
        duty: &args.duty,
        task,
        host_harness: &args.host,
        author_lineage: (args.author != "none").then_some(args.author.as_str()),
        exclusions: &inputs.exclusions,
        observed_ids: &observed,
        trigger_facts: &args.trigger,
        requested_override: route.as_ref(),
        operator_override: record.as_ref(),
    })
}

fn parse_exclusion(catalog: &Catalog, text: &str) -> Result<Vec<Exclusion>, String> {
    let (kind, id) = text
        .split_once(':')
        .ok_or("exclude must be `selector:<id>` or `bucket:<id>`")?;
    let scopes: Vec<_> = match kind {
        "selector" => catalog
            .lines
            .iter()
            .flat_map(|l| &l.versions)
            .filter(|v| {
                v.id == id
                    || v.pinned_id == id
                    || v.alias == id
                    || v.selectors.values().any(|s| s == id)
            })
            .map(|v| ExclusionScope::Version(v.id.clone()))
            .collect(),
        "bucket" if catalog.families.iter().any(|f| f.usage_bucket == id) => {
            vec![ExclusionScope::UsageBucket(id.into())]
        }
        _ => return Err("exclude must name a catalog selector or bucket".into()),
    };
    if scopes.is_empty() {
        return Err(format!("unknown excluded selector {id}"));
    }
    Ok(scopes
        .into_iter()
        .map(|scope| Exclusion {
            scope,
            reason: format!("caller supplied {text}"),
            fresh_native: true,
        })
        .collect())
}

fn render_text(result: &Resolution) {
    for (prefix, p) in result
        .participants
        .iter()
        .map(|p| ("", p))
        .chain(result.obligations.iter().map(|p| ("obligation: ", p)))
    {
        let label = match p.label {
            ParticipantLabel::Required => "required",
            ParticipantLabel::SecondOpinion => "second-opinion",
        };
        println!(
            "{prefix}{}: {} / {} version={} pinned={} harness={} effort={} [{}{}]",
            p.participant,
            p.seat.as_deref().unwrap_or("worker"),
            p.line,
            p.version,
            p.pinned_id,
            p.harness,
            p.effort.as_str(),
            label,
            if p.reduced_assurance {
                ", reduced assurance"
            } else {
                ""
            }
        );
        for limitation in &p.limitations {
            println!("  {limitation}");
        }
        for alternative in &p.remaining_alternatives {
            println!(
                "  alternative: {} / {} version={} pinned={} harness={} effort={}",
                alternative.seat.as_deref().unwrap_or("worker"),
                alternative.line,
                alternative.version,
                alternative.pinned_id,
                alternative.harness,
                alternative.effort.as_str()
            );
        }
    }
    for gap in &result.open {
        let prefix = if gap.participant.starts_with("xhigh-reasoning:") {
            "obligation: "
        } else {
            ""
        };
        let label = if gap.label == ParticipantLabel::Required {
            "required"
        } else {
            "second-opinion"
        };
        println!(
            "{prefix}{}: open [{label}]: {}",
            gap.participant,
            gap.reasons.join("; ")
        );
    }
    println!("xhigh trigger met: {}", result.xhigh_trigger_met);
}
