# PRD-009: Git Worktree Parallel Isolation

## Overview
Enable parallel task execution by running each agent in an isolated git worktree. Currently the orchestrator runs tasks sequentially in the main working directory — this prevents concurrent agent sessions since they'd conflict on the filesystem. Git worktrees provide lightweight, isolated copies of the repo where agents can work independently.

## Goals
- Execute multiple tasks concurrently using `tokio::spawn`
- Isolate each agent's filesystem changes via `git worktree`
- Merge completed worktree changes back to the main branch
- Maintain event sourcing through the shared engine (thread-safe)

## Quality Gates

### Epic-Level (run once on epic completion)
- `cargo test` — all tests pass
- `cargo clippy -- -D warnings` — no warnings
- `cargo fmt --check` — formatting clean

### Story-Level (checked per story)
- Each story's acceptance criteria verified individually

## User Stories

### US-009-01: Worktree manager module [Infrastructure]
**Description:** As the orchestrator, I need to create and clean up git worktrees so each task runs in filesystem isolation.

**Acceptance Criteria:**
- [ ] `WorktreeManager` struct with `create(task_id) -> PathBuf` and `cleanup(task_id)` methods
- [ ] `create` runs `git worktree add .rewind/worktrees/<task-id> -b rewind/<task-id>`
- [ ] `cleanup` runs `git worktree remove` and deletes the branch
- [ ] `merge_back(task_id)` cherry-picks the worktree's commits onto the current branch
- [ ] Unit test: create/cleanup round-trip (mocked git commands)

Mark each item [x] as you complete it. Only close when all are checked.

### US-009-02: Parallel orchestrator execution [Infrastructure]
**Description:** As the orchestrator, I need to run multiple tasks concurrently using worktrees instead of sequentially in the main directory.

**Acceptance Criteria:**
- [ ] New `execute_parallel` method on Orchestrator that spawns tasks with `tokio::spawn`
- [ ] Each spawned task gets its own worktree via WorktreeManager
- [ ] `max_concurrent` limits parallelism via a `tokio::sync::Semaphore`
- [ ] Failed tasks don't block other running tasks
- [ ] Completed tasks get merged back via `merge_back`
- [ ] Events still flow through the shared engine (Arc<RewindEngine>)

Mark each item [x] as you complete it. Only close when all are checked.

### US-009-03: CLI --parallel flag [Backend]
**Description:** As a user, I want `rewind run --parallel` to use worktree-based parallel execution.

**Acceptance Criteria:**
- [ ] `--parallel` flag added to `run` subcommand
- [ ] When set, uses `execute_parallel` instead of `execute_runnable`
- [ ] Without flag, behavior unchanged (sequential execution)
- [ ] Progress output shows `[task-id]` prefix for each concurrent task's output

Mark each item [x] as you complete it. Only close when all are checked.

### US-009-04: Worktree merge conflict handling [Infrastructure]
**Description:** As the orchestrator, I need to handle merge conflicts when cherry-picking worktree changes back.

**Acceptance Criteria:**
- [ ] If cherry-pick fails with conflict, abort and mark task as failed with reason "merge conflict"
- [ ] Emit `TaskFailed` event with conflict details
- [ ] Clean up the worktree even on conflict
- [ ] Test: simulate conflict scenario

Mark each item [x] as you complete it. Only close when all are checked.

## Non-Goals
- Cross-worktree dependency resolution (tasks with deps still wait for predecessors)
- Shared build cache across worktrees
- Remote worktrees (only local git worktrees)

## Technical Considerations
- `RewindEngine` is already behind `Arc` in run.rs — thread-safe for parallel use
- The event store (allframe) must support concurrent appends — verify with `InMemoryBackend`
- Worktrees share `.git` directory, so concurrent git operations need care
- Each worktree gets its own `.rewind/data/` — but events should flow to the main engine, not per-worktree stores
