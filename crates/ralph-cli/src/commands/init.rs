use std::path::Path;

use tracing::info;

use crate::config::RalphConfig;

const RALPH_DIR: &str = ".ralph";
const DATA_DIR: &str = ".ralph/data";
const CONFIG_FILE: &str = ".ralph/ralph.toml";

pub async fn execute() -> Result<(), String> {
    let ralph_dir = Path::new(RALPH_DIR);

    if ralph_dir.exists() {
        return Err("Project already initialized (.ralph/ directory exists)".into());
    }

    // Create directory structure
    std::fs::create_dir_all(DATA_DIR)
        .map_err(|e| format!("Failed to create data directory: {e}"))?;

    // Write default config
    let config = RalphConfig::default();
    config.save(Path::new(CONFIG_FILE))?;

    // Initialize the engine (creates event store)
    let _engine = ralph_core::infrastructure::engine::RalphEngine::init(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    info!("Initialized ralph project");
    println!("Initialized ralph project in {RALPH_DIR}/");
    println!("  Config: {CONFIG_FILE}");
    println!("  Data:   {DATA_DIR}/");
    println!();
    println!("Edit .ralph/ralph.toml to configure your project.");

    Ok(())
}
