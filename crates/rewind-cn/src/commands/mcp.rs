use std::path::Path;
use std::sync::Arc;

use rewind_cn_core::infrastructure::engine::RewindEngine;
use rewind_cn_core::infrastructure::mcp_server::RewindMcpServer;

const DATA_DIR: &str = ".rewind/data";
const CONFIG_FILE: &str = ".rewind/rewind.toml";

pub async fn execute() -> Result<(), String> {
    if !Path::new(".rewind").exists() {
        return Err("No rewind project found. Run `rewind init` first.".into());
    }

    let engine = RewindEngine::load(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    let server = RewindMcpServer::new(Arc::new(engine), CONFIG_FILE.into());

    eprintln!("MCP server running on stdin/stdout. Connect your IDE to use rewind tools.");
    eprintln!("Press Ctrl+C to stop.");

    server.run().await.map_err(|e| e.to_string())
}
