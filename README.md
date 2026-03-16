# rewind

**Autonomous coding agent orchestrator built on CQRS + Event Sourcing.**

Rewind decomposes product requirements into tasks, schedules them across AI agent workers, and tracks progress — all backed by an append-only event store for full auditability. Multi-provider LLM support (Anthropic, OpenAI, Ollama) with per-role configuration. Use the Anthropic API directly or delegate to Claude Code CLI.

```
rewind plan -f prd.md
rewind run --parallel --tui
rewind status
rewind query task-summary
```

## Architecture

```
┌───────────────────────────────────────────────────────────────┐
│  CLI (clap)                                                   │
│  init · plan · run · status · query · import · report · mcp  │
├───────────────────────────────────────────────────────────────┤
│  Application Layer (pure logic)                               │
│  commands · handlers · planning · scheduler · analytics       │
├───────────────────────────────────────────────────────────────┤
│  Domain Layer (zero framework deps)                           │
│  events · aggregates · projections · ports                    │
├───────────────────────────────────────────────────────────────┤
│  Infrastructure (allframe CQRS)                               │
│  engine · agents · orchestrator · worktree · importer · llm   │
├───────────────────────────────────────────────────────────────┤
│  AllSource Event Store                                        │
└───────────────────────────────────────────────────────────────┘
```

