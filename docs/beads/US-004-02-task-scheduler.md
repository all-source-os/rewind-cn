# US-004-02: Task scheduler — pick next runnable tasks

**Parent:** US-004 (`ralph run`)
**Size:** S
**Depends on:** US-004-01

## Goal
Implement a scheduler that selects the next batch of runnable tasks from the backlog (pending, not blocked).

## Tasks
1. Create `application/scheduler.rs`:
   ```rust
   use crate::domain::model::{BacklogProjection, TaskView, TaskStatus};

   /// Returns pending tasks in creation order, excluding blocked tasks.
   pub fn pick_runnable_tasks(backlog: &BacklogProjection, max: usize) -> Vec<&TaskView>;
   ```
2. Logic:
   - Filter tasks where `status == Pending`
   - Sort by `created_at` ascending (FIFO)
   - Take up to `max`
   - (Blocked tasks already have status `Blocked`, so the filter excludes them)
3. Tests:
   - Empty backlog → empty result
   - Mix of pending/completed/blocked → only pending returned
   - Respects max limit
   - Returned in creation order

## Files touched
- `crates/ralph-core/src/application/scheduler.rs` (create)
- `crates/ralph-core/src/application/mod.rs` (modify)

## Done when
- All scheduler tests pass
- Scheduler is a pure function with no side effects
