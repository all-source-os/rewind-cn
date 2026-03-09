# rewind

**Autonomous coding agent orchestrator built on CQRS + Event Sourcing.**

Rewind decomposes product requirements into tasks, schedules them across agent workers, and tracks progress — all backed by an append-only event store for full auditability.

```
rewind plan "Add user authentication with JWT"
rewind run --max-concurrent 3
rewind status
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
| `rewind-cn` | Binary — CLI interface and command orchestration |
| `rewind-cn-core` | Library — domain model, CQRS engine, event sourcing |

## Getting Started

```bash
# Install from source
cargo install --git https://github.com/all-source-os/rewind-cn rewind-cn

# Initialize a project
rewind init

# Create a plan from a description
rewind plan "Build a REST API with CRUD endpoints for users"

# Or from a file
rewind plan -f prd.md

# Execute pending tasks
rewind run

# Check progress
rewind status
```

### Configuration

After `rewind init`, edit `.rewind/rewind.toml`:

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

### `rewind init`

Creates the `.rewind/` directory with config and event store. Run this once per project.

### `rewind plan <description>`

Generates an execution plan (epic + tasks) from a description or PRD file.

| Flag | Description |
|---|---|
| `-f, --file <path>` | Read description from a file |
| `--dry-run` | Print the plan without persisting |

### `rewind run`

Picks up pending tasks and executes them through agent workers.

| Flag | Description |
|---|---|
| `--task <id>` | Run a single specific task |
| `--dry-run` | Show what would execute without running |
| `--max-concurrent <n>` | Maximum parallel workers (default: 3) |

### `rewind status`

Displays the current backlog and epic progress.

| Flag | Description |
|---|---|
| `--json` | Output as JSON for scripting |

### `rewind mcp`

Starts an [MCP](https://modelcontextprotocol.io) server over stdio for IDE integration, exposing rewind's capabilities as tools and resources.

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
RUST_LOG=debug rewind status

# Run a specific test
cargo test test_engine_roundtrip
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
│   ├── scheduler.rs   # FIFO task scheduler
│   └── status.rs      # Status summary builder
└── infrastructure/    # Framework integration
    ├── engine.rs      # RewindEngine<B> composition root
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
