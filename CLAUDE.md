# rewind-cn

Autonomous coding agent orchestrator built on CQRS + Event Sourcing.

## Crate Naming

- Binary: `rewind-cn` (CLI name: `rewind`)
- Library: `rewind-cn-core`
- Internal name: "rewind" (used in configs, MCP URIs, event type names)

## Architecture Rules

- **domain/** — Zero framework dependencies. No allframe traits except `Event`/`EventTypeName` on events.
- **application/** — Pure business logic. No async, no framework coupling. Functions return `Result<Vec<RewindEvent>, RewindError>`.
- **infrastructure/** — allframe integration, I/O, external tool bridges (chronis, MCP).

## Build & Test

```bash
cargo test                    # Run all 22+ tests
cargo build                   # Build workspace
cargo clippy                  # Lint
```

## Chronis Integration

This project uses [chronis](https://github.com/nicholasgasior/chronis) (`cn` CLI) as an external task tracker.

### Task Ownership Model
- **Chronis** owns the task backlog (definitions, dependencies, readiness)
- **Rewind** owns execution state (sessions, agent assignments, event sourcing)

### Agent Workflow (cn → rewind bridge)
```bash
cn ready --toon          # Get next available (open + unblocked) task
cn claim <id> --toon     # Claim: maps to assign_task + start_task
cn done <id> --toon      # Complete: maps to complete_task
```

### Skill Pipeline (for new features)
```
1. /rewind-prd            → Generate PRD with user stories
2. /rewind-beads           → Convert PRD to chronis beads (cn task create)
3. rewind run --tracker chronis --epic <id>  → Execute
```

## MCP Server

Tools support optional `format: "toon"` parameter for token-optimized output (~50% fewer tokens).
TOON uses pipe-delimited rows instead of JSON objects.

## Event Store

- Production: `AllSourceBackend` at `.rewind/data/`
- Tests: `InMemoryBackend`
- Single event enum: `RewindEvent` (tagged JSON via serde)
- Custom `event_type_name()`: `"rewind.domain.event"` (required by AllSourceBackend)

## Conventions

- IDs use UUID v4 via newtype wrappers (`TaskId`, `EpicId`, `SessionId`, `AgentId`)
- All commands are plain structs in `application/commands.rs`
- Command handlers are pure sync functions in `application/handlers.rs`
- Infrastructure bridges wrap commands with allframe's `Command` trait in `command_bridge.rs`
