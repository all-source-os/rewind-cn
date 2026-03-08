# US-004-01: Add StartTask, StartSession, EndSession commands

**Parent:** US-004 (`rewind run`)
**Size:** S
**Depends on:** —

## Goal
Add the missing command structs, handlers, and engine methods needed to drive task execution lifecycle.

## Tasks
1. In `application/commands.rs`, add:
   ```rust
   pub struct StartTask { pub task_id: TaskId }
   pub struct StartSession {}
   pub struct EndSession { pub session_id: SessionId }
   ```
2. In `application/handlers.rs`, add:
   - `handle_start_task` → emits `TaskStarted`
   - `handle_start_session` → emits `SessionStarted` (generates SessionId)
   - `handle_end_session` → emits `SessionEnded`
3. In `infrastructure/command_bridge.rs`, add bridge newtypes + handlers:
   - `StartTaskCmd`, `StartSessionCmd`, `EndSessionCmd`
4. In `infrastructure/engine.rs`:
   - Register new handlers in `register_handlers()`
   - Add `start_task()`, `start_session()`, `end_session()` convenience methods
5. Tests:
   - `handle_start_task` emits `TaskStarted` with correct task_id
   - `handle_start_session` emits `SessionStarted` with generated session_id
   - Engine roundtrip: create → assign → start → complete

## Files touched
- `crates/rewind-cn-core/src/application/commands.rs` (modify)
- `crates/rewind-cn-core/src/application/handlers.rs` (modify)
- `crates/rewind-cn-core/src/infrastructure/command_bridge.rs` (modify)
- `crates/rewind-cn-core/src/infrastructure/engine.rs` (modify)

## Done when
- All new handler tests pass
- Engine roundtrip test covers full task lifecycle
