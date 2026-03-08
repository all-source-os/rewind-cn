---
name: rewind-beads
description: "Convert PRDs to beads for rewind execution using chronis (cn CLI). Creates an epic with child beads for each user story. Use when you have a PRD and want to use rewind with chronis as the task source. Triggers on: rewind beads, create beads, convert prd to beads, beads for rewind, cn beads, cn create."
---

# Rewind - Create Beads (chronis)

Converts PRDs to beads (epic + child tasks) for rewind autonomous execution using **chronis** (`cn` CLI).

---

## The Job

Take a PRD (markdown file or text) and create beads using `cn` commands:
1. **Detect project tooling** (build runner, package manager, language)
2. **Classify Quality Gates** into story-specific vs. epic-level
3. Create an **epic** bead (with epic-level quality gates)
4. Create **child beads** for each user story (with story-specific acceptance criteria only)
5. Set up **dependencies** between beads
6. Output ready for `rewind run --tracker chronis`

---

## Step 0: Detect Project Tooling

Before creating beads, detect the project's build runner and language tooling. This determines which commands appear in quality gates.

### Build runner detection

Check in order:
1. `Taskfile.yml` or `Taskfile.yaml` exists → use `task` (go-task)
2. `Makefile` exists → use `make`
3. Neither → use raw commands

```bash
# Detect build runner
if [ -f Taskfile.yml ] || [ -f Taskfile.yaml ]; then
  RUNNER="task"    # e.g., task ci, task lint
elif [ -f Makefile ]; then
  RUNNER="make"    # e.g., make ci, make lint
else
  RUNNER="none"    # use raw commands directly
fi
```

When detected, scan the file for available targets to use in quality gates:
- `task`: read `Taskfile.yml` for task names (e.g., `task ci`, `task lint`, `task test`)
- `make`: read `Makefile` for target names (e.g., `make ci`, `make lint`, `make test`)

### Language tooling detection

Check what's in the project:
- `Cargo.toml` → Rust project: use `cargo test`, `cargo clippy`, `cargo build`
- `package.json` → JS/TS project: use `bun` as package manager (not npm/pnpm)
- `mix.exs` → Elixir project: use `mix test`, `mix format`
- `go.mod` → Go project: use `go test`, `go vet`

For **Rust projects** (like rewind itself):

| What | Command |
|------|---------|
| Build | `cargo build` |
| Tests | `cargo test` |
| Lint | `cargo clippy` |
| Format check | `cargo fmt --check` |
| Single test | `cargo test test_name` |
| Full CI | `task ci` / `make ci` / `cargo test && cargo clippy` |

For **JS/TS projects**, always use **bun**:
- `bun install`, `bun test`, `bun typecheck`, `bun lint`
- `bunx <package>` (not npx)

### E2E testing with Playwright

If the project has Playwright configured (check for `playwright.config.ts` or `@playwright/test` in package.json):
- **E2E tests**: `bunx playwright test`
- **Single test file**: `bunx playwright test tests/my-feature.spec.ts`
- **Story-level**: specific test file for the story
- **Epic-level**: full suite on completion

---

## Quality Gates: Two Tiers

Quality gates split into two tiers based on scope:

### Story-Level Gates (checked per bead)

Criteria that verify THIS story's specific deliverable. The agent MUST check each item individually and mark `- [x]` before closing the bead.

Examples:
- `- [ ] cargo test test_new_handler passes`
- `- [ ] New event variant added to RewindEvent enum and compiles`
- `- [ ] Add investorType column with default 'cold'`
- `- [ ] Verify in browser using dev-browser skill` (UI stories only)
- `- [ ] Playwright e2e test passes: bunx playwright test tests/toggle.spec.ts` (when story has a specific test)

### Epic-Level Gates (checked on epic completion only)

General/universal commands that validate the whole codebase. These go in the **epic description**, NOT in individual stories. They run once when all stories are done.

Examples:
- `cargo test` (full test suite)
- `cargo clippy` (no warnings)
- `cargo fmt --check` (formatting)
- `task ci` / `make ci`
- `bun typecheck` / `bun lint`
- `bunx playwright test` (full e2e suite)

**Why?** Running `cargo test` after every single story wastes agent context and time. Intermediate stories may legitimately have temporary compile errors that the next story fixes.

### Extracting Gates from a PRD

Look for the "Quality Gates" section in the PRD. Classify each gate:
- `cargo test` → **epic-level** (general codebase check)
- `cargo clippy` → **epic-level** (general codebase check)
- `Unit test for specific handler` → **story-level** (specific to that story)
- `Verify in browser` → **story-level** (specific to UI stories)

If the project has a `task ci` or `make ci` target, prefer that as the single epic-level gate since it typically runs all checks.

**If no Quality Gates section exists:** Ask the user what commands should pass and how to classify them.

---

## Acceptance Criteria: Check Each Item

**CRITICAL:** The agent working on a bead MUST individually verify and mark each acceptance criteria item as done. This is not optional.

### How it works

