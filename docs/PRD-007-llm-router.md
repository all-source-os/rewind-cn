# PRD-007: LLM Router — Multi-Provider Failover & Complexity Routing

## Problem

Rewind currently uses a single LLM provider per role (coder, evaluator, planner) with no failover. If the primary provider returns a 429 rate-limit or 5xx error, the entire task fails. There's also no cost optimization — simple evaluation prompts hit the same expensive model as complex coding tasks.

Inspired by [ClawRouter](https://github.com/BlockRunAI/ClawRouter), we want to add intelligent routing without disturbing the existing agent architecture.

## Design Principle

**`ProviderClient::prompt()` is the single choke point.** Every agent calls through it. We wrap it in a `RouterClient` that adds routing, failover, and cooldown tracking. Agents never change.

```
Before:  AgentConfig → ProviderClient → LLM API
After:   AgentConfig → RouterClient → [ProviderClient, ...] → LLM API
```

## Architecture

### RouterClient

```rust
pub struct RouterClient {
    primary: ProviderClient,
    fallbacks: Vec<FallbackProvider>,
    cooldowns: Arc<Mutex<HashMap<String, Instant>>>,
    config: RouterConfig,
}

pub struct FallbackProvider {
    pub client: ProviderClient,
    pub model: String,
}

pub struct RouterConfig {
    pub cooldown_secs: u64,
    pub max_retries: u32,
    pub retry_codes: Vec<u16>,        // 429, 502, 503, 504
    pub degraded_detection: bool,
}
```

### RouterClient::prompt()

1. Try primary provider
2. On retryable error (429/5xx) → mark provider in cooldown, try next fallback
3. Skip providers currently in cooldown
4. On degraded response (too short, repetitive) → retry on next provider
5. If all providers exhausted → return last error

### Configuration

```toml
[agent]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"

[agent.router]
cooldown_secs = 60
max_retries = 2

[[agent.router.fallback]]
provider = "openai"
api_key_env = "OPENAI_API_KEY"
model = "gpt-4o"

[agent.coder]
model = "claude-sonnet-4-5-20250514"

[agent.evaluator]
model = "claude-haiku-4-5-20251001"
```

### Complexity Scoring (Phase 2)

Port ClawRouter's heuristic scorer — pure string analysis, no LLM call, sub-1ms:

- Token count weight
- Code presence (backticks, keywords)
- Reasoning markers ("step by step", "analyze")
- Multi-step patterns ("first...then...finally")
- Tool-use indicators

Score maps to tiers: `Simple | Medium | Complex`

Each tier has a preferred model. Simple evals → Haiku. Complex coding → Sonnet/Opus.

## User Stories

### US-007-01: RouterClient with failover

Add `RouterClient` struct that wraps one or more `ProviderClient` instances. Implements the same `prompt()` signature. On retryable errors (429, 502, 503, 504), tries the next provider in the fallback chain. If primary succeeds, returns immediately.

**Acceptance Criteria:**
- RouterClient exposes `prompt()` with same signature as ProviderClient
- On 429/5xx from primary, automatically tries fallback providers
- Returns last error if all providers fail
- Unit test: primary fails → fallback succeeds → returns fallback result
- Unit test: all providers fail → returns error

### US-007-02: Cooldown tracking

When a provider returns a retryable error, mark it in cooldown for a configurable duration. Skip providers in cooldown when selecting the next provider to try.

**Acceptance Criteria:**
- Cooldown map tracks provider → expiry time
- Providers in cooldown are skipped during selection
- Cooldown duration configurable via `RouterConfig.cooldown_secs`
- Expired cooldowns are automatically cleared
- Unit test: provider in cooldown is skipped
- Unit test: cooldown expires, provider is retried

### US-007-03: Router configuration parsing

Add `[agent.router]` section to config with fallback providers, cooldown, and retry settings. Parse into `RouterConfig`. Backward-compatible — missing section means no routing (direct to primary).

**Acceptance Criteria:**
- `RouterConfig` struct with `cooldown_secs`, `max_retries`, `fallback` list
- Each fallback has `provider`, `api_key_env`, `model`
- Missing `[agent.router]` section → no routing, existing behavior preserved
- Unit test: parse config with router section
- Unit test: parse config without router section (backward compat)

### US-007-04: Wire RouterClient into Orchestrator

Replace direct `ProviderClient` usage in `run.rs` with `RouterClient` when router config is present. Planner, coder, and evaluator each get their own `RouterClient` with appropriate fallback models.

**Acceptance Criteria:**
- `create_coder_client()` returns `RouterClient` when router config present
- `create_evaluator_client()` returns `RouterClient` when router config present
- `create_planner_client()` returns `RouterClient` when router config present
- Without router config, behavior is identical to current (returns plain ProviderClient)
- No changes to CoderAgent, EvaluatorAgent, or PlannerAgent

### US-007-05: Degraded response detection

Detect degraded LLM responses (too short, repetitive, placeholder text) and automatically retry on the next provider in the fallback chain.

**Acceptance Criteria:**
- `is_degraded()` function checks: response < 20 chars, repetitive patterns, known placeholder phrases
- When degraded response detected, retry on next provider
- Configurable via `RouterConfig.degraded_detection` (default: true)
- Unit test: short response detected as degraded
- Unit test: repetitive response detected as degraded
- Unit test: normal response passes through

### US-007-06: Complexity scoring for model selection

Pure heuristic scorer that classifies prompt complexity into tiers (Simple, Medium, Complex). Each tier maps to a preferred model. No LLM call — sub-1ms string analysis.

**Acceptance Criteria:**
- `score_complexity(input: &str) -> Complexity` function
- Dimensions: token count, code presence, reasoning markers, multi-step patterns
- Returns `Simple | Medium | Complex` enum
- Each tier maps to a model in config
- Unit test: short simple question → Simple
- Unit test: multi-step code task → Complex
- Unit test: scoring is deterministic and fast

## Quality Gates

- `cargo test -p rewind-cn -p rewind-cn-core`
- `cargo clippy -p rewind-cn -p rewind-cn-core`
- `make check`

## Non-Goals

- No crypto/wallet payments (we use standard API keys)
- No external proxy process (routing is in-process)
- No response caching (evaluator prompts are unique per task)
- No session pinning (tasks are independent)
