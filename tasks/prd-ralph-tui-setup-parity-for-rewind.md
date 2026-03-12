# PRD: Ralph-TUI Setup Parity for Rewind

## Overview
Bring rewind's agent execution loop to parity with ralph-tui's setup by adding five capabilities: a user-customizable Tera prompt template system, progress memory via event sourcing, iteration logging via events, two-tier quality gates, and config enhancements. These features make agent sessions context-aware, debuggable, and self-improving.

## Goals
- Agents receive rich, customizable prompts with task context, progress notes, and project-specific variables
- Cross-session learnings are captured as events and injected into future agent prompts
- Every agent iteration is logged as an event for debugging and audit
- Quality gates are split into epic-level (run once) and story-level (per-task) tiers
- `rewind.toml` gains new fields for prompt template path, max iterations, and subagent tracing detail

## Quality Gates

### Epic-Level (run once on epic completion)
General codebase checks that run ONCE when all stories are done:
- `cargo test` — all tests pass
- `cargo clippy` — no warnings
- `cargo fmt --check` — formatting is correct

### Story-Level (checked per story)
- **All stories:** `cargo check` compiles without errors

## User Stories

### US-001: Add Tera prompt template engine [Backend]
As a developer, I want rewind to use Tera templates for agent prompts so that I can customize what context agents receive.

**Acceptance Criteria:**
- [ ] `tera` crate added to `Cargo.toml` dependencies
- [ ] New module `infrastructure/prompt_template.rs` exists and compiles
- [ ] `render_prompt(template_path, context_map) -> Result<String>` function implemented
- [ ] Supports a context map of `HashMap<String, String>` for arbitrary key-value injection
- [ ] Default template embedded via `include_str!` if no custom template file exists
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-002: Create default prompt template [Backend]
As a developer, I want a sensible default prompt template shipped with rewind so that agents work well out of the box.

**Acceptance Criteria:**
- [ ] Default template file at `src/infrastructure/default_prompt.tera` exists
- [ ] Template includes variables: `{{task}}`, `{{epic}}`, `{{progress}}`, `{{project_context}}`
- [ ] Template compiles and renders with all variables populated
- [ ] Template compiles and renders with optional variables missing (graceful defaults)
- [ ] Unit test verifies rendering with full and partial context
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-003: Wire prompt template into coder agent [Backend]
As a developer, I want the coder agent to use the Tera template engine instead of hardcoded prompts so that prompts are customizable.

**Acceptance Criteria:**
- [ ] `coder.rs` calls `render_prompt()` instead of using hardcoded prompt strings
- [ ] Context map populated with task title, epic name, and project context from session
- [ ] Custom template path read from `rewind.toml` if specified, otherwise uses default
- [ ] Existing agent behavior unchanged when using default template (output equivalent)
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-004: Add ProgressNoted event [Backend]
As a developer, I want a `ProgressNoted` event in the domain so that agent learnings are captured in the event store.

**Acceptance Criteria:**
- [ ] `ProgressNoted` variant added to `RewindEvent` enum
- [ ] Fields: `session_id`, `task_id` (optional), `note: String`, `note_type: ProgressNoteType`
- [ ] `ProgressNoteType` enum: `TaskCompleted`, `TaskFailed`, `RetryLearning`, `Discretionary`
- [ ] `event_type_name()` returns `"rewind.domain.progress_noted"`
- [ ] Serialization/deserialization roundtrip test passes
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-005: Emit ProgressNoted events from handlers [Backend]
As a developer, I want command handlers to automatically emit `ProgressNoted` events on task completion and failure so that learnings accumulate without agent effort.

**Acceptance Criteria:**
- [ ] `complete_task` handler emits `ProgressNoted` with `note_type: TaskCompleted`
- [ ] Task failure/retry paths emit `ProgressNoted` with `note_type: TaskFailed` or `RetryLearning`
- [ ] Handlers accept an optional `discretionary_note: Option<String>` for agent-chosen notes
- [ ] When discretionary note is provided, an additional `ProgressNoted` event with `note_type: Discretionary` is emitted
- [ ] Unit tests verify events are emitted for each path
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-006: Project progress into prompt context [Backend]
As a developer, I want progress notes projected from the event store and injected into the prompt template so that agents benefit from past learnings.

**Acceptance Criteria:**
- [ ] New projection function: `project_progress_notes(events) -> String` in application layer
- [ ] Projects recent `ProgressNoted` events into a markdown-formatted summary
- [ ] Summary is injected as `{{progress}}` variable in the prompt template context
- [ ] Configurable limit on how many notes to include (default: last 20)
- [ ] Unit test verifies projection output format
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-007: Add IterationLogged event [Backend]
As a developer, I want an `IterationLogged` event so that every agent iteration is recorded in the event store for debugging.

**Acceptance Criteria:**
- [ ] `IterationLogged` variant added to `RewindEvent` enum
- [ ] Fields: `session_id`, `task_id`, `iteration_number: u32`, `agent_output: String`, `duration_ms: u64`
- [ ] `event_type_name()` returns `"rewind.domain.iteration_logged"`
- [ ] Serialization/deserialization roundtrip test passes
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-008: Emit IterationLogged events from agent runner [Backend]
As a developer, I want the agent runner to emit `IterationLogged` events after each iteration so that execution history is queryable.

