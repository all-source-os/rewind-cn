# PRD-003: Agent Orchestration — LLM-Powered Plan & Execute

## Overview

Replace the Phase 1 stubs in rewind-cn with real LLM-powered agent orchestration using rig-core. This makes `rewind plan` decompose PRDs into stories with acceptance criteria and two-tier quality gates, and `rewind run` spawn agent sessions that code, verify criteria, and complete tasks autonomously.

This PRD implements the core of [all-source#77](https://github.com/all-source-os/all-source/issues/77) — the rig-core agent layer, planner/coder/evaluator agents, and the SELECT→PROMPT→EXECUTE→EVALUATE loop.

## Goals

- `rewind plan "description"` decomposes input into epic + N tasks via rig planner agent
- `rewind plan` with no args enters interactive mode (asks clarifying questions)
- `rewind run` spawns rig coder agents that read/write files, run commands, and verify acceptance criteria
- Two-tier quality gates: story-level checked per task, epic-level run once on completion
- Task dependencies block execution until predecessors complete
- Acceptance criteria tracked as `- [ ]` checkboxes that agents mark `- [x]`
- All agent tool calls recorded as events for full audit trail

## Quality Gates

### Epic-Level (run once on epic completion)
- `cargo test` — all tests pass (29+ existing + new)
- `cargo clippy -- -D warnings` — zero warnings
- `cargo fmt --all -- --check` — formatting clean

### Story-Level (checked per story)
- **Domain stories:** `cargo check` compiles, new types have tests
- **Infrastructure stories:** integration test exercises the new code path
- **CLI stories:** `cargo check` compiles, command parses correctly

## User Stories

### US-003-01: Add rig-core dependency and LLM config [Infrastructure]
**Description:** As a developer, I want rig-core wired into rewind-cn-core so that agents can make LLM calls through a structured framework.

**Acceptance Criteria:**
- [ ] `rig-core` added to `rewind-cn-core/Cargo.toml` with `anthropic` feature
- [ ] `infrastructure/llm.rs` module created with `create_anthropic_client()` that reads API key from env var specified in config
- [ ] `RewindConfig` extended with `[agent]` section: `provider`, `planner.model`, `coder.model`, `evaluator.model`
- [ ] Config parsing tested: loading a `rewind.toml` with `[agent]` section produces correct `AgentConfig`
- [ ] `cargo check` passes with new dependency

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-02: Domain model — acceptance criteria and quality gates [Domain]
**Description:** As a developer, I want tasks to have acceptance criteria checkboxes and quality gates so that agents can verify and track completion.

**Acceptance Criteria:**
- [ ] `AcceptanceCriterion` struct added: `{ description: String, checked: bool }`
- [ ] `QualityGate` struct added: `{ command: String, tier: GateTier }` where `GateTier` is `Epic` or `Story`
- [ ] `TaskCreated` event extended with `acceptance_criteria: Vec<AcceptanceCriterion>` and `story_type: Option<StoryType>`
- [ ] `StoryType` enum: `Schema`, `Backend`, `UI`, `Integration`, `Infrastructure`
- [ ] `EpicCreated` event extended with `quality_gates: Vec<QualityGate>`
- [ ] New events: `CriterionChecked { task_id, criterion_index }`, `QualityGateRan { epic_id, command, passed, output }`
- [ ] `BacklogProjection` tracks criteria completion per task
- [ ] All existing tests updated and passing with new fields (use `Default` for backwards compat)

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-03: Domain model — task dependencies [Domain]
**Description:** As a developer, I want tasks to declare dependencies so that the scheduler only runs unblocked tasks.

**Acceptance Criteria:**
- [ ] `TaskCreated` event extended with `depends_on: Vec<TaskId>`
- [ ] `BacklogProjection` tracks dependency graph
- [ ] `BacklogProjection::is_blocked(task_id)` returns true if any dependency is not `Completed`
- [ ] `pick_runnable_tasks()` in scheduler filters out blocked tasks
- [ ] Test: task B depends on task A → B not picked until A completes → after A completes, B is pickable

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-04: Planner agent — PRD decomposition [Infrastructure]
**Description:** As a developer, I want a rig planner agent that decomposes a description into an epic with stories, acceptance criteria, dependencies, and two-tier quality gates.

**Acceptance Criteria:**
- [ ] `infrastructure/planner.rs` module created with `PlannerAgent` struct
- [ ] `PlannerAgent` uses rig-core's `Agent` with structured extraction to return a `Plan` struct
- [ ] `Plan` struct: `{ epic_title, epic_description, quality_gates: Vec<QualityGate>, stories: Vec<PlannedStory> }`
- [ ] `PlannedStory` struct: `{ title, description, story_type, acceptance_criteria: Vec<String>, depends_on: Vec<usize> }`
- [ ] System prompt instructs the LLM to produce right-sized stories with verifiable criteria and two-tier gate classification
- [ ] `PassthroughPlanGenerator` kept as fallback when no LLM config present
- [ ] `PlanGenerator` trait: `async fn decompose(&self, input: &str) -> Result<Plan, RewindError>`
- [ ] Test with mock LLM response: structured Plan parsed correctly from JSON

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-05: Interactive plan mode [CLI]
**Description:** As a developer, I want `rewind plan` with no arguments to enter interactive mode where the planner asks clarifying questions before generating the plan.

**Acceptance Criteria:**
- [ ] `rewind plan` with no args, no file, no stdin → enters interactive mode
- [ ] Interactive mode sends user's initial description to planner with `interactive: true` flag
- [ ] Planner responds with clarifying questions; user answers are fed back in a conversation loop
- [ ] Loop ends when planner returns a `[PLAN_READY]` marker, then generates the plan
- [ ] Non-interactive mode (`rewind plan "desc"` or `rewind plan -f file`) unchanged — single-shot decomposition
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-06: Coder agent — tool-use execution [Infrastructure]
**Description:** As a developer, I want a rig coder agent with file and shell tools that executes tasks by reading/writing code and running commands.

**Acceptance Criteria:**
- [ ] `infrastructure/coder.rs` module created with `CoderAgent` struct
- [ ] Rig tools implemented: `ReadFile`, `WriteFile`, `ListFiles`, `SearchCode`, `RunCommand`
- [ ] `RunCommand` tool has configurable timeout from `agents.timeout_secs` config
- [ ] Agent system prompt includes: task title, description, acceptance criteria as `- [ ]` list, instruction to mark `- [x]` when verified
- [ ] Each tool call emits `AgentToolCall` event: `{ task_id, tool_name, args_summary, result_summary }`
- [ ] Agent runs in a loop until it declares all criteria checked or hits max iterations
- [ ] Test: mock agent session with tool calls → events emitted in correct order

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-07: Evaluator agent — completion judgment [Infrastructure]
**Description:** As a developer, I want an evaluator agent that judges whether a task's acceptance criteria are actually met, dispatching complete or fail.

**Acceptance Criteria:**
- [ ] `infrastructure/evaluator.rs` module created with `EvaluatorAgent` struct
- [ ] Evaluator receives: task description, acceptance criteria, coder agent's output/tool call log, files changed
- [ ] Uses rig structured extraction to return `{ passed: bool, criteria_results: Vec<{ index, passed, reason }>, summary: String }`
- [ ] If passed: dispatches `CompleteTask` command
- [ ] If failed: dispatches `FailTask` command with reason
- [ ] Evaluator uses a cheaper/faster model (configurable, default `claude-haiku-4-5-20251001`)
- [ ] Test: evaluator correctly identifies passed vs failed criteria from mock agent output

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-08: Orchestrator loop — SELECT→PROMPT→EXECUTE→EVALUATE [Infrastructure]
**Description:** As a developer, I want the full orchestration loop that picks tasks, prompts agents, executes, and evaluates — replacing the mock `AgentWorker`.

**Acceptance Criteria:**
- [ ] `infrastructure/orchestrator.rs` module created with `Orchestrator` struct
- [ ] SELECT: queries `BacklogProjection` for unblocked pending tasks, respects `max_concurrent`
- [ ] PROMPT: builds coder agent context from task + acceptance criteria + project context
- [ ] EXECUTE: runs `CoderAgent` with tool-use loop, streams output
- [ ] EVALUATE: runs `EvaluatorAgent` on coder output, dispatches complete/fail
- [ ] Failed tasks increment `retry_count`; re-queued up to `execution.max_retries` (default 2)
- [ ] `AgentWorker` updated to use `Orchestrator` instead of mock execution
- [ ] Progress output: `[2/5] Executing: US-003 — Add auth middleware... ✓` (or `✗` on fail)
- [ ] Test: full loop with mock LLM → task goes pending→assigned→started→completed

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-09: Epic quality gate runner [Infrastructure]
**Description:** As a developer, I want epic-level quality gates to run automatically when all tasks in an epic complete, determining if the epic passes or needs fixes.

**Acceptance Criteria:**
- [ ] `infrastructure/gate_runner.rs` module created with `QualityGateRunner`
- [ ] After last task in epic completes, orchestrator triggers gate runner
- [ ] Gate runner executes each epic-level `QualityGate.command` as a shell command
- [ ] Each gate emits `QualityGateRan { epic_id, command, passed, output }` event
- [ ] All gates pass → `EpicCompleted` event emitted
- [ ] Any gate fails → new fix-up task created with failure output as context, epic stays open
- [ ] Test: epic with 2 tasks + 1 gate → both tasks complete → gate runs → epic completes (or fix-up task created)

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-10: Wire `rewind plan` to planner agent [CLI]
**Description:** As a developer, I want `rewind plan` to use the planner agent when LLM config is present, falling back to passthrough when not configured.

**Acceptance Criteria:**
- [ ] `commands/plan.rs` checks for `[agent]` config section
- [ ] If present: uses `PlannerAgent` for decomposition
- [ ] If absent: uses `PassthroughPlanGenerator` (existing behavior)
- [ ] Generated plan printed to stdout: epic title, numbered stories with criteria
- [ ] `--dry-run` still works (prints plan without persisting)
- [ ] Events persisted include acceptance criteria, dependencies, quality gates
- [ ] `rewind status` shows the decomposed plan with criteria counts

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-11: Wire `rewind run` to orchestrator [CLI]
**Description:** As a developer, I want `rewind run` to use the orchestrator loop with real agent execution when LLM config is present.

**Acceptance Criteria:**
- [ ] `commands/run.rs` checks for `[agent]` config section
- [ ] If present: uses `Orchestrator` for task execution
- [ ] If absent: uses mock execution (existing Phase 1 behavior)
- [ ] `--task <id>` runs a single specific task through the orchestrator
- [ ] `--dry-run` shows which tasks would execute and in what order (respecting deps)
- [ ] `--max-concurrent <n>` controls parallel agent sessions via `tokio::JoinSet` + `Semaphore`
- [ ] Real-time progress printed to stderr, final summary to stdout

Mark each item [x] as you complete it. Only close when all are checked.

### US-003-12: Chronis bridge — sync criteria and gates [Integration]
**Description:** As a developer, I want the chronis bridge to sync acceptance criteria status and quality gate results back to `cn` tasks.

**Acceptance Criteria:**
- [ ] `ChronisBridge::claim()` includes acceptance criteria in task description
- [ ] `ChronisBridge::done()` includes criteria results summary
- [ ] `ChronisBridge::fail()` includes which criteria failed and why
- [ ] Gate results synced as comments or status updates on the chronis epic
- [ ] Bridge remains best-effort (failures logged, not fatal)
- [ ] Existing chronis tests still pass

Mark each item [x] as you complete it. Only close when all are checked.

## Functional Requirements

- FR-1: `rewind plan` with LLM config decomposes input into 3-10 right-sized stories with verifiable acceptance criteria
- FR-2: `rewind plan` without LLM config falls back to passthrough (1 epic, 1 task)
- FR-3: `rewind plan` with no args enters interactive clarifying-questions mode
- FR-4: `rewind run` executes the SELECT→PROMPT→EXECUTE→EVALUATE loop per task
- FR-5: Coder agent has tools: ReadFile, WriteFile, ListFiles, SearchCode, RunCommand
- FR-6: Evaluator agent judges completion against acceptance criteria
- FR-7: Failed tasks retry up to `max_retries` before being marked permanently failed
- FR-8: Task dependencies block scheduling until predecessors complete
- FR-9: Epic-level quality gates run once when all tasks complete
- FR-10: Failed quality gates create fix-up tasks automatically
- FR-11: All tool calls emit `AgentToolCall` events for audit trail
- FR-12: Chronis bridge syncs criteria/gate results when tracker is configured

## Non-Goals (Out of Scope)

- TUI dashboard (US-006 in issue #77 — separate PRD)
- Git worktree parallel execution (US-009 in issue #77 — separate PRD)
- Multi-provider agent config (US-012 — anthropic only for now)
- Analytics and EventQL queries (US-008 — separate PRD)
- Import/export from JSON/Beads format (US-011 — separate PRD)
- `rewind resume` command — sessions are not resumable in v1
- Sandboxing/containerization of agent tool calls

## Technical Considerations

- **rig-core version**: Use latest stable (0.31+) with `anthropic` feature
- **Structured extraction**: Planner returns `Plan` struct via rig's `Extractor` agent — no manual JSON parsing
- **Tool call events**: Use `tracing` spans for real-time output, persist `AgentToolCall` events for replay
- **Token budget**: Coder agent context includes task description + criteria + relevant file contents. Keep under model context limit by summarizing large files.
- **Backwards compatibility**: All new event fields use `Option<T>` or `#[serde(default)]` so existing event stores replay without errors
- **Config migration**: `[llm]` section renamed to `[agent]` with sub-sections. Old `[llm]` section still parsed for backwards compat.

## Implementation Order

```
US-003-01 (rig dep)
    ↓
US-003-02 (criteria/gates domain) ──→ US-003-03 (dependencies domain)
    ↓                                      ↓
US-003-04 (planner agent)            US-003-08 (orchestrator) ←── US-003-06 (coder) + US-003-07 (evaluator)
    ↓                                      ↓
US-003-05 (interactive plan)         US-003-09 (gate runner)
    ↓                                      ↓
US-003-10 (plan CLI wiring)          US-003-11 (run CLI wiring)
                                           ↓
                                     US-003-12 (chronis sync)
```

## Dependencies to Add

| Crate | Version | Purpose |
|-------|---------|---------|
| `rig-core` | `0.31+` | LLM agent framework with tool-use |

## Config Changes

```toml
# rewind.toml — new [agent] section
[agent]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"

[agent.planner]
model = "claude-sonnet-4-5-20250514"

[agent.coder]
model = "claude-sonnet-4-5-20250514"
max_tokens = 16384

[agent.evaluator]
model = "claude-haiku-4-5-20251001"

[execution]
max_retries = 2
max_concurrent = 3
timeout_secs = 300
```

## Success Metrics

- `rewind plan "Add user auth"` produces 3-10 stories with verifiable criteria in under 30 seconds
- `rewind run` completes a simple 3-story epic end-to-end without human intervention
- Epic quality gates catch real issues (failing tests, lint errors)
- Failed tasks produce actionable fix-up tasks that succeed on retry
- All events replay correctly — `rewind status` shows accurate state after engine restart

## Open Questions

1. Should the coder agent have access to MCP tools (from `rewind mcp`) in addition to file/shell tools?
2. What's the right token budget split between coder context and tool call results?
3. Should fix-up tasks from failed quality gates include the full gate output or a summary?
4. Do we need a `rewind plan --approve` interactive step where the user reviews the plan before persisting?
