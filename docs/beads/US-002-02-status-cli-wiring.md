# US-002-02: Wire `rewind status` CLI command

**Parent:** US-002 (`rewind status`)
**Size:** S
**Depends on:** US-002-01

## Goal
Implement the `rewind status` command end-to-end: load engine, rebuild projections, print summary.

## Tasks
1. Add `--json` flag to `Commands::Status` in `main.rs` (clap arg)
2. Implement `commands/status.rs`:
   - Check `.rewind/` exists, error if not
   - Load config from `.rewind/rewind.toml`
   - `RewindEngine::load(".rewind/data/")` → `rebuild_projections()`
   - Call `build_summary()` from US-002-01
   - If `--json`: `serde_json::to_string_pretty(&summary)` → stdout
   - Else: format as plain text table (manual formatting, no external crate needed for v1)
3. Plain text format:
   ```
   Project: rewind-project

   Tasks: 5 total
     pending:     2
     in-progress: 1
     completed:   2

   Epics:
     Sprint 1  [████░░░░░░] 40% (2/5)
   ```
4. Integration test: init engine in temp dir, create tasks via engine, run status, verify output

## Files touched
- `crates/rewind-cn/src/main.rs` (modify — add --json arg)
- `crates/rewind-cn/src/commands/status.rs` (rewrite)

## Done when
- `rewind init && rewind status` prints "Tasks: 0 total" (empty project)
- `rewind status --json` outputs valid JSON
- `rewind status` without init exits with error message
