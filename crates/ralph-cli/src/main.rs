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
    Plan,
    /// Execute the current plan with agent workers
    Run,
    /// Show project status and backlog
    Status,
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
        Commands::Plan => commands::plan::execute().await,
        Commands::Run => commands::run::execute().await,
        Commands::Status => commands::status::execute().await,
        Commands::Mcp => commands::mcp::execute().await,
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
