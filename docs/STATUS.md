# Rewind Status Report

**Date:** 2025-03-15
**Version:** 0.2.1
**Grade:** C+ (Promising Prototype, Not Yet Battle-Tested)

## What Works

| Feature | Status | Notes |
|---------|--------|-------|
| `rewind init` | Working | Creates `.rewind/`, config template, event store |
| `rewind status` | Working | Task counts, epic progress bars, JSON output |
| `rewind plan <desc>` | Working | LLM decomposition (with `[agent]` config) or passthrough |
| `rewind plan -f <file>` | Working | Reads PRD from file |
| `rewind plan --dry-run` | Working | Shows plan without persisting |
| `rewind run` | Wired, untested E2E | Orchestrator loop works with mocks, never validated with real LLM |
| `rewind run --dry-run` | Working | Shows what would execute |
| `rewind run --parallel` | Wired, untested E2E | Git worktree isolation coded, unit tests pass |
| `rewind run --tui` | Exists | TUI dashboard code present, unpolished |
| `rewind import <file>` | Working | Imports beads JSONL/JSON, creates epics/tasks/deps |
| `rewind query <name>` | Working | Analytics: task-summary, epic-summary, tool-usage, session-history |
| `rewind report` | Working | Diagnostic export |
| `rewind mcp` | Working | JSON-RPC 2.0 MCP server, integration tested |

## Architecture (Solid)

- **CQRS + Event Sourcing** via allframe — clean separation of domain/application/infrastructure
- **163 unit tests** passing (157 reliably, 6 orchestrator-loop tests pass individually but have nondeterministic hangs when run together)
- **Tool sandboxing**: path traversal prevention, command injection protection, symlink safety
- **Retry logic**: planner and evaluator retry on JSON parse failure with corrective prompts
- **TaskExecutor trait**: orchestrator is testable with mock executors

## The Gap That Matters

`rewind run` — the core value proposition — has **never been validated end-to-end with a real LLM**.

The orchestrator loop (SELECT → PROMPT → EXECUTE → EVALUATE) is:
- Correctly wired
- Passes unit tests with mock providers
- Uses real rig-core clients (Anthropic/OpenAI)

But nobody has confirmed that:
- Claude/GPT produces outputs that parse correctly
- The evaluator reliably judges task completion
- Real coding tasks complete successfully
- Error recovery handles rate limits, malformed output, context overflow

## What's Missing for Production

1. **Real-world validation** — Run 3-5 real tasks with Claude to see what breaks
2. **LLM error recovery** — Rate limits, malformed outputs, context overflow
3. **ALLOWED_COMMANDS** — Hardcoded allowlist in coder, not configurable
4. **Task timeout** — Kills task but could be more graceful
5. **Prompt tuning** — System prompts are reasonable but unvalidated
6. **Examples** — No recorded successful runs to show users

## Dependencies

| Crate | Version | Role | Stability |
|-------|---------|------|-----------|
| allframe | 0.1 | CQRS/EventStore | Pre-release but functional |
| rig-core | 0.32 | LLM provider abstraction | Stable, Anthropic + OpenAI |
| tokio | 1.x | Async runtime | Production-grade |
| serde/schemars | 1.x | Serialization | Standard |
| tera | 1.x | Prompt templates | Stable |

## Configuration

Minimal config to enable LLM execution (`.rewind/rewind.toml`):

```toml
project_name = "my-project"

[agent]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"

[agent.coder]
model = "claude-sonnet-4-5-20250514"
max_tokens = 16384

[agent.evaluator]
model = "claude-haiku-4-5-20251001"

[execution]
max_concurrent = 1
timeout_secs = 300
max_retries = 2
```

Requires `ANTHROPIC_API_KEY` environment variable set.

## Bottom Line

The architecture is A+, code quality is A, but confidence in the actual agent loop is **zero** because it's never been run for real. Next step: run it against real tasks.