Each bead's description contains `- [ ]` checkboxes. The agent must:

1. Work through each criterion
2. Verify it is satisfied (run a command, check output, inspect code)
3. Mark it `- [x]` in the bead description (via `cn edit --toon` or comment)
4. Only close the bead when ALL items are `- [x]`

### Writing verifiable criteria

Every criterion must be something the agent can concretely verify:

**Good (agent can check these):**
- `- [ ] File crates/rewind-cn-core/src/domain/new_module.rs exists and compiles`
- `- [ ] cargo test test_engine_roundtrip passes`
- `- [ ] New command struct added to application/commands.rs`
- `- [ ] Handler returns TaskDependencyAdded event (verify in unit test)`

**Bad (agent cannot verify these):**
- `- [ ] Works correctly`
- `- [ ] Good architecture`
- `- [ ] Handles edge cases`
- `- [ ] Performant`

---

## Output Format

Beads use `cn create` command with **HEREDOC syntax** to safely handle special characters:

```bash
# Create epic with epic-level quality gates
cn create --toon --type=epic \
  --title="[Feature Name]" \
  --description="$(cat <<'EOF'
[Feature description from PRD]

## Epic Quality Gates (run on completion)
- [ ] `cargo test` passes (all tests)
- [ ] `cargo clippy` passes (no warnings)
EOF
)" \
  --external-ref="prd:./tasks/feature-name-prd.md"

# Create child bead with story-specific criteria only
cn create --toon \
  --parent=EPIC_ID \
  --title="[Story Title]" \
  --description="$(cat <<'EOF'
[Story description]

## Acceptance Criteria
- [ ] Specific verifiable criterion 1
- [ ] Specific verifiable criterion 2
- [ ] Specific verifiable criterion 3

Mark each item [x] as you complete it. Only close when all are checked.
EOF
)" \
  --priority=[1-4]
```

> **CRITICAL:** Always use `<<'EOF'` (single-quoted) for the HEREDOC delimiter. This prevents shell interpretation of backticks, `$variables`, and `()` in descriptions.

---

## Story Size: The #1 Rule

**Each story must be completable in ONE rewind iteration (~one agent context window).**

Rewind spawns a fresh agent instance per iteration with no memory of previous work. If a story is too big, the agent runs out of context before finishing.

### Right-sized stories:
- Add a new event variant + handler
- Add a CLI subcommand
- Add a domain model type + tests
- Wire up an infrastructure adapter
- Add an MCP tool endpoint

### Too big (split these):
- "Build the entire CQRS engine" → Split into: events, aggregates, projections, engine wiring
- "Add full MCP server" → Split into: scaffold, tools, resources, transport
- "Refactor the domain" → Split into one story per aggregate/concept

**Rule of thumb:** If you can't describe the change in 2-3 sentences, it's too big.

---

## Story Ordering: Dependencies First

Stories execute in dependency order. Earlier stories must not depend on later ones.

**Correct order for rewind's architecture:**
1. Domain changes (events, model, error types)
2. Application logic (commands, handlers, planning)
3. Infrastructure wiring (engine, adapters, bridges)
4. CLI integration (command parsing, output formatting)

---

## Dependencies with `cn dep add`

```bash
cn dep add --toon rewind-002 rewind-001  # US-002 depends on US-001
cn dep add --toon rewind-003 rewind-002  # US-003 depends on US-002
```

**Syntax:** `cn dep add <issue> <depends-on>` — the issue depends on (is blocked by) depends-on.

---

## Conversion Rules

1. **Detect tooling**: check for Cargo.toml, Taskfile.yml/Makefile, package.json
2. **Classify Quality Gates** from PRD into story-level vs. epic-level
3. **Epic-level gates** go in the epic description (`cargo test`, `cargo clippy`, `task ci`)
4. **Story-level gates** go in individual bead descriptions (specific unit tests, browser verification)
5. **Each user story → one bead** with only story-specific acceptance criteria
6. **Every criterion** must be a `- [ ]` checkbox the agent can verify and mark `- [x]`
7. **First story**: No dependencies (creates foundation)
8. **Subsequent stories**: Depend on their predecessors
9. **Priority**: Based on dependency order, then document order (1=high, 2=medium, 3=low)
10. **All stories**: Include the instruction to mark items `[x]` and only close when all checked

---

## Example

**Input PRD:**
```markdown
# PRD: Task Dependency Support

Add dependency tracking between tasks.

## Quality Gates

### Epic-Level
- `cargo test` — all tests pass
- `cargo clippy` — no warnings

### Story-Level
- Domain stories: unit test for the specific handler
- Infrastructure stories: integration test with engine

## User Stories

### US-001: Add dependency tracking to domain [Domain]
- [ ] Add dependencies field to task model
- [ ] Add TaskDependencyAdded event variant
- [ ] Unit test: dependency stored correctly

### US-002: Cycle detection [Application]
- [ ] Implement cycle detection in handler
- [ ] Return CyclicDependency error on cycle
- [ ] Unit test: A→B→C→A rejected

### US-003: Dependency-aware scheduler [Application]
- [ ] Scheduler skips blocked tasks
- [ ] Unit test: blocked task not returned
```

