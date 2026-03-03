# US-002-01: Display impl and status formatting helpers

**Parent:** US-002 (`ralph status`)
**Size:** S
**Depends on:** —

## Goal
Add `Display` for `TaskStatus` and `EpicStatus`, plus a `StatusSummary` struct that aggregates counts from projections into a printable/serializable form.

## Tasks
1. In `domain/model.rs`, add `impl fmt::Display for TaskStatus` (Pending → "pending", etc.)
2. In `domain/model.rs`, add `impl fmt::Display for EpicStatus`
3. Create `application/status.rs` with:
   ```rust
   #[derive(Debug, Serialize)]
   pub struct StatusSummary {
       pub total_tasks: usize,
       pub by_status: HashMap<String, usize>,
       pub epics: Vec<EpicSummary>,
   }

   #[derive(Debug, Serialize)]
   pub struct EpicSummary {
       pub epic_id: String,
       pub title: String,
       pub total_tasks: usize,
       pub completed_tasks: usize,
       pub is_completed: bool,
   }

   pub fn build_summary(backlog: &BacklogProjection, epics: &EpicProgressProjection) -> StatusSummary;
   ```
4. Add `serde` derive to `StatusSummary` / `EpicSummary` for JSON output
5. Unit test: given a backlog with mixed statuses, `build_summary` returns correct counts

## Files touched
- `crates/ralph-core/src/domain/model.rs` (modify)
- `crates/ralph-core/src/application/status.rs` (create)
- `crates/ralph-core/src/application/mod.rs` (modify)

## Done when
- `cargo test` passes with new summary tests
- `StatusSummary` can be serialized to JSON
