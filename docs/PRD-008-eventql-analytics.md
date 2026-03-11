# PRD-008: EventQL Analytics Queries

## Overview
Add analytics capabilities to query the event store for execution insights. The event store already captures rich data (task lifecycles, agent tool calls, quality gate results, sessions) but there's no way to query or aggregate it. This adds a `rewind query` CLI command with predefined analytics queries.

## Goals
- Surface execution metrics from the event store (duration, success rate, tool usage)
- Provide predefined query commands via `rewind query <name>`
- Support JSON output for scripting/dashboards
- Enable filtering by epic, session, or time range

## Quality Gates

### Epic-Level (run once on epic completion)
- `cargo test` — all tests pass
- `cargo clippy -- -D warnings` — no warnings
- `cargo fmt --check` — formatting clean

### Story-Level (checked per story)
- Each story's acceptance criteria verified individually

## User Stories

### US-008-01: Analytics projection [Application]
**Description:** As a developer, I need an analytics projection that aggregates metrics from events so queries can run efficiently.

**Acceptance Criteria:**
- [ ] `AnalyticsProjection` struct in `application/analytics.rs`
- [ ] Tracks per-task: start_time, end_time, duration, outcome (pass/fail), tool_call_count
- [ ] Tracks per-epic: total_tasks, completed, failed, gate_pass_rate
- [ ] Tracks per-session: start, end, tasks_executed
- [ ] Tracks tool usage: call counts by tool_name
- [ ] `apply_event` handles all relevant RewindEvent variants
- [ ] Unit test: full lifecycle produces correct metrics

Mark each item [x] as you complete it. Only close when all are checked.

### US-008-02: Predefined queries [Application]
**Description:** As a user, I want predefined analytics queries so I can understand execution patterns without writing custom code.

**Acceptance Criteria:**
- [ ] `QueryResult` enum with typed results for each query
- [ ] `task-summary`: per-task duration, status, criteria progress
- [ ] `epic-summary`: per-epic completion %, gate results
- [ ] `tool-usage`: tool call frequency ranked by count
- [ ] `session-history`: session timeline with task counts
- [ ] Each query returns structured data (serializable to JSON)

Mark each item [x] as you complete it. Only close when all are checked.

### US-008-03: CLI query command [Backend]
**Description:** As a user, I want `rewind query <name>` to run analytics queries from the CLI.

**Acceptance Criteria:**
- [ ] `query` subcommand added to CLI with positional `name` argument
- [ ] `--json` flag for JSON output (default: human-readable table)
- [ ] `--epic <id>` filter for epic-scoped queries
- [ ] `--session <id>` filter for session-scoped queries
- [ ] `rewind query list` shows available query names
- [ ] Invalid query name shows error with available options

Mark each item [x] as you complete it. Only close when all are checked.

## Non-Goals
- Custom query language (SQL-like DSL) — predefined queries only
- Real-time streaming analytics
- Token cost tracking (would need API integration)

## Technical Considerations
- AnalyticsProjection rebuilds from events like other projections
- Add to engine's projection set so it rebuilds alongside BacklogProjection
- Duration calculation: diff between TaskStarted and TaskCompleted/TaskFailed timestamps
- Tool usage data comes from AgentToolCall events