Clean architecture with strict dependency direction — domain knows nothing about the framework, application layer contains pure functions, infrastructure adapts everything to [allframe](https://crates.io/crates/allframe).

### Workspace

| Crate | Role |
|---|---|
| [`rewind-cn`](https://crates.io/crates/rewind-cn) | Binary — CLI interface and command orchestration |
| [`rewind-cn-core`](https://crates.io/crates/rewind-cn-core) | Library — domain model, CQRS engine, event sourcing |

## Getting Started

```bash
# Install from crates.io
cargo install rewind-cn

# Initialize a project
rewind init

# Create a plan from a PRD file
rewind plan -f prd.md

# Or from a description
rewind plan "Build a REST API with CRUD endpoints for users"

# Execute with parallel worktrees and TUI dashboard
rewind run --parallel --tui

# Check progress
rewind status

# Query analytics
rewind query task-summary
```

### Configuration

After `rewind init`, edit `.rewind/rewind.toml`:

```toml
project_name = "my-project"

# Agent configuration — per-role provider overrides supported
[agent]
provider = "anthropic"         # anthropic | openai | ollama
api_key_env = "ANTHROPIC_API_KEY"
coder_backend = "api"          # "api" (LLM API) or "claude-code" (Claude Code CLI)

[agent.planner]
model = "claude-sonnet-4-5-20250514"

[agent.coder]
model = "claude-sonnet-4-5-20250514"
max_tokens = 16384

[agent.evaluator]
model = "claude-haiku-4-5-20251001"
# Override provider for a specific role:
# provider = "openai"
# api_key_env = "OPENAI_API_KEY"

[execution]
max_concurrent = 3
timeout_secs = 300
max_retries = 2
```

### Multi-Provider Support

Each agent role (planner, coder, evaluator) can use a different LLM provider:

```toml
[agent]
provider = "anthropic"                    # global default

[agent.planner]
provider = "openai"                       # override for planner
api_key_env = "OPENAI_API_KEY"
model = "gpt-4o"

[agent.coder]
model = "claude-sonnet-4-5-20250514"      # inherits anthropic

[agent.evaluator]
model = "claude-haiku-4-5-20251001"       # inherits anthropic
```

Supported providers: `anthropic`, `openai`, `ollama`. Powered by [rig-core](https://crates.io/crates/rig-core).

### Ollama (Local Models)

Run with local models — no API keys needed:

```toml
[agent]
provider = "ollama"
coder_backend = "claude-code"    # use Claude Code CLI for coding

[agent.evaluator]
model = "qwen2.5-coder:14b"
# base_url = "http://localhost:11434/v1"   # default
```

### Claude Code Backend

Use `claude` CLI instead of the Anthropic API for the coder role:

```toml
[agent]
coder_backend = "claude-code"

[agent.coder]
model = "claude-sonnet-4-5-20250514"   # passed to claude --model
```

This delegates coding to Claude Code's built-in tools (Read, Edit, Bash, etc.) via `claude --print --dangerously-skip-permissions`. No `ANTHROPIC_API_KEY` needed for the coder role.

## Commands

### `rewind init`

Creates the `.rewind/` directory with config and event store. Run this once per project.

### `rewind plan <description>`

Generates an execution plan (epic + tasks) from a description or PRD file.

| Flag | Description |
|---|---|
| `-f, --file <path>` | Read description from a file |
| `--dry-run` | Print the plan without persisting |

### `rewind run`

Picks up pending tasks and executes them through agent workers. When no pending tasks exist and chronis is available, launches a TUI epic browser to import from chronis.

| Flag | Description |
|---|---|
| `--task <id>` | Run a single specific task |
| `--dry-run` | Show what would execute without running |
| `--max-concurrent <n>` | Maximum parallel workers (default: 3) |
| `--parallel` | Use git worktrees for parallel task isolation |
| `--tui` | Show TUI dashboard during execution |

### `rewind status`

Displays the current backlog and epic progress.

| Flag | Description |
|---|---|
| `--json` | Output as JSON for scripting |

### `rewind query <name>`

Query execution analytics from the event store.

| Query | Description |
|---|---|
| `task-summary` | Task status breakdown and timing |
| `epic-summary` | Epic completion percentages |
| `tool-usage` | Agent tool call frequency |
| `session-history` | Session timeline |
| `list` | Show available queries |

| Flag | Description |
|---|---|
| `--json` | Output as JSON |
| `--epic <id>` | Filter by epic |

### `rewind import <file>`

Import tasks and epics from a beads JSONL or JSON file.

| Flag | Description |
|---|---|
| `--skip-closed` | Skip closed/done items (default: true) |

Extracts acceptance criteria from `- [ ]` checkboxes, quality gates from backtick-quoted commands, and resolves parent-child and blocking dependencies.

### `rewind report`

Export an anonymized diagnostic report for troubleshooting.

| Flag | Description |
|---|---|
| `--session <id>` | Export a specific session (default: last) |
| `--full` | Include non-anonymized data |

### `rewind feedback <message>`

Submit feedback or report an issue.

| Flag | Description |
|---|---|
| `--attach-report` | Include an anonymized diagnostic report |

### `rewind mcp`

Starts an [MCP](https://modelcontextprotocol.io) server over stdio for IDE integration, exposing rewind's capabilities as tools and resources. Supports optional `format: "toon"` for token-optimized output.

## Key Features

### TUI Epic Browser

When `rewind run` finds no pending tasks, it automatically browses chronis epics in a TUI, lets you select one, imports its tasks (enriched with descriptions from PRD files in `tasks/`), and transitions to the execution dashboard.

### TUI Execution Dashboard

Live dashboard during `rewind run --tui` showing task progress, agent activity, and epic completion in real time. Pre-seeded with existing backlog state so tasks imported before the TUI starts are visible immediately.

### Git Worktree Parallel Execution

`rewind run --parallel` isolates each agent in its own git worktree, enabling true parallel execution without merge conflicts. Changes are merged back on task completion.

### Chronis Integration

Rewind integrates with [Chronis](https://all-source-os.github.io/all-source/chronis/) as an external task tracker:

- **Chronis** owns the task backlog (definitions, dependencies, readiness)
- **Rewind** owns execution state (sessions, agent assignment, event sourcing)

### Beads Import

Import existing task hierarchies from chronis/beads JSONL files. Automatically extracts acceptance criteria, quality gates, and dependency graphs. When chronis tasks lack descriptions, scans `tasks/*.md` for matching PRD files and extracts user story content.

### Execution Analytics

Query the event store with `rewind query` for task timing, tool usage patterns, session history, and epic progress analytics.

### Skill Pipeline

```
1. /rewind-prd      → Generate PRD with quality gates and acceptance criteria
2. /rewind-beads    → Convert PRD to chronis beads (cn create --toon)
3. rewind run       → Execute with agent orchestration
```

## Event Sourcing Model

Every state change is captured as an immutable event:

```
TaskCreated → TaskAssigned → TaskStarted → TaskCompleted
                                         → TaskFailed → TaskRetried

EpicCreated → EpicCompleted

SessionStarted → SessionEnded

AgentToolCall · CriterionChecked · QualityGateRan · IterationLogged · ProgressNoted
```

State is rebuilt by replaying events through projections — no mutable database, full audit trail, time-travel debugging for free.

### Projections

- **BacklogProjection** — materialized view of all tasks and their current status
- **EpicProgressProjection** — tracks completion percentage per epic
- **AnalyticsProjection** — tool usage, iteration counts, progress notes

## Development

```bash
# Run tests (196+ passing)
cargo test

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt --check

# Run with debug logging
RUST_LOG=debug rewind status
```

### Project Structure

```
crates/rewind-cn-core/src/
├── domain/            # Pure domain — no framework dependencies
│   ├── ids.rs         # TaskId, EpicId, SessionId, AgentId
│   ├── events.rs      # RewindEvent enum (single event stream)
│   ├── model.rs       # Aggregates, projections, read models
│   ├── error.rs       # RewindError (thiserror)
│   └── ports.rs       # Trait abstractions
├── application/       # Use cases — pure sync functions
│   ├── commands.rs    # Command structs
│   ├── handlers.rs    # Command → Vec<Event> handlers
│   ├── planning.rs    # PRD → Epic + Tasks decomposition
│   ├── scheduler.rs   # Dependency-aware task scheduler
│   ├── status.rs      # Status summary builder
│   └── analytics.rs   # Event store query engine
└── infrastructure/    # Framework integration
    ├── engine.rs      # RewindEngine<B> composition root
    ├── adapters.rs    # allframe Aggregate/Projection impls
    ├── command_bridge.rs  # Command trait adapters
    ├── orchestrator.rs    # Multi-agent orchestration loop
    ├── planner.rs     # LLM-powered plan decomposition
    ├── coder.rs       # Coding agent with tool use
    ├── claude_code.rs # Claude Code CLI backend
    ├── evaluator.rs   # Code review agent
    ├── llm.rs         # Multi-provider client factory (Anthropic/OpenAI/Ollama)
    ├── chronis.rs     # Chronis task tracker bridge
    ├── importer.rs    # Beads/PRD import with story matching
    ├── worktree.rs    # Git worktree management
    ├── gate_runner.rs # Quality gate execution
    ├── telemetry.rs   # PostHog telemetry (opt-in)
    ├── mcp_server.rs  # JSON-RPC 2.0 MCP server
    ├── sanitize.rs    # Prompt injection mitigation
    ├── prompt_template.rs # Tera prompt rendering
    └── toon.rs        # Token-optimized output format
```

## License

MIT
