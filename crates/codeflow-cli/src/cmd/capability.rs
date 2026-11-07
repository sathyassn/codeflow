//! Capability creation uses the shared catalog schema and id registry.

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct CapabilityArgs {
    #[command(subcommand)]
    pub command: CapabilityCommand,
}

#[derive(Debug, Subcommand)]
pub enum CapabilityCommand {
    /// Start or resume the four-stage initial capability catalog.
    Bootstrap,
    /// Convert the legacy fence catalog in docs/capabilities.md into one record per capability.
    Migrate,
    /// Create a planned capability with a reserved id and stable uid.
    New {
        /// Lowercase capability slug, used in its filename.
        name: String,
        /// One sentence describing the capability for its reader.
        #[arg(long)]
        summary: String,
        /// Area declared in .codeflow/project.toml.
        #[arg(long)]
        area: String,
        /// Intended readers; repeat or separate with commas.
        #[arg(long, required = true, value_delimiter = ',', value_parser = ["external", "internal", "agent"])]
        audience: Vec<String>,
        #[arg(long, default_value = "functional", value_parser = ["functional", "non-functional"])]
        kind: String,
        /// Quality characteristic, required for non-functional capabilities.
        #[arg(long)]
        quality: Option<String>,
        /// Stimulus, metric and target for a non-functional capability.
        #[arg(long)]
        measure: Option<String>,
    },
}

pub fn run(args: &CapabilityArgs) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {}", super::shown(&error));
            return 2;
        }
    };
    if matches!(args.command, CapabilityCommand::Bootstrap) {
        return run_bootstrap(&root);
    }
    if matches!(args.command, CapabilityCommand::Migrate) {
        return run_migrate(&root);
    }
    let tracking = match codeflow_core::workgraph::durable_work_tracking_enabled(&root) {
        Ok(tracking) => tracking,
        Err(error) => {
            eprintln!("codeflow: {}", super::shown(&error));
            return 1;
        }
    };
    let CapabilityCommand::New {
        name,
        summary,
        area,
        audience,
        kind,
        quality,
        measure,
    } = &args.command
    else {
        return 2;
    };
    let options = codeflow_core::capability::create::NewCapability {
        name: name.clone(),
        summary: summary.clone(),
        area: area.clone(),
        audience: audience.clone(),
        kind: kind.clone(),
        quality: quality.clone(),
        measure: measure.clone(),
    };
    match codeflow_core::capability::create::create(&root, &options, tracking) {
        Ok(reservation) => {
            println!(
                "{}  docs/capabilities/{}-{}.md",
                reservation.id,
                reservation.id,
                super::shown(name)
            );
            eprintln!(
                "registry: {} {}",
                reservation.id,
                reservation.standing.describe()
            );
            if let Some(note) = reservation.note {
                eprintln!("registry: {}", super::shown(&note));
            }
            0
        }
        Err(error) => {
            eprintln!("codeflow capability new: {}", super::shown(&error));
            1
        }
    }
}

fn run_migrate(root: &std::path::Path) -> i32 {
    match codeflow_core::capability::migrate::migrate(root) {
        Ok(migration) => {
            for converted in &migration.converted {
                let how = if converted.page { "  (page kept)" } else { "" };
                println!("{}  {}{how}", converted.id, super::shown(&converted.file));
                eprintln!(
                    "registry: {} {}",
                    converted.id,
                    converted.standing.describe()
                );
            }
            println!(
                "migrated {} capabilities; the converted sections were removed from docs/capabilities.md",
                migration.converted.len()
            );
            0
        }
        Err(error) => {
            eprintln!("codeflow capability migrate: {}", super::shown(&error));
            1
        }
    }
}

fn run_bootstrap(root: &std::path::Path) -> i32 {
    match bootstrap(root) {
        Ok(progress) => {
            if progress.checklist {
                println!(
                    "tracking is not enabled; four-stage checklist: {}",
                    codeflow_core::capability::bootstrap::CHECKLIST
                );
            }
            if let Some(epic) = progress.epic {
                println!("bootstrap epic: {epic}");
            }
            if let Some(stage) = progress.next {
                println!(
                    "next stage: {}; guide: {}",
                    stage.name(),
                    codeflow_core::capability::bootstrap::GUIDE
                );
            } else if progress.inventory_complete {
                println!("capability inventory is complete; no bootstrap work created");
            } else {
                println!("all stages complete; set `inventory: complete` in docs/capabilities/vocabulary.yaml after the review");
            }
            0
        }
        Err(error) => {
            eprintln!("codeflow capability bootstrap: {}", super::shown(&error));
            1
        }
    }
}

fn bootstrap(
    root: &std::path::Path,
) -> Result<codeflow_core::capability::bootstrap::Progress, String> {
    use crate::embedded::EmbeddedAssets;
    use codeflow_core::capability::bootstrap::{self, Stage, Templates};
    use codeflow_core::scaffold::AssetSource;
    let body = |name: &str| -> Result<String, String> {
        let asset = format!("base/pm/capability-bootstrap/{name}.md");
        let bytes = EmbeddedAssets
            .read(&asset)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("bootstrap template {asset} is absent"))?;
        let embedded = String::from_utf8(bytes).map_err(|error| error.to_string())?;
        bootstrap::load_body(root, name, &embedded)
    };
    let tracking = codeflow_core::workgraph::durable_work_tracking_enabled(root)
        .map_err(|error| error.to_string())?;
    let tier = codeflow_core::scaffold::state::ProjectState::load(root)
        .map_err(|error| error.to_string())?
        .tier;
    let enable_tracking = !tracking && tier >= codeflow_core::scaffold::Tier::Standard;
    let templates = Templates {
        epic: super::new::load_template("base/pm/epic.md.tmpl")?,
        task: super::new::load_template("base/pm/task.md.tmpl")?,
        epic_body: body("epic")?,
        stages: [
            body(Stage::Inventory.name())?,
            body(Stage::Classification.name())?,
            body(Stage::Evidence.name())?,
            body(Stage::Review.name())?,
        ],
    };
    let progress = bootstrap::run(root, tracking || enable_tracking, &templates, |creation| {
        if creation.stage.is_some() {
            super::new::create_task_record(
                root,
                &creation.template,
                creation.epic,
                None,
                None,
                creation.title,
            )
        } else {
            super::new::create_epic_record(root, &creation.template, creation.title)
        }
    })?;
    if enable_tracking && progress.epic.is_some() {
        println!("tracking enabled: bootstrap created project-management/epics and project-management/tasks; policy unchanged");
    }
    Ok(progress)
}
