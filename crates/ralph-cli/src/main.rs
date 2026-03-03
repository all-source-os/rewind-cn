use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod commands;
mod config;

#[derive(Parser)]
#[command(name = "ralph", about = "Autonomous coding agent orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new ralph project in the current directory
    Init,
    /// Generate an execution plan from a PRD or task description
    Plan {
        /// Task or PRD description
        description: Option<String>,
        /// Read description from file
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Print plan without persisting
        #[arg(long)]
        dry_run: bool,
    },
    /// Execute the current plan with agent workers
    Run {
        /// Run a single specific task by ID
        #[arg(long)]
        task: Option<String>,
        /// Show what would execute without running
        #[arg(long)]
        dry_run: bool,
        /// Maximum concurrent workers
        #[arg(long)]
        max_concurrent: Option<usize>,
    },
    /// Show project status and backlog
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start the MCP server for IDE integration
    Mcp,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => commands::init::execute().await,
        Commands::Plan {
            description,
            file,
            dry_run,
        } => commands::plan::execute(description, file, dry_run).await,
        Commands::Run {
            task,
            dry_run,
            max_concurrent,
        } => commands::run::execute(task, dry_run, max_concurrent).await,
        Commands::Status { json } => commands::status::execute(json).await,
        Commands::Mcp => commands::mcp::execute().await,
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
