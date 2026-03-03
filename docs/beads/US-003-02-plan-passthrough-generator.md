# US-003-02: Passthrough plan generator (Phase 1)

**Parent:** US-003 (`ralph plan`)
**Size:** S
**Depends on:** US-003-01

## Goal
Implement the Phase 1 plan generator that creates one epic with one task from the raw input text. No LLM required.

## Tasks
1. Create `application/planning.rs` with:
   ```rust
   pub struct Plan {
       pub epic_title: String,
       pub epic_description: String,
       pub tasks: Vec<PlannedTask>,
   }

   pub struct PlannedTask {
       pub title: String,
       pub description: String,
   }

   /// Phase 1: wraps the input as a single epic + single task.
   pub fn passthrough_plan(input: &str) -> Plan;
   ```
2. `passthrough_plan` logic:
   - Epic title: first line of input (truncated to 80 chars)
   - Epic description: full input
   - Single task with same title/description
3. Unit tests: single-line input, multi-line input, long title truncation

## Files touched
- `crates/ralph-core/src/application/planning.rs` (create)
- `crates/ralph-core/src/application/mod.rs` (modify)

## Done when
- `passthrough_plan("Build auth\nNeeds OAuth + JWT")` returns epic with title "Build auth" and 1 task
- Tests pass