**Acceptance Criteria:**
- [ ] Agent execution loop in infrastructure layer emits `IterationLogged` after each iteration
- [ ] `iteration_number` increments correctly across iterations within a session
- [ ] `agent_output` captures the agent's response text
- [ ] `duration_ms` measures wall-clock time of the iteration
- [ ] Unit test verifies event emission with correct fields
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-009: Implement two-tier quality gate runner [Backend]
As a developer, I want the gate runner to distinguish between epic-level and story-level gates so that expensive checks only run once at epic completion.

**Acceptance Criteria:**
- [ ] `QualityGateLevel` enum added: `Epic`, `Story`
- [ ] Gate configuration in `rewind.toml` supports `[gates.epic]` and `[gates.story]` sections
- [ ] `run_gates(level: QualityGateLevel)` function runs only gates for the specified level
- [ ] Story completion triggers story-level gates only (`cargo check`)
- [ ] Epic completion triggers epic-level gates (`cargo test && cargo clippy && cargo fmt --check`)
- [ ] Existing `gate_runner.rs` refactored to support the two tiers
- [ ] Unit test verifies correct gate selection per level
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-010: Extend rewind.toml configuration [Backend]
As a developer, I want new config fields in `rewind.toml` so that prompt templates, iteration limits, and tracing are configurable.

**Acceptance Criteria:**
- [ ] `prompt_template` field added (optional path to `.tera` file)
- [ ] `max_iterations` field added (u32, default: 10)
- [ ] `subagent_tracing_detail` field added (enum: `minimal`, `normal`, `verbose`, default: `normal`)
- [ ] `[gates.epic]` and `[gates.story]` sections added with `commands` arrays
- [ ] Existing config parsing remains backwards-compatible (new fields are optional with defaults)
- [ ] Config deserialization test with all new fields
- [ ] Config deserialization test with no new fields (defaults applied)
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-011: Add MCP query for iteration history [Integration]
As a developer, I want an MCP tool to query iteration logs so that I can debug agent sessions externally.

**Acceptance Criteria:**
- [ ] New MCP tool `rewind_list_iterations` accepts `session_id` parameter
- [ ] Returns iteration number, duration, and truncated output for each iteration
- [ ] Supports `format: "toon"` for token-optimized output
- [ ] Integration test verifies tool returns correct data for a session with multiple iterations
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

### US-012: Add MCP query for progress notes [Integration]
As a developer, I want an MCP tool to query progress notes so that learnings are accessible outside the agent loop.

**Acceptance Criteria:**
- [ ] New MCP tool `rewind_list_progress` accepts optional `session_id` and `note_type` filters
- [ ] Returns note text, type, timestamp, and associated task ID
- [ ] Supports `format: "toon"` for token-optimized output
- [ ] Integration test verifies filtering by note type
- [ ] `cargo check` passes

Mark each item [x] as you complete it. Only close when all are checked.

## Functional Requirements
- FR-1: The system must render agent prompts using Tera templates with an extensible key-value context map
- FR-2: The system must ship a default prompt template that works without user configuration
- FR-3: The system must emit `ProgressNoted` events automatically on task completion, failure, and retry
- FR-4: The system must support discretionary progress notes emitted by agent decision
- FR-5: The system must project progress notes into a markdown summary for prompt injection
- FR-6: The system must emit `IterationLogged` events after each agent iteration with output and duration
- FR-7: The system must run story-level gates (`cargo check`) on task completion and epic-level gates (`cargo test && cargo clippy && cargo fmt --check`) on epic completion
- FR-8: The system must read gate configuration from `rewind.toml` with separate epic and story sections
- FR-9: The system must support new config fields with backwards-compatible defaults

## Non-Goals
- Auto-commit feature (deferred)
- Separate iteration log files on disk (events are the single source of truth)
- Progress memory as a separate markdown file (events + projection instead)
- GUI or web-based iteration viewer
- Custom gate tiers beyond epic/story

## Technical Considerations
- Tera is the chosen template engine (Rust-native, Jinja2-like syntax)
- All new state goes through the event store — no new files in `.rewind/data/` beyond what AllSourceBackend manages
- `ProgressNoted` and `IterationLogged` events must follow existing `event_type_name()` convention: `"rewind.domain.<snake_case>"`
- Progress projection should be efficient — consider caching or limiting to recent N events
- `agent_output` in `IterationLogged` may be large; consider truncation strategy for MCP responses

## Success Metrics
- Agent prompts include task context, progress notes, and project-specific variables
- Progress notes accumulate across sessions and are queryable via MCP
- Iteration history is available for any past session via MCP
- Epic-level gates run only once (not per-story), reducing wasted CI time
- All new config fields are optional with sensible defaults (zero breaking changes)

## Open Questions
- Should progress note projection support filtering by relevance (e.g., only notes from the same epic)?
- What truncation length for `agent_output` in MCP responses? (Suggest 2000 chars with `full: true` option)
- Should the default prompt template be versioned so upgrades don't silently change agent behavior?