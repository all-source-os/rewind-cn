# US-004-03: Agent worker — Phase 1 mock execution

**Parent:** US-004 (`rewind run`)
**Size:** M
**Depends on:** US-004-01, US-004-02

## Goal
Implement the agent worker that picks a task, transitions it through the lifecycle (assign → start → complete/fail), using mock execution in Phase 1.

## Tasks
1. Create `infrastructure/agent.rs`:
   ```rust
   pub struct AgentWorker {
       pub agent_id: AgentId,
   }

   impl AgentWorker {
       pub fn new() -> Self; // generates AgentId

       /// Execute a single task. Phase 1: immediately completes.
       pub async fn execute_task<B: EventStoreBackend<RewindEvent>>(
           &self,
           task_id: TaskId,
           task_title: &str,
           engine: &RewindEngine<B>,
       ) -> Result<(), RewindError>;
   }
   ```
2. `execute_task` flow:
   - `engine.assign_task(AssignTask { task_id, agent_id })`
   - `engine.start_task(StartTask { task_id })`
   - Phase 1: no-op (just log "Executing: {title}")
   - `engine.complete_task(CompleteTask { task_id })`
   - On any error: `engine.fail_task(FailTask { task_id, reason })`
3. Test: create task → agent executes → verify task is Completed in projections

## Files touched
- `crates/rewind-cn-core/src/infrastructure/agent.rs` (create)
- `crates/rewind-cn-core/src/infrastructure/mod.rs` (modify)

## Done when
- Agent worker test passes: task goes Pending → Assigned → InProgress → Completed
- Error path test: simulated failure → task ends in Failed state
