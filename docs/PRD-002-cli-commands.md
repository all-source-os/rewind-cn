# PRD-002: CLI Command Implementation

## Overview

Implement the four stub CLI commands (`plan`, `run`, `status`, `mcp`) to make ralph a functional autonomous coding agent orchestrator. Each command builds on the CQRS engine and clean architecture already in place.

---

## US-002: `ralph status` — Project Status Dashboard

### Priority: P0 (prerequisite for validating all other commands)

### Description
Display the current project backlog, epic progress, and agent activity by reading from the engine's projections.

### Acceptance Criteria
1. Loads engine from `.ralph/data/`, rebuilds projections from event store
2. Prints a summary table:
   - Total tasks, by status (pending / assigned / in-progress / completed / failed / blocked)
   - Epic progress bars (completed_tasks / total_tasks per epic)
3. With `--json` flag, outputs the same data as JSON to stdout (for scripting / MCP consumption)
4. Exits with error if `.ralph/` doesn't exist (prompts user to run `ralph init`)

### CLI Interface
```
ralph status [--json]
```

### Implementation Notes
- Wire up `RalphEngine::load()` → `rebuild_projections()` → read `backlog()` and `epic_progress()`
- Add `Display` impl for `TaskStatus` (or a format helper)
- No new domain events or commands needed — read-only operation

---

## US-003: `ralph plan` — Generate Execution Plan

### Priority: P0

### Description
Accept a task description (from argument, stdin, or file), decompose it into an epic with tasks, and persist the plan to the event store.

### Acceptance Criteria
1. Accepts input via:
   - `ralph plan "Build user authentication"` (positional arg)
   - `ralph plan -f prd.md` (file input)
   - `echo "..." | ralph plan` (stdin, when no arg or `-f`)
2. **Phase 1 (no LLM):** Simple parsing — creates one epic with a single task matching the input text. This makes the pipeline testable end-to-end without an LLM dependency.
3. **Phase 2 (LLM decomposition):** When `llm.provider` is configured in `ralph.toml`, sends the input to the LLM with a system prompt that returns structured JSON: `{ epic_title, epic_description, tasks: [{ title, description }] }`. Parses the response and emits `EpicCreated` + N × `TaskCreated` events.
4. Prints the generated plan to stdout (epic title, numbered task list)
5. Events are persisted — `ralph status` reflects the new plan immediately

### CLI Interface
```
ralph plan [DESCRIPTION] [-f <file>] [--dry-run]
```
- `--dry-run`: print the plan but don't persist events

### New Domain Surface
- No new events needed — uses existing `CreateEpic` + `CreateTask` commands
- Consider adding a `PlanGenerated` session event (optional, for audit trail):
  ```rust
  PlanGenerated {
      session_id: SessionId,
      epic_id: EpicId,
      task_ids: Vec<TaskId>,
      source: String, // "manual" | "llm"
      generated_at: DateTime<Utc>,
  }
  ```

### LLM Integration (Phase 2)
- New crate or module: `ralph-core/src/infrastructure/llm.rs`
- Trait: `trait PlanGenerator { async fn decompose(&self, input: &str) -> Result<Plan, RalphError>; }`
- Implementations: `LlmPlanGenerator` (calls API), `PassthroughPlanGenerator` (Phase 1 fallback)
- Read provider/model/api_key_env from `RalphConfig`
- Support `anthropic` provider initially (Claude API via `reqwest`)

---

## US-004: `ralph run` — Execute Plan with Agent Workers

### Priority: P1

### Description
Pick up pending tasks from the backlog, assign them to agent workers, execute them (via LLM tool-use sessions), and record results as events.

### Acceptance Criteria
1. Loads engine, rebuilds projections, identifies pending tasks
2. Spawns up to `agents.max_concurrent` worker tasks (from config)
3. Each worker:
   a. Picks the next unassigned pending task (acquires via `AssignTask` command)
   b. Emits `TaskStarted` event
   c. **Phase 1 (no LLM):** Prints "Executing: {task_title}" and immediately completes with `TaskCompleted`
   d. **Phase 2 (LLM execution):** Sends the task to an LLM agent session with tool-use (file read/write/shell), streams output, records result
   e. On success: emits `TaskCompleted`
   f. On failure: emits `TaskFailed { reason }`
   g. On timeout (`agents.timeout_secs`): emits `TaskFailed { reason: "timeout" }`
