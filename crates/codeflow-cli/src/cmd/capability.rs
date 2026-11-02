//! Capability creation uses the shared catalog schema and id registry.

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct CapabilityArgs {
    #[command(subcommand)]
    pub command: CapabilityCommand,
}

#[derive(Debug, Subcommand)]
pub enum CapabilityCommand {
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
    } = &args.command;
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
