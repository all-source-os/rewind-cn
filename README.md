# ralph

**Autonomous coding agent orchestrator built on CQRS + Event Sourcing.**

Ralph decomposes product requirements into tasks, schedules them across agent workers, and tracks progress — all backed by an append-only event store for full auditability.

```
ralph plan "Add user authentication with JWT"
ralph run --max-concurrent 3
ralph status
```

## Architecture

```
┌─────────────────────────────────────────────────┐
│  CLI (clap)                                     │
│  init · plan · run · status · mcp               │
├─────────────────────────────────────────────────┤
│  Application Layer (pure logic)                 │
│  commands · handlers · planning · scheduler     │
├─────────────────────────────────────────────────┤
│  Domain Layer (zero framework deps)             │
│  events · aggregates · projections · ports      │
├─────────────────────────────────────────────────┤
│  Infrastructure (allframe CQRS)                 │
│  engine · command bridge · agent workers · mcp  │
├─────────────────────────────────────────────────┤
│  AllSource Event Store                          │
└─────────────────────────────────────────────────┘
```

Clean architecture with strict dependency direction — domain knows nothing about the framework, application layer contains pure functions, infrastructure adapts everything to [allframe](https://crates.io/crates/allframe).

### Workspace

| Crate | Role |
|---|---|
| `ralph-cli` | Binary — CLI interface and command orchestration |
| `ralph-core` | Library — domain model, CQRS engine, event sourcing |

## Getting Started

```bash
# Build
cargo build --release

# Initialize a project
ralph init

# Create a plan from a description
ralph plan "Build a REST API with CRUD endpoints for users"

# Or from a file
ralph plan -f prd.md

# Execute pending tasks
ralph run

# Check progress
ralph status
```

### Configuration

After `ralph init`, edit `.ralph/ralph.toml`:

```toml
project_name = "my-project"

[llm]
# provider = "anthropic"
# model = "claude-sonnet-4-20250514"
# api_key_env = "ANTHROPIC_API_KEY"

[agents]
max_concurrent = 3
timeout_secs = 300
```

## Commands

### `ralph init`

Creates the `.ralph/` directory with config and event store. Run this once per project.

### `ralph plan <description>`

Generates an execution plan (epic + tasks) from a description or PRD file.

| Flag | Description |
|---|---|
| `-f, --file <path>` | Read description from a file |
| `--dry-run` | Print the plan without persisting |

### `ralph run`

Picks up pending tasks and executes them through agent workers.

| Flag | Description |
|---|---|
| `--task <id>` | Run a single specific task |
| `--dry-run` | Show what would execute without running |
| `--max-concurrent <n>` | Maximum parallel workers (default: 3) |

### `ralph status`

Displays the current backlog and epic progress.

| Flag | Description |
|---|---|
| `--json` | Output as JSON for scripting |

### `ralph mcp`

Starts an [MCP](https://modelcontextprotocol.io) server over stdio for IDE integration, exposing ralph's capabilities as tools and resources.

## Event Sourcing Model

Every state change is captured as an immutable event:

```
TaskCreated → TaskAssigned → TaskStarted → TaskCompleted
                                         → TaskFailed

EpicCreated → EpicCompleted

SessionStarted → SessionEnded
```

State is rebuilt by replaying events through projections — no mutable database, full audit trail, time-travel debugging for free.

### Projections

- **BacklogProjection** — materialized view of all tasks and their current status
- **EpicProgressProjection** — tracks completion percentage per epic

## Development

```bash
# Run tests (22 passing)
cargo test

# Run with debug logging
RUST_LOG=debug ralph status

# Run a specific test
cargo test test_engine_roundtrip
```

### Project Structure

```
crates/ralph-core/src/
├── domain/            # Pure domain — no framework dependencies
│   ├── ids.rs         # TaskId, EpicId, SessionId, AgentId
│   ├── events.rs      # RalphEvent enum (single event stream)
│   ├── model.rs       # Aggregates, projections, read models
│   ├── error.rs       # RalphError (thiserror)
│   └── ports.rs       # Trait abstractions
├── application/       # Use cases — pure sync functions
│   ├── commands.rs    # Command structs
│   ├── handlers.rs    # Command → Vec<Event> handlers
│   ├── planning.rs    # PRD → Epic + Tasks decomposition
│   ├── scheduler.rs   # FIFO task scheduler
│   └── status.rs      # Status summary builder
└── infrastructure/    # Framework integration
    ├── engine.rs      # RalphEngine<B> composition root
    ├── adapters.rs    # allframe Aggregate/Projection impls
    ├── command_bridge.rs  # Command trait adapters
    ├── agent.rs       # AgentWorker task executor
    └── mcp_server.rs  # JSON-RPC 2.0 MCP server
```

## Roadmap

- **Phase 1** (current) — CLI scaffold, CQRS engine, mock agent execution
- **Phase 2** — LLM-powered plan decomposition and agent execution
- **Phase 3** — Multi-agent coordination, dependency graphs, parallel execution

## License

MIT
