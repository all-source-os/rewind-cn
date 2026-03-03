use std::path::Path;
use std::sync::Arc;

use ralph_core::infrastructure::engine::RalphEngine;
use ralph_core::infrastructure::mcp_server::RalphMcpServer;

const DATA_DIR: &str = ".ralph/data";
const CONFIG_FILE: &str = ".ralph/ralph.toml";

pub async fn execute() -> Result<(), String> {
    if !Path::new(".ralph").exists() {
        return Err("No ralph project found. Run `ralph init` first.".into());
    }

    let engine = RalphEngine::load(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    let server = RalphMcpServer::new(Arc::new(engine), CONFIG_FILE.into());

    server.run().await.map_err(|e| e.to_string())
}
