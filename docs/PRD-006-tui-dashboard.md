# PRD-006: TUI Dashboard

## Overview
Add a terminal UI dashboard for real-time monitoring of agent execution. Currently `rewind run` prints sequential log lines — this replaces that with a rich TUI showing task progress, agent activity, and quality gate results using `ratatui`.

## Goals
- Real-time visual feedback during `rewind run`
- Show all tasks with status, progress bars, and acceptance criteria
- Display live agent tool calls as they happen
- Show epic-level progress and quality gate results

## Quality Gates

### Epic-Level (run once on epic completion)
- `cargo test` — all tests pass
- `cargo clippy -- -D warnings` — no warnings
- `cargo fmt --check` — formatting clean

### Story-Level (checked per story)
- Each story's acceptance criteria verified individually

## User Stories

### US-006-01: Ratatui scaffold and app state [Infrastructure]
**Description:** As a developer, I need the ratatui dependency and basic app state struct so the TUI can render.

**Acceptance Criteria:**
- [ ] `ratatui` and `crossterm` added to rewind-cn Cargo.toml
- [ ] `tui/` module in CLI crate with `app.rs`, `ui.rs`, `mod.rs`
- [ ] `App` struct holds: tasks (Vec<TaskState>), epic progress, selected index, log messages
- [ ] `TaskState` mirrors TaskView with additional TUI fields (progress_pct, last_tool_call)
- [ ] Basic render loop: init terminal, draw, handle input, restore terminal
- [ ] 'q' key quits the dashboard

Mark each item [x] as you complete it. Only close when all are checked.

### US-006-02: Task list panel [UI]
**Description:** As a user, I want to see all tasks with their status so I can track execution progress.

**Acceptance Criteria:**
- [ ] Left panel shows task list with status icons (pending ○, running ◉, done ✓, failed ✗)
- [ ] Each task shows: title, status, acceptance criteria progress (2/5)
- [ ] Running tasks highlighted with accent color
- [ ] Arrow keys navigate task selection
- [ ] Selected task shows details in right panel

Mark each item [x] as you complete it. Only close when all are checked.

### US-006-03: Task detail panel [UI]
**Description:** As a user, I want to see task details including acceptance criteria checkboxes and recent tool calls.

**Acceptance Criteria:**
- [ ] Right panel shows selected task's full details
- [ ] Acceptance criteria shown as checkboxes: [x] done, [ ] pending
- [ ] Recent tool calls shown below criteria (tool name, args summary, timestamp)
- [ ] Scrollable if content exceeds panel height

Mark each item [x] as you complete it. Only close when all are checked.

### US-006-04: Live event updates [Infrastructure]
**Description:** As a user, I want the dashboard to update in real-time as agents execute tasks.

**Acceptance Criteria:**
- [ ] `EventChannel` using `tokio::sync::broadcast` for real-time event distribution
- [ ] Engine emits events to channel after persisting
- [ ] Dashboard subscribes to channel and updates App state on each event
- [ ] UI redraws on event receipt (not polling)
- [ ] TaskStarted/Completed/Failed update task status immediately
- [ ] AgentToolCall appends to task's tool call log
- [ ] CriterionChecked updates criteria checkbox

Mark each item [x] as you complete it. Only close when all are checked.

### US-006-05: Epic progress bar and summary [UI]
**Description:** As a user, I want to see overall epic progress and quality gate results.

**Acceptance Criteria:**
- [ ] Top bar shows epic title and progress: "Feature X [████░░░░░░] 40% (4/10 tasks)"
- [ ] Bottom bar shows session info: duration, tasks completed/failed
- [ ] After epic gates run, show gate results (pass/fail with command)

Mark each item [x] as you complete it. Only close when all are checked.

### US-006-06: Dashboard CLI integration [Backend]
**Description:** As a user, I want `rewind run --tui` to show the dashboard during execution.

**Acceptance Criteria:**
- [ ] `--tui` flag added to `run` subcommand
- [ ] When set, starts dashboard and runs orchestrator concurrently
- [ ] Dashboard exits when orchestrator completes (or on 'q')
- [ ] Without `--tui`, behavior unchanged (log output)
- [ ] Works with both sequential and parallel execution modes

Mark each item [x] as you complete it. Only close when all are checked.

## Non-Goals
- Mouse support
- Persistent dashboard (only during `rewind run`)
- Configuration of layout/colors
- Log file export from TUI

## Technical Considerations
- `ratatui` uses `crossterm` backend for terminal control
- Need to suppress regular stderr/stdout output when TUI is active
- Dashboard runs in main thread, orchestrator in spawned task
- Event channel must be bounded to prevent memory issues during long runs
- Terminal must be restored on panic (use `std::panic::set_hook`)
