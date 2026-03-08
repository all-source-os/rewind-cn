# US-003-03: Persist plan to event store and print output

**Parent:** US-003 (`rewind plan`)
**Size:** M
**Depends on:** US-003-01, US-003-02, US-002-02

## Goal
Wire the full `rewind plan` command: resolve input → generate plan → dispatch CreateEpic + CreateTask commands → print result.

## Tasks
1. In `commands/plan.rs`, implement `execute()`:
   - Check `.rewind/` exists
   - Resolve input (US-003-01)
   - Generate plan via `passthrough_plan()` (US-003-02)
   - Load engine
   - If not `--dry-run`:
     - `engine.create_epic(CreateEpic { title, description })`
     - Extract `epic_id` from the returned `EpicCreated` event
     - For each planned task: `engine.create_task(CreateTask { title, description, epic_id })`
   - Print plan to stdout:
     ```
     Epic: Build user authentication

       1. Build user authentication

     Created 1 epic with 1 task.
     ```
   - If `--dry-run`: print plan with "[dry run]" header, skip persistence
2. Integration test: plan → status shows the created epic and tasks

## Files touched
- `crates/rewind-cn/src/commands/plan.rs` (modify)

## Done when
- `rewind init && rewind plan "Fix login" && rewind status` shows 1 epic, 1 task
- `rewind plan --dry-run "Fix login" && rewind status` shows 0 tasks
- Events persisted correctly (verified via status)
