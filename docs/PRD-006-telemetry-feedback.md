# PRD-006: Telemetry & User Feedback

## Problem

Rewind has no way to gather feedback, error reports, or usage patterns from users. When something fails in production (LLM calls, evaluator mismatches, quality gate failures), we have no visibility unless users manually report issues.

## Solution

A layered telemetry and feedback system, feature-gated and opt-in, using PostHog for analytics and the existing event store for structured error export.

## Design Principles

- **Opt-in by default** — no data leaves the machine unless the user explicitly enables it
- **Feature-gated** — `--features=telemetry` compiles in PostHog SDK, zero overhead otherwise
- **Privacy-first** — anonymize task titles/descriptions, never send API keys or file contents
- **Event store native** — leverage existing CQRS events rather than inventing new schemas

---

## Components

### 1. `rewind report` — Local Session Export

Zero-infra diagnostic bundle for GitHub issues.

**Exports:**
- Last session's events (TaskCreated, TaskFailed, QualityGateRan, AgentToolCall)
- Anonymized: task titles hashed, descriptions stripped, file paths relativized
- hotpath profile summary (if available)
- `rewind.toml` with API keys redacted
- System info: OS, arch, rewind version, Rust version

**Output:** `rewind-report-<session-id>.json` in current directory.

**Usage:**
```bash
rewind report                    # Export last session
rewind report --session <id>     # Export specific session
rewind report --full             # Include non-anonymized data (user's choice)
```

### 2. PostHog Analytics (opt-in)

Feature-gated telemetry via [PostHog Rust SDK](https://github.com/nicholasgasior/posthog-rs) or direct HTTP API.

**Events captured:**
| Event | Properties |
|-------|-----------|
| `rewind.session.started` | version, os, arch |
| `rewind.task.completed` | duration_ms, tool_call_count, retry_count |
| `rewind.task.failed` | error_type (enum, not message), retry_count |
| `rewind.gate.ran` | passed (bool), duration_ms |
| `rewind.plan.generated` | story_count, epic_quality_gate_count |
| `rewind.llm.call` | agent_type (planner/coder/evaluator), duration_ms, model |

**Never sent:** task titles, descriptions, file contents, API keys, acceptance criteria text.

**Configuration:**
```toml
[telemetry]
enabled = true                           # default: false
posthog_key = "phc_..."                  # project API key
posthog_host = "https://app.posthog.com" # or self-hosted
```

**Distinct ID:** Random UUID generated on `rewind init`, stored in `.rewind/telemetry_id`. Not tied to any user identity.

### 3. `rewind feedback` — CLI Feedback Command

Quick feedback submission from the terminal.

**Usage:**
```bash
rewind feedback "evaluator keeps rejecting valid criteria"
rewind feedback --attach-report "gate runner times out on cargo test"
```

**Behavior:**
- If `gh` CLI is available and authenticated: creates a GitHub issue on the rewind-cn repo with label `user-feedback`
- Otherwise: prints a pre-formatted issue template the user can paste into GitHub
- `--attach-report` automatically runs `rewind report` and includes the JSON

### 4. MCP Feedback Tool

Expose `rewind.submit_feedback` as an MCP tool so IDE agents can report issues mid-session.

**Schema:**
```json
{
  "name": "rewind.submit_feedback",
  "parameters": {
    "message": "string",
    "include_session": "boolean"
  }
}
```

---

## Implementation Order

1. **US-006-01**: `rewind report` command (no external deps, immediate value)
2. **US-006-02**: Telemetry ID + config parsing (foundation for PostHog)
3. **US-006-03**: PostHog event capture (feature-gated)
4. **US-006-04**: Instrument key code paths with PostHog events
5. **US-006-05**: `rewind feedback` CLI command
6. **US-006-06**: MCP feedback tool

## Quality Gates

- `make check` passes
- `cargo build` succeeds without telemetry feature (zero overhead)
- `cargo build --features=telemetry` succeeds
- No PII or secrets in captured events (verified by test)
