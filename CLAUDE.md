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
## Rules

This repo follows the portable rule set published at
[decebal-claude-skills](https://github.com/decebal/decebal-claude-skills) → `rules/`.
Each rule carries the incident that produced it; that is the load-bearing part.

**Loaded by reference, not by copy.** Install once per machine:

```bash
git clone https://github.com/decebal/decebal-claude-skills
mkdir -p ~/.claude/rules
cp decebal-claude-skills/rules/*.md ~/.claude/rules/
```

then `@`-import the subset you want from `~/.claude/CLAUDE.md`. Copying the rule
text into this repo would fork it, and a rule written twice drifts in one copy
without anyone noticing which — that is what the single source prevents.

### The subset that applies here

| Rule | Covers |
|---|---|
| `git-discipline` | One PR = one commit, merged by rebase. Dead-branch liveness check before the first commit. Never push to the trunk |
| `evidence-discipline` | Check the destination before trusting an absence; read runtime state, never guess it |
| `definition-of-done` | End-to-end or not done; size is never a reason to split |
| `estimation` | Never estimate in time; rock / sand / water is confidence, not size |
| `comments` | What a comment must earn; never narrate the fix |
| `anti-slop` | Report findings, don't perform them |
| `pr-evidence-report` | The HTML report a PR ships: claim → evidence → limits, screenshots with provenance, SHA-anchored before/after, copy blocks with a Pass line |
| `documents-not-artifacts` | Deliverables are versioned documents under `docs/`, never hosted pages |

Add `agent-parallelism`, `timeouts` and `process-ownership` when running several
agents against this repo at once.

### Merge settings, already enforced on this repo

Rebase-only — `allow_squash_merge=false`, `allow_merge_commit=false`,
`allow_rebase_merge=true` — with `delete_branch_on_merge=true`.

A PR therefore cannot merge until its branch is a **single commit**. Collapse
before review:

```bash
git reset --soft "$(git merge-base origin/main HEAD)"
git commit -F <message-file>
git diff <old-head> HEAD          # MUST be empty — that is the proof
git push --force-with-lease=refs/heads/<branch>:<old-head> origin HEAD:<branch>
```

Pin the lease to the old head you actually reviewed. The bare form does not
refuse when someone else pushed in between; the pinned form does.
