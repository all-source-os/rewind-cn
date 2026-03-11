# Profiling with hotpath.rs

Rewind integrates [hotpath.rs](https://hotpath.rs) for zero-cost, feature-gated performance profiling across the entire agent orchestration pipeline.

## Quick Start

```bash
# Build with profiling enabled
cargo build --features=hotpath

# Build with profiling + allocation tracking
cargo build --features=hotpath,hotpath-alloc

# Run with profiling (shows live TUI dashboard on exit)
cargo run --features=hotpath,hotpath-alloc -- run --max-concurrent 3
```

Without `--features=hotpath`, all instrumentation compiles away to nothing. Zero runtime overhead in production.

## What's Instrumented

### Tier 1: LLM Calls (highest latency)

These are external API calls to Anthropic — typically 2-30s each.

| Function | File | What it measures |
|----------|------|------------------|
| `CoderAgent::execute_task` | `infrastructure/coder.rs` | Full multi-turn tool-use conversation |
| `PlannerAgent::decompose` | `infrastructure/planner.rs` | Single-shot PRD decomposition |
| `EvaluatorAgent::evaluate` | `infrastructure/evaluator.rs` | Completion judgment call |

### Tier 2: Orchestrator Loop

The core scheduling and execution loop.

| Function | File | What it measures |
|----------|------|------------------|
| `execute_task` | `infrastructure/orchestrator.rs` | Full SELECT-PROMPT-EXECUTE-EVALUATE cycle |
| `execute_runnable` | `infrastructure/orchestrator.rs` | Sequential task batch execution |
| `execute_task_in_dir` | `infrastructure/orchestrator.rs` | Worktree-isolated task execution |
| `execute_parallel` | `infrastructure/orchestrator.rs` | Parallel execution with semaphore |

### Tier 3: Event Store (CQRS critical path)

Every command and event flows through these.

| Function | File | What it measures |
|----------|------|------------------|
| `dispatch_and_append` | `infrastructure/engine.rs` | Command dispatch + event persist + projection update |
| `apply_to_projections` | `infrastructure/engine.rs` | Broadcast to 3 projections + event channel |
| `append_events` | `infrastructure/engine.rs` | Direct event append (tool calls, criteria checks) |
| `rebuild_projections` | `infrastructure/engine.rs` | Full event replay on startup |

### Tier 4: Agent Tools

I/O-bound operations the coder agent invokes.

| Function | File | What it measures |
|----------|------|------------------|
| `ReadFileTool::call` | `infrastructure/coder.rs` | `tokio::fs::read_to_string` |
| `WriteFileTool::call` | `infrastructure/coder.rs` | `tokio::fs::write` with dir creation |
| `ListFilesTool::call` | `infrastructure/coder.rs` | Directory listing |
| `SearchCodeTool::call` | `infrastructure/coder.rs` | Grep-based code search |
| `RunCommandTool::call` | `infrastructure/coder.rs` | Shell command with timeout |

### Tier 5: Quality Gates

| Function | File | What it measures |
|----------|------|------------------|
| `run_gate` | `infrastructure/gate_runner.rs` | Single gate command execution |
| `run_epic_gates` | `infrastructure/gate_runner.rs` | Full epic gate sweep |

## Reading the Output

When the process exits, hotpath prints a summary table:

```
Function                    | Calls | Avg     | Total   | p95     | p99     | % Time
----------------------------|-------|---------|---------|---------|---------|-------
CoderAgent::execute_task    |     5 | 12.3s   | 61.5s   | 18.1s   | 18.1s   | 72.1%
EvaluatorAgent::evaluate    |     5 |  2.1s   | 10.5s   |  3.2s   |  3.2s   | 12.3%
RunCommandTool::call        |    23 |  1.8s   |  8.2s   |  5.1s   |  5.1s   |  9.6%
dispatch_and_append         |    31 |  12ms   | 372ms   |  25ms   |  25ms   |  0.4%
rebuild_projections         |     6 |   8ms   |  48ms   |  15ms   |  15ms   |  0.1%
...
```

With `hotpath-alloc`, you also get per-function allocation counts and bytes.

## Common Profiling Scenarios

### "My orchestrator run is slow"

Look at `execute_task` call count and average time. If the average is high but call count is low, the LLM is slow. If call count is high, check whether failed tasks are being retried excessively.

### "Startup takes too long"

Check `rebuild_projections`. If it's slow, you have a large event store. Consider archiving old sessions.

### "Parallel execution isn't faster"

Check `execute_parallel` vs individual `execute_task_in_dir` times. If the semaphore is the bottleneck, increase `--max-concurrent`. If worktree creation is slow, check disk I/O.

### "Agent is wasting context on tool calls"

Check `ReadFileTool::call` and `SearchCodeTool::call` counts. A high call count means the agent is exploring too much before acting. Consider improving the coder prompt with more context.

## Configuration

Profiling is controlled entirely by Cargo feature flags:

```toml
# Cargo.toml (already configured)
[features]
hotpath = ["hotpath/hotpath"]
hotpath-alloc = ["hotpath/hotpath-alloc"]
```

| Feature | What it enables |
|---------|----------------|
| `hotpath` | Function timing, call counts, percentiles |
| `hotpath-alloc` | Per-function memory allocation tracking |

Both features are additive. Use `hotpath` alone for minimal overhead, add `hotpath-alloc` when investigating memory.

## CI Integration

hotpath supports GitHub Actions for automated benchmarking. See [hotpath.rs docs](https://hotpath.rs) for CI setup.

For local regression checks:

```bash
# Run a known workload and compare
cargo run --features=hotpath -- run --task <id> 2>&1 | tee profile-before.txt
# ... make changes ...
cargo run --features=hotpath -- run --task <id> 2>&1 | tee profile-after.txt
diff profile-before.txt profile-after.txt
```
