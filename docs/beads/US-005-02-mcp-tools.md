# US-005-02: MCP tool handlers

**Parent:** US-005 (`ralph mcp`)
**Size:** M
**Depends on:** US-005-01, US-003-03, US-004-04

## Goal
Register MCP tools that expose ralph's functionality to IDE clients.

## Tasks
1. In `infrastructure/mcp_server.rs`, implement tool dispatch for `tools/call`:
   - `ralph_status` — rebuild projections, return `StatusSummary` as JSON
   - `ralph_plan` — accept `{ description: string }`, run passthrough plan, return plan JSON
   - `ralph_run` — run pending tasks, return session summary JSON
   - `ralph_task_list` — return all tasks, optionally filtered by `status` or `epic_id`
   - `ralph_task_get` — accept `{ task_id: string }`, return single task view
2. Each tool handler:
   - Validates input parameters
   - Calls existing engine/application functions
   - Returns structured JSON result
3. Implement `tools/list` method returning tool schemas with JSON Schema input definitions
4. Tests per tool: mock request → verify response shape

## Files touched
- `crates/ralph-core/src/infrastructure/mcp_server.rs` (modify)

## Done when
- All 5 tools respond correctly to well-formed requests
- Invalid inputs return proper MCP error responses
- Tool list matches registered handlers
