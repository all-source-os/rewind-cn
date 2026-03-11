use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod commands;
mod config;
mod tui;

#[derive(Parser)]
#[command(name = "rewind", about = "Autonomous coding agent orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new rewind project in the current directory
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
        /// Use git worktrees for parallel task isolation
        #[arg(long)]
        parallel: bool,
        /// Show TUI dashboard during execution
        #[arg(long)]
        tui: bool,
    },
    /// Show project status and backlog
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Query execution analytics
    Query {
        /// Query name (task-summary, epic-summary, tool-usage, session-history, list)
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Filter by epic ID
        #[arg(long)]
        epic: Option<String>,
    },
    /// Import tasks/epics from a beads JSONL or JSON file
    Import {
        /// Path to the file to import (.jsonl for beads, .json)
        file: String,
        /// Skip closed/done items during import
        #[arg(long, default_value_t = true)]
        skip_closed: bool,
    },
    /// Submit feedback or report an issue
    Feedback {
        /// Feedback message
        message: String,
        /// Attach an anonymized diagnostic report
        #[arg(long)]
        attach_report: bool,
    },
    /// Export a diagnostic report for troubleshooting
    Report {
        /// Export a specific session (default: last session)
        #[arg(long)]
        session: Option<String>,
        /// Include non-anonymized data (task titles, descriptions)
        #[arg(long)]
        full: bool,
    },
    /// Start the MCP server for IDE integration
    Mcp,
}

#[tokio::main]
#[hotpath::main]
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
            parallel,
            tui: use_tui,
        } => commands::run::execute(task, dry_run, max_concurrent, parallel, use_tui).await,
        Commands::Status { json } => commands::status::execute(json).await,
        Commands::Query { name, json, epic } => commands::query::execute(name, json, epic).await,
        Commands::Import { file, skip_closed } => {
            commands::import::execute(file, skip_closed).await
        }
        Commands::Feedback {
            message,
            attach_report,
        } => commands::feedback::execute(message, attach_report).await,
        Commands::Report { session, full } => commands::report::execute(session, full).await,
        Commands::Mcp => commands::mcp::execute().await,
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
