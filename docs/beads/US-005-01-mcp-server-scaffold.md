# US-005-01: MCP server scaffold and stdio transport

**Parent:** US-005 (`rewind mcp`)
**Size:** M
**Depends on:** US-002-02

## Goal
Set up the MCP server skeleton with JSON-RPC 2.0 over stdio, handling initialize/shutdown lifecycle.

## Tasks
1. Evaluate and add MCP dependency:
   - Check if `rmcp` crate is suitable, otherwise use raw JSON-RPC over stdio
   - Add dependency to `rewind-cn-core/Cargo.toml`
2. Create `infrastructure/mcp_server.rs`:
   ```rust
   pub struct RewindMcpServer<B: EventStoreBackend<RewindEvent>> {
       engine: Arc<RewindEngine<B>>,
   }

   impl RewindMcpServer {
       pub fn new(engine: Arc<RewindEngine<B>>) -> Self;
       pub async fn run(&self) -> Result<(), RewindError>; // stdio loop
   }
   ```
3. Implement JSON-RPC message loop:
   - Read line from stdin → parse as JSON-RPC request
   - Route `initialize`, `initialized`, `shutdown` methods
   - Return `ServerCapabilities` with tools and resources lists
   - Write JSON-RPC response to stdout
4. Implement `commands/mcp.rs`:
   - Load engine from `.rewind/data/`
   - Create `RewindMcpServer` and call `run()`
5. Test: send `initialize` request via stdin mock, verify response has correct capabilities

## Files touched
- `crates/rewind-cn-core/Cargo.toml` (modify — add MCP dep)
- `crates/rewind-cn-core/src/infrastructure/mcp_server.rs` (create)
- `crates/rewind-cn-core/src/infrastructure/mod.rs` (modify)
- `crates/rewind-cn/src/commands/mcp.rs` (rewrite)

## Done when
- MCP server starts, handles initialize handshake, and shuts down cleanly
- `echo '{"jsonrpc":"2.0","id":1,"method":"initialize",...}' | rewind mcp` returns valid response
