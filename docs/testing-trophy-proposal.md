# Testing Trophy Proposal: US-011 & US-012

Covers the testing gaps for Import (US-011), Multi-provider (US-012), and adjacent untested features (Telemetry, Report).

## Current State

- **110 tests** total (77 unit + 9 analytics + 5 broadcast + 7 worktree + 3 TUI + 3 config + 2 telemetry + 2 report + 2 misc)
- US-011 (Import): 11 inline unit tests, **0 integration tests**
- US-012 (Multi-provider): 9 inline unit tests, **0 integration tests**
- Telemetry: 2 inline tests (disabled client no-op only)
- Report: 2 inline tests (anonymize + redact)

## Testing Trophy Layers

### Layer 1: Unit Tests (already done — fill remaining gaps)

**US-012 `llm.rs` — 2 new tests:**
- `openai_client_missing_env_var` — verify OpenAI path returns clear error on missing key
- `per_role_api_key_defaults_to_provider_key` — when role overrides provider but not api_key_env, confirm the provider's default env var is used (not the global one)

**Telemetry `telemetry.rs` — 3 new tests:**
- `is_active_requires_feature_and_config_and_key` — verify all three conditions must be true
- `new_with_disabled_config` — explicit `enabled: false` with key present still returns inactive
- `capture_simple_builds_correct_properties` — verify property map construction from string pairs (test the mapping logic, not the send)

**Report `report.rs` — 3 new tests:**
- `filter_session_events_captures_bounded_range` — events between SessionStarted and SessionEnded for a given ID
- `filter_session_events_no_match_returns_all` — fallback when session not found
- `hash_string_is_deterministic` — same input always produces same hash

**Estimated: +8 unit tests**

### Layer 2: Integration Tests (new test files)

#### `tests/import_integration.rs` — 6 tests

End-to-end import through the in-memory engine, verifying the full pipeline from JSONL parsing to queryable projections.

```
import_creates_epic_with_quality_gates
  Parse a JSONL epic whose description contains `- [ ] \`cargo test\` passes`.
  Verify the engine's epic_progress projection has the epic with quality_gates populated.

import_creates_tasks_with_criteria_linked_to_epic
  Parse JSONL with 1 epic + 2 tasks (parent-child deps).
  Verify backlog has 2 tasks, both with epic_id set, criteria extracted from descriptions.

import_blocking_deps_wire_through_to_scheduler
  Parse JSONL where task B blocks-on task A.
  Verify scheduler picks A before B (A is runnable, B is blocked).

import_skip_closed_filters_correctly
  Parse JSONL with mix of open/closed epics and tasks.
  Verify only open items appear in backlog after import with skip_closed=true.

import_from_json_array_format
  Same as JSONL test but using a JSON array file.
  Verifies the .json path in import_file works.

import_empty_file_is_noop
  Import an empty .jsonl file.
  Verify no error, 0 epics, 0 tasks.
```

#### `tests/multi_provider_integration.rs` — 5 tests

Tests the config-to-client wiring without requiring actual API keys (using the error paths and config resolution).

```
default_config_creates_anthropic_clients
  Default AgentConfig → create_planner_client, create_coder_client, create_evaluator_client.
  All should fail with "ANTHROPIC_API_KEY not set" (confirming they resolve to Anthropic).

per_role_override_creates_different_providers
  Config with planner.provider = "openai" and coder left default.
  create_planner_client should fail with "OPENAI_API_KEY not set".
  create_coder_client should fail with "ANTHROPIC_API_KEY not set".
  Confirms each role resolves independently.

per_role_api_key_env_override
  Config with evaluator.api_key_env = "MY_EVAL_KEY".
  create_evaluator_client should fail with "MY_EVAL_KEY not set".
  Confirms custom env var is respected.

unknown_provider_falls_back_to_anthropic
  Config with provider = "unknown_llm".
  Resolution should fall back to Anthropic (not error).
  create_planner_client fails with "ANTHROPIC_API_KEY not set".

orchestrator_accepts_different_clients_per_role
  Create an AgentConfig, call create_coder_client + create_evaluator_client.
  Construct Orchestrator::new with both (will fail on missing keys but the
  type-level wiring compiles and the factory functions are exercised).
  This is primarily a compilation/wiring test.
```

#### `tests/report_integration.rs` — 4 tests

Tests the report generation against a real in-memory engine with populated events.

```
report_session_filtering_captures_correct_events
  Populate engine with 2 sessions. Filter to session 1.
  Verify only session 1 events appear in the filtered list.

report_anonymize_strips_all_sensitive_fields
  Create a TaskCreated + AgentToolCall + QualityGateRan event set.
  Anonymize each. Verify: title is sha256-hashed, description is [redacted],
  args_summary is hashed, output is [N bytes redacted].

report_anonymize_preserves_structure
  Anonymized events should still have the same keys (type, task_id, etc.)
  just with values redacted. Verify event type tag is preserved.

report_redact_config_handles_nested_secrets
  TOML with nested [agent] and [secrets] tables containing key/token/password fields.
  Verify all sensitive fields are [REDACTED], non-sensitive fields preserved.
```

**Estimated: +15 integration tests**

### Layer 3: Smoke Tests (CLI-level)

#### Added to `tests/tui_app_state.rs` or new `tests/cli_smoke.rs` — 3 tests

```
import_subcommand_rejects_missing_file
  Call `rewind import nonexistent.jsonl` (via command parsing).
  Verify it returns an error containing "File not found".

import_subcommand_rejects_unsupported_format
  Call `rewind import data.csv`.
  Verify error mentions "Unsupported file format".

query_subcommand_list_shows_available_queries
  Call `rewind query list` (parse output or verify function returns Ok).
```

**Estimated: +3 smoke tests**

## Summary

| Layer | Existing | New | Total |
|-------|----------|-----|-------|
| Unit | 99 | 8 | 107 |
| Integration | 24 | 15 | 39 |
| Smoke | 0 | 3 | 3 |
| **Total** | **110** (current) | **+26** | **136** |

## Priority Order

1. `tests/import_integration.rs` (6 tests) — highest risk, most complex feature
2. `tests/multi_provider_integration.rs` (5 tests) — validates the OpenAI path exists
3. Unit test gap fills (8 tests) — quick wins, catch edge cases
4. `tests/report_integration.rs` (4 tests) — validates anonymization correctness
5. Smoke tests (3 tests) — CLI error path coverage

## Quality Gates

After implementation, all must pass:
```bash
cargo test                           # 136+ tests green
cargo clippy -- -D warnings          # zero warnings
cargo fmt --check                    # clean
```
