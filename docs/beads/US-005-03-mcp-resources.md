# US-005-03: MCP resource handlers

**Parent:** US-005 (`rewind mcp`)
**Size:** S
**Depends on:** US-005-01

## Goal
Expose rewind state as MCP resources for IDE clients to read.

## Tasks
1. In `infrastructure/mcp_server.rs`, implement resource dispatch for `resources/read`:
   - `rewind://backlog` — returns serialized `BacklogProjection` (all task views as JSON)
   - `rewind://epics` — returns serialized `EpicProgressProjection` (all epic progress as JSON)
   - `rewind://config` — reads and returns `.rewind/rewind.toml` as text
2. Implement `resources/list` method returning resource metadata (uri, name, description, mimeType)
3. Add `Serialize` derives to `TaskView`, `EpicProgress` if not already present
4. Test: request each resource, verify JSON structure

## Files touched
- `crates/rewind-cn-core/src/infrastructure/mcp_server.rs` (modify)
- `crates/rewind-cn-core/src/domain/model.rs` (modify — add Serialize if needed)

## Done when
- All 3 resources return valid JSON/text
- Resource list includes all registered resources with correct URIs
