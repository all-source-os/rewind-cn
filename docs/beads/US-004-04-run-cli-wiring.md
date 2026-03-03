# US-004-04: Wire `ralph run` CLI command

**Parent:** US-004 (`ralph run`)
**Size:** M
**Depends on:** US-004-01, US-004-02, US-004-03, US-003-03

## Goal
Implement the `ralph run` command end-to-end: load engine, schedule tasks, spawn workers, print progress.

## Tasks
1. Update `Commands::Run` in `main.rs` with clap args:
   ```rust
   Run {
       #[arg(long)]
       task: Option<String>,
       #[arg(long)]
       dry_run: bool,
       #[arg(long)]
       max_concurrent: Option<usize>,
   }
   ```
2. Implement `commands/run.rs`:
   - Check `.ralph/` exists
   - Load engine, rebuild projections
   - `engine.start_session()` → capture session_id
   - Determine tasks to run:
     - If `--task`: find that specific task, error if not found/not pending
     - Else: `pick_runnable_tasks(backlog, max_concurrent)`
   - If `--dry-run`: print task list and exit
   - Spawn workers using `tokio::JoinSet`:
     - Semaphore with `max_concurrent` permits (from config or `--max-concurrent`)
     - Each worker calls `AgentWorker::execute_task()`
   - Print progress as tasks complete:
     ```
     Session started: sess-abc123
     [1/3] Executing: Fix login bug... done
     [2/3] Executing: Add tests... done
     [3/3] Executing: Update docs... done
     Session complete: 3 tasks executed (3 passed, 0 failed)
     ```
   - `engine.end_session()`
   - If any task failed: exit with code 1
3. Timeout handling:
   - Wrap each worker in `tokio::time::timeout(Duration::from_secs(config.agents.timeout_secs))`
   - On timeout: emit `TaskFailed { reason: "timeout" }`

## Files touched
- `crates/ralph-cli/src/main.rs` (modify)
- `crates/ralph-cli/src/commands/run.rs` (rewrite)

## Done when
- `ralph init && ralph plan "Fix bug" && ralph run` completes successfully
- `ralph status` after run shows task as Completed
- `ralph run --dry-run` lists tasks without executing
- `ralph run` with no pending tasks prints "No pending tasks to run"