**Tooling detected:** `Cargo.toml` exists → Rust project. No Taskfile or Makefile → use raw commands.

**Gate classification:**
- `cargo test` → epic-level
- `cargo clippy` → epic-level
- `Unit test for specific handler` → story-level

**Output beads:**
```bash
# Create epic with epic-level quality gates
cn create --toon --type=epic \
  --title="Task Dependency Support" \
  --description="$(cat <<'EOF'
Add dependency tracking between tasks for correct execution ordering.

## Epic Quality Gates (run on completion)
- [ ] `cargo test` passes (all tests)
- [ ] `cargo clippy` passes (no warnings)
EOF
)" \
  --external-ref="prd:./tasks/prd-task-dependencies.md"

# US-001: Domain story (no deps)
cn create --toon --parent=rewind-abc \
  --title="US-001: Add dependency tracking to domain model" \
  --description="$(cat <<'EOF'
As an orchestrator, I need to track task dependencies for execution ordering.

## Acceptance Criteria
- [ ] Add `dependencies: Vec<TaskId>` field to task model
- [ ] Add `TaskDependencyAdded` event variant to RewindEvent
- [ ] Unit test: dependency stored correctly (cargo test test_dependency_stored)

Mark each item [x] as you complete it. Only close when all are checked.
EOF
)" \
  --priority=1

# US-002: Application story (depends on US-001)
cn create --toon --parent=rewind-abc \
  --title="US-002: Cycle detection" \
  --description="$(cat <<'EOF'
As an orchestrator, I need to reject dependency cycles to prevent deadlocks.

## Acceptance Criteria
- [ ] Implement cycle detection in add_dependency handler
- [ ] Return RewindError::CyclicDependency on cycle
- [ ] Unit test: A→B→C→A detected and rejected

Mark each item [x] as you complete it. Only close when all are checked.
EOF
)" \
  --priority=2

cn dep add --toon rewind-002 rewind-001

# US-003: Application story (depends on US-001)
cn create --toon --parent=rewind-abc \
  --title="US-003: Dependency-aware scheduler" \
  --description="$(cat <<'EOF'
As an orchestrator, I need the scheduler to skip blocked tasks.

## Acceptance Criteria
- [ ] Scheduler skips tasks whose dependencies are not completed
- [ ] Unit test: blocked task not returned by next_task()
- [ ] Unit test: task returned after all dependencies complete

Mark each item [x] as you complete it. Only close when all are checked.
EOF
)" \
  --priority=2

cn dep add --toon rewind-003 rewind-001
```

---

## Syncing Changes

After creating beads, sync to export to JSONL (for git tracking):

```bash
cn sync --toon --flush-only
```

---

## Output Location

Beads are stored in: `.beads/` directory (SQLite DB + JSONL export)

After creation, run rewind:
```bash
rewind run --tracker chronis --epic rewind-abc
```

Rewind will:
1. Pick an unblocked bead, spawn an agent
2. Agent works through each `- [ ]` item, marking `- [x]` as verified
3. Agent closes the bead only when all criteria are `- [x]`
4. When all child beads are closed, rewind runs epic-level quality gates
5. If epic gates pass, closes the epic and outputs `<promise>COMPLETE</promise>`
6. If epic gates fail, creates a fix-up bead to address the failures

---

## Checklist Before Creating Beads

- [ ] Detected language tooling (Cargo.toml → Rust, package.json → JS/TS)
- [ ] Detected build runner (Taskfile.yml → `task`, Makefile → `make`, or raw commands)
- [ ] Using correct package manager (`cargo` for Rust, `bun`/`bunx` for JS/TS)
- [ ] Quality gates classified: story-level vs. epic-level
- [ ] Epic description includes epic-level gates as `- [ ]` checkboxes
- [ ] Each story has ONLY story-specific criteria (no cargo test/clippy in stories)
- [ ] Every criterion is a verifiable `- [ ]` checkbox
- [ ] Each story includes "Mark each item [x]..." instruction
- [ ] Stories are right-sized (one agent context window)
- [ ] Dependencies set with `cn dep add`
- [ ] Ran `cn sync --toon --flush-only`

---

## CLI Command Reference

| Command | Purpose |
|---------|---------|
| `cn create --toon` | Create a new bead (epic or story) |
| `cn create --toon --type=epic` | Create an epic bead |
| `cn create --toon --parent=ID` | Create a child bead under an epic |
| `cn dep add --toon <issue> <blocked-by>` | Add a dependency |
| `cn sync --toon --flush-only` | Export to JSONL for git tracking |
| `cn close --toon <id>` | Close a completed bead |
| `cn edit --toon <id>` | Edit a bead's description |
| `cn ready --toon` | Get next available (open + unblocked) task |
| `cn claim <id> --toon` | Claim a task for execution |
| `cn done <id> --toon` | Mark a task as complete |