4. Respects task ordering — blocked tasks are skipped until their blocker completes
5. Emits `SessionStarted` at run begin, `SessionEnded` at run end
6. Prints real-time progress: `[2/5] Completing: Fix login bug ✓`
7. `--task <task_id>` flag to run a single specific task
8. `--dry-run` flag to show what would execute without actually running

### CLI Interface
```
ralph run [--task <task_id>] [--dry-run] [--max-concurrent <n>]
```

### New Domain Surface
- New commands: `StartTask { task_id }`, `StartSession`, `EndSession`
- New handler functions in `application/handlers.rs`
- New engine methods: `start_task()`, `start_session()`, `end_session()`

### Agent Worker Architecture
- `ralph-core/src/infrastructure/agent.rs`:
  ```rust
  pub struct AgentWorker { agent_id: AgentId, config: AgentsConfig }
  impl AgentWorker {
      async fn execute_task(&self, task: &TaskView, engine: &RalphEngine<B>) -> Result<(), RalphError>;
  }
  ```
- Worker pool: `tokio::JoinSet` with semaphore for concurrency control
- Phase 1: mock execution (sleep + complete)
- Phase 2: LLM tool-use session per task

### Task Scheduling
- Simple greedy scheduler: iterate pending tasks in creation order, skip blocked
- No priority system in v1 — tasks within an epic execute in insertion order
- Future: dependency DAG, priority scores

---

## US-005: `ralph mcp` — MCP Server for IDE Integration

### Priority: P2

### Description
Start a Model Context Protocol (MCP) server that exposes ralph's CQRS surface as tools, enabling IDE integrations (VS Code, Claude Code, etc.) to interact with the orchestrator.

### Acceptance Criteria
1. Starts an MCP server on stdio transport (standard for CLI-based MCP servers)
2. Exposes the following tools:
   - `ralph_status` — returns backlog + epic progress as JSON
   - `ralph_plan` — accepts description, creates epic + tasks, returns plan
   - `ralph_run` — triggers execution, returns session summary
   - `ralph_task_list` — returns all tasks with filters (status, epic_id)
   - `ralph_task_get` — returns a single task's full details
3. Exposes resources:
   - `ralph://backlog` — live backlog state
   - `ralph://epics` — epic progress state
   - `ralph://config` — current ralph.toml config
4. Server runs until stdin closes (standard MCP lifecycle)
5. All tool calls go through the same engine/command path as CLI commands

### CLI Interface
```
ralph mcp
```
No flags — MCP servers are configured by the client (Claude Code, VS Code extension, etc.).

### Implementation Notes
- Use `allframe-mcp` if available, otherwise hand-roll with `serde_json` over stdio
- MCP protocol: JSON-RPC 2.0 over stdin/stdout
- Consider `rmcp` crate (Rust MCP SDK) or raw implementation
- Each tool handler reuses the same `RalphEngine` instance
- `ralph-core/src/mcp.rs` becomes the tool/resource registration point

### MCP Server Configuration (for clients)
```json
{
  "mcpServers": {
    "ralph": {
      "command": "ralph",
      "args": ["mcp"],
      "cwd": "/path/to/project"
    }
  }
}
```

---

## Implementation Order

```
US-002 (status)  →  US-003 (plan)  →  US-004 (run)  →  US-005 (mcp)
     P0                 P0                P1                P2
   read-only        write path       full pipeline      integration
```

Each US is independently shippable. Status is first because it validates the read path and is needed to verify plan/run output. Plan is next because run depends on having tasks in the backlog. MCP wraps everything built before it.

## Phase Strategy

| Phase | LLM Required | Scope |
|-------|-------------|-------|
| Phase 1 | No | All four commands work end-to-end with mock/passthrough logic |
| Phase 2 | Yes | Plan uses LLM decomposition, Run uses LLM agent sessions |

Phase 1 should be implemented first for all commands. This gives a complete testable pipeline without external dependencies. Phase 2 adds the LLM integration incrementally.

## Dependencies to Add (estimated)

| Crate | Purpose | US |
|-------|---------|-----|
| `comfy-table` or similar | Terminal table formatting | US-002 |
| `reqwest` | HTTP client for LLM APIs | US-003 Phase 2 |
| `rmcp` or raw JSON-RPC | MCP server protocol | US-005 |
| `indicatif` | Progress bars for run output | US-004 |
| `tokio::sync::Semaphore` | Agent concurrency control | US-004 |

## Out of Scope
- Web UI or dashboard
- Multi-project workspaces
- Remote agent execution (all agents run locally)
- Git integration (commit/PR creation) — future US
- Authentication or multi-user support
