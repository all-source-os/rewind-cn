# US-005-01: MCP server scaffold and stdio transport

**Parent:** US-005 (`ralph mcp`)
**Size:** M
**Depends on:** US-002-02

## Goal
Set up the MCP server skeleton with JSON-RPC 2.0 over stdio, handling initialize/shutdown lifecycle.

## Tasks
1. Evaluate and add MCP dependency:
   - Check if `rmcp` crate is suitable, otherwise use raw JSON-RPC over stdio
   - Add dependency to `ralph-core/Cargo.toml`
2. Create `infrastructure/mcp_server.rs`:
   ```rust
   pub struct RalphMcpServer<B: EventStoreBackend<RalphEvent>> {
       engine: Arc<RalphEngine<B>>,
   }

   impl RalphMcpServer {
       pub fn new(engine: Arc<RalphEngine<B>>) -> Self;
       pub async fn run(&self) -> Result<(), RalphError>; // stdio loop
   }
   ```
3. Implement JSON-RPC message loop:
   - Read line from stdin → parse as JSON-RPC request
   - Route `initialize`, `initialized`, `shutdown` methods
   - Return `ServerCapabilities` with tools and resources lists
   - Write JSON-RPC response to stdout
4. Implement `commands/mcp.rs`:
   - Load engine from `.ralph/data/`
   - Create `RalphMcpServer` and call `run()`
5. Test: send `initialize` request via stdin mock, verify response has correct capabilities

## Files touched
- `crates/ralph-core/Cargo.toml` (modify — add MCP dep)
- `crates/ralph-core/src/infrastructure/mcp_server.rs` (create)
- `crates/ralph-core/src/infrastructure/mod.rs` (modify)
- `crates/ralph-cli/src/commands/mcp.rs` (rewrite)

## Done when
- MCP server starts, handles initialize handshake, and shuts down cleanly
- `echo '{"jsonrpc":"2.0","id":1,"method":"initialize",...}' | ralph mcp` returns valid response
