use std::path::Path;

use tracing::info;

use crate::config::RewindConfig;

const REWIND_DIR: &str = ".rewind";
const DATA_DIR: &str = ".rewind/data";
const CONFIG_FILE: &str = ".rewind/rewind.toml";

pub async fn execute() -> Result<(), String> {
    let rewind_dir = Path::new(REWIND_DIR);

    if rewind_dir.exists() {
        return Err("Project already initialized (.rewind/ directory exists)".into());
    }

    // Create directory structure
    std::fs::create_dir_all(DATA_DIR)
        .map_err(|e| format!("Failed to create data directory: {e}"))?;

    // Write default config
    let config = RewindConfig::default();
    config.save(Path::new(CONFIG_FILE))?;

    // Generate anonymous telemetry ID
    let telemetry_id = uuid::Uuid::new_v4().to_string();
    let telemetry_id_path = Path::new(REWIND_DIR).join("telemetry_id");
    std::fs::write(&telemetry_id_path, &telemetry_id)
        .map_err(|e| format!("Failed to write telemetry ID: {e}"))?;

    // Initialize the engine (creates event store)
    let _engine = rewind_cn_core::infrastructure::engine::RewindEngine::init(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    info!("Initialized rewind project");
    println!("Initialized rewind project in {REWIND_DIR}/");
    println!("  Config: {CONFIG_FILE}");
    println!("  Data:   {DATA_DIR}/");
    println!();
    println!("Next steps:");
    println!("  1. Configure LLM in .rewind/rewind.toml (add [agent] section)");
    println!("  2. Create a plan:  rewind plan \"Build a REST API for users\"");
    println!("     Or from a file: rewind plan -f docs/prd.md");
    println!("     Or import beads: rewind import tasks.jsonl");
    println!("  3. Run tasks:      rewind run");
    println!();
    println!("Without [agent] config, `plan` creates a single passthrough task.");
    println!("See: rewind plan --help, rewind run --help");

    Ok(())
}
