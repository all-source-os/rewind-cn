# ADR-001: Architecture Assessment and Refactoring Plan

**Date:** 2026-03-16
**Status:** Accepted
**Authors:** decebaldobrica, Claude

## Context

Rewind v0.2.1 has a solid CQRS + Event Sourcing foundation with 196+ tests, clean domain/application/infrastructure layering, and working CLI commands. However, several structural issues reduce confidence in releases:

1. **Zero E2E validation** — `rewind run` has never completed a real coding task.
2. **Test deadlocks** — 3 orchestrator tests hang when run concurrently due to allframe/tokio interaction.
3. **God modules** — `orchestrator.rs` (1040 lines) and `coder.rs` (1304 lines) mix too many concerns.
4. **TUI coupling** — Dashboard required runtime snapshots to show pre-existing tasks (fixed, but symptom of tight coupling between engine state and presentation).
5. **Hardcoded policy** — `ALLOWED_COMMANDS` in coder, `EVALUATOR_SYSTEM_PROMPT` inline, no configurability.
6. **No release gate** — No CI, no integration test suite that runs automatically.

## Decision

Adopt a phased refactoring plan organized around **increasing release confidence** rather than cosmetic cleanup. Each phase is independently shippable and adds measurable value.

---

## Phase 1: Fix the Test Suite (Confidence: tests mean something)

**Problem:** 3 orchestrator tests deadlock when run together. `cargo test` cannot be trusted as a gate.

**Root cause:** `rebuild_projections()` acquires 3 `RwLock` write guards sequentially (`backlog`, `epic_progress`, `analytics`) within a single async function. When multiple `#[tokio::test]` instances share a thread pool, allframe's `CommandBus::dispatch` can block on the same locks from within `dispatch_and_append`, causing a deadlock cycle.

**Refactoring:**

1. **Collapse projection locks into a single `RwLock<Projections>` struct:**
   ```rust
   struct Projections {
       backlog: BacklogProjection,
       epic_progress: EpicProgressProjection,
       analytics: AnalyticsProjection,
   }
   ```
   One lock, one acquisition, zero deadlock risk. `apply_to_projections` and `rebuild_projections` both acquire this single lock.

2. **Add `#[tokio::test(flavor = "multi_thread")]`** to the 3 orchestrator-loop tests as an immediate fix while the lock refactor is in progress.

3. **Add a CI script** (`scripts/ci.sh` or `Taskfile.yml`) that runs:
   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   ```
   This becomes the release gate.

**Exit criteria:** `cargo test` passes 100% of the time on 10 consecutive runs.

---

## Phase 2: E2E Smoke Test (Confidence: the agent loop works)

**Problem:** The core value prop (`rewind run`) has never been validated against a real LLM.

**Refactoring:**

1. **Create `tests/e2e_smoke.rs`** — a single integration test that:
   - Creates an in-memory engine
   - Imports a tiny epic (1 task: "create a file `hello.txt` with content `hello`")
   - Runs the orchestrator with `ClaudeCodeExecutor` (or a recorded replay)
   - Asserts task reaches `Completed` status

2. **Record/replay adapter** — wrap `TaskExecutor` with a recording layer:
   ```rust
   struct RecordingExecutor { inner: Box<dyn TaskExecutor>, recordings_dir: PathBuf }
   struct ReplayExecutor { recordings_dir: PathBuf }
   ```
   First run records LLM responses to disk. Subsequent runs replay without API calls. This gives deterministic CI tests without API keys.

3. **Acceptance test fixture** — `tests/fixtures/smoke-epic.jsonl` with a trivial task that any LLM can complete. Run manually once to record, then replay in CI forever.

**Exit criteria:** `cargo test e2e_smoke` passes in CI without API keys (replay mode).

---

## Phase 3: Split God Modules (Confidence: code is reviewable)

**Problem:** `orchestrator.rs` (1040 lines) owns task selection, execution, evaluation, retry logic, gate running, chronis sync, worktree coordination, and event emission. `coder.rs` (1304 lines) mixes LLM agent construction, tool definitions, path validation, and command execution.

**Refactoring:**

### 3a. Extract `infrastructure/tools/` from `coder.rs`

```
infrastructure/
  tools/
    mod.rs          # re-exports
    read_file.rs    # ReadFileTool
    edit_file.rs    # EditFileTool
    run_command.rs  # RunCommandTool + ALLOWED_COMMANDS
    list_files.rs   # ListFilesTool
    search_code.rs  # SearchCodeTool
    write_file.rs   # WriteFileTool
    validation.rs   # validate_path, validate_command, simple_glob_match
  coder.rs          # slimmed: prompt building + agent construction only
```

Each tool file is <100 lines. `coder.rs` drops from 1304 to ~300 lines.

### 3b. Extract orchestrator concerns

```
infrastructure/
  orchestrator/
    mod.rs            # re-exports Orchestrator
    task_runner.rs    # execute_task_impl (single task lifecycle)
    retry_loop.rs     # execute_runnable (batch loop with retry)
    parallel.rs       # execute_parallel (worktree-based)
    chronis_sync.rs   # claim/done/fail bridge calls
```

Each file owns one concern. The orchestrator struct stays, but methods are organized by file.

**Exit criteria:** No file in `infrastructure/` exceeds 400 lines. `cargo test` still passes.

---

## Phase 4: Configuration over Convention (Confidence: users can adapt)

**Problem:** Hardcoded values prevent users from customizing behavior without forking.

**Refactoring:**

1. **`ALLOWED_COMMANDS` → `rewind.toml`:**
   ```toml
   [execution]
   allowed_commands = ["cargo", "git", "make", "task", "bun", "npm"]
   ```
   Default populated on `rewind init`. Coder reads from config at runtime.

2. **Evaluator prompt → template file:**
   ```
   .rewind/templates/evaluator.tera    # customizable
   .rewind/templates/coder.tera        # already supported via prompt_template
   ```
   Fall back to embedded defaults if files don't exist.

3. **Gate tiers → explicit config:**
   ```toml
   [gates.story]
   commands = []  # none by default, user adds per-project

   [gates.epic]
   commands = ["cargo test", "cargo clippy"]
   ```
   Already partially supported in `GateConfig`, but not fully wired.

**Exit criteria:** A fresh `rewind init` project can customize allowed commands and evaluator prompt without code changes.

---

## Phase 5: Projection Decoupling (Confidence: TUI and MCP are reliable)

**Problem:** TUI required a backlog snapshot workaround because it subscribes after events are emitted. MCP server directly imports application modules. Both are tightly coupled to engine internals.

**Refactoring:**

1. **Event replay on subscribe** — when a new broadcast subscriber connects, replay existing events from the event store so latecomers see full state. This eliminates the `seed_from_backlog` workaround:
   ```rust
   pub fn subscribe_with_replay(&self) -> broadcast::Receiver<RewindEvent> {
       let rx = self.event_tx.subscribe();
       // Replay existing events through the channel
       // (or provide an initial snapshot alongside the receiver)
       rx
   }
   ```

2. **Read-only projection handle** — instead of exposing `Arc<RwLock<BacklogProjection>>`, expose a read-only snapshot:
   ```rust
   pub async fn backlog_snapshot(&self) -> BacklogProjection {
       self.backlog.read().await.clone()
   }
   ```
   This prevents consumers from accidentally holding write locks.

3. **MCP query layer** — extract MCP tool handlers into `application/queries.rs` that operate on projection snapshots, not engine references. This keeps the MCP server as a thin transport layer.

**Exit criteria:** TUI and MCP server have zero direct `RwLock` access. All reads go through snapshot methods.

---

## Phase 6: Dependency Hygiene (Confidence: upgrades don't break things)

1. **Pin allframe** — currently `0.1`, pre-release. Pin exact version in `Cargo.toml` to prevent surprise breaking changes.
2. **Pin rig-core** — `0.32` is stable but the API surface changes frequently. Pin exact.
3. **Audit `hotpath`** — used for profiling instrumentation but the `#[hotpath::measure]` macros add overhead even when not profiling. Make it compile-time optional (already feature-gated, verify it's truly no-op when disabled).
4. **Remove unused deps** — run `cargo machete` to identify dead dependencies.

**Exit criteria:** `Cargo.lock` is committed. `cargo update` doesn't silently change behavior.

---

## Consequences

### Positive
- `cargo test` becomes a trustworthy release gate (Phase 1)
- E2E smoke test catches agent loop regressions without API keys (Phase 2)
- Code review becomes tractable — no 1000+ line files (Phase 3)
- Users can customize without forking (Phase 4)
- TUI/MCP consumers can't deadlock the engine (Phase 5)
- Upgrades are explicit and auditable (Phase 6)

### Negative
- Phase 3 (module split) touches many files and will cause merge conflicts with in-flight work
- Phase 2 (record/replay) adds test infrastructure that itself needs maintenance
- Phase 5 (snapshot API) is a breaking change for any code holding `Arc<RwLock<_>>` references

### Trade-offs
- Phases are ordered by ROI: 1 and 2 give the most confidence with the least churn
- Phase 3 is the riskiest but also the most impactful for long-term maintainability
- Phases 4-6 can be deferred without blocking releases

---

## Priority Matrix

| Phase | Effort | Impact on Release Confidence | Recommended Timing |
|-------|--------|-----------------------------|--------------------|
| 1. Fix test suite | Small (1-2 days) | Critical — unblocks everything | Immediate |
| 2. E2E smoke test | Medium (2-3 days) | Critical — validates core value prop | This week |
| 3. Split god modules | Medium (3-5 days) | High — enables safe parallel development | Next sprint |
| 4. Configuration | Small (2-3 days) | Medium — user adoption | After Phase 3 |
| 5. Projection decoupling | Medium (2-3 days) | Medium — prevents runtime bugs | After Phase 3 |
| 6. Dependency hygiene | Small (1 day) | Low — prevents surprise breakage | Anytime |
