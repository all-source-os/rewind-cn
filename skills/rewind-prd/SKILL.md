---
name: rewind-prd
description: "Generate a Product Requirements Document (PRD) for rewind task orchestration using chronis (cn CLI). Creates PRDs with user stories that can be converted to beads for automated execution. Triggers on: rewind prd, create a prd, write prd for, plan this feature, requirements for, spec out."
---

# Rewind PRD Generator

Create detailed Product Requirements Documents optimized for AI agent execution via rewind with the chronis (`cn`) CLI.

---

## The Job

1. Receive a feature description from the user
2. Ask 3-5 essential clarifying questions (with lettered options) - one set at a time
3. **Always ask about quality gates** — classify into epic-level vs. story-level
4. After each answer, ask follow-up questions if needed (adaptive exploration)
5. Generate a structured PRD when you have enough context
6. Output the PRD wrapped in `[PRD]...[/PRD]` markers for parsing

**Important:** Do NOT start implementing. Just create the PRD.

---

## Step 1: Clarifying Questions (Iterative)

Ask questions one set at a time. Each answer should inform your next questions. Focus on:

- **Problem/Goal:** What problem does this solve?
- **Core Functionality:** What are the key actions?
- **Scope/Boundaries:** What should it NOT do?
- **Success Criteria:** How do we know it's done?
- **Integration:** How does it fit with existing features?
- **Quality Gates:** What commands validate success, and which tier? (REQUIRED)

### Format Questions Like This:

```
1. What is the primary goal of this feature?
   A. Improve user onboarding experience
   B. Increase user retention
   C. Reduce support burden
   D. Other: [please specify]

2. Who is the target user?
   A. New users only
   B. Existing users only
   C. All users
   D. Admin users only
```

This lets users respond with "1A, 2C" for quick iteration.

### Quality Gates Question (REQUIRED)

Always ask about quality gates with **two-tier classification**:

```
What commands validate the codebase? These run ONCE when all stories are done (epic-level):
   A. cargo test (Rust tests)
   B. cargo clippy (Rust linting)
   C. cargo test && cargo clippy
   D. task ci (go-task runner)
   E. make ci
   F. bun typecheck && bun lint (JS/TS)
   G. Other: [specify your commands]

For individual stories, what story-specific checks apply?
   A. Unit tests for the specific module
   B. Integration test for the specific feature
   C. UI stories: verify in browser using dev-browser skill
   D. Backend stories: curl/test the specific endpoint
   E. Other: [specify]
```

**Why two tiers?** Running `cargo test` after every single story wastes agent context and time. Intermediate stories may have temporary compile errors that the next story fixes. General checks run once at the end.

### Adaptive Questioning

After each response, decide whether to:
- Ask follow-up questions (if answers reveal complexity)
- Ask about a new aspect (if current area is clear)
- Generate the PRD (if you have enough context)

Typically 2-4 rounds of questions are needed.

---

## Step 2: PRD Structure

Generate the PRD with these sections:

### 1. Introduction/Overview
Brief description of the feature and the problem it solves.

### 2. Goals
Specific, measurable objectives (bullet list).

### 3. Quality Gates (Two Tiers)

**CRITICAL:** Split gates into two tiers. The conversion tool (rewind-beads) reads this section directly.

```markdown
## Quality Gates

### Epic-Level (run once on epic completion)
General codebase checks. These validate the whole project and run ONCE when all stories are done:
- `cargo test` — all tests pass
- `cargo clippy` — no warnings
- `task ci` — full CI pipeline

### Story-Level (checked per story)
Criteria checked on specific stories where relevant:
- **Domain stories:** Unit test for the specific aggregate/handler
- **Infrastructure stories:** Integration test passes
- **UI stories:** Verify in browser using dev-browser skill
```

### 4. User Stories

Each story needs:
- **Title:** Short descriptive name
- **Type tag:** `[Domain]`, `[Application]`, `[Infrastructure]`, `[CLI]`, `[Schema]`, `[Backend]`, `[UI]`, `[Integration]` — tells the conversion tool which story-level gates apply
- **Description:** "As a [user], I want [feature] so that [benefit]"
- **Acceptance Criteria:** Verifiable `- [ ]` checkboxes the agent marks as done
- **Mark-each-item instruction:** Reminder to verify and check off each item

Each story should be small enough to implement in one focused AI agent session.

**Format:**
```markdown
### US-001: [Title] [Domain]
**Description:** As a [user], I want [feature] so that [benefit].

**Acceptance Criteria:**
- [ ] Specific verifiable criterion
- [ ] Another criterion
- [ ] (story-level gate if applicable)

Mark each item [x] as you complete it. Only close when all are checked.
```

**Important — writing verifiable criteria:**

Every criterion must be something the agent can concretely verify:

**Good (agent can check these):**
- `- [ ] File crates/rewind-cn-core/src/domain/new_module.rs exists and compiles`
- `- [ ] cargo test test_new_handler passes`
- `- [ ] New event variant added to RewindEvent enum`
- `- [ ] Command handler returns correct events (verify in unit test)`

**Bad (agent cannot verify these):**
- `- [ ] Works correctly`
- `- [ ] Good architecture`
- `- [ ] Handles edge cases`
- `- [ ] Performant`

**Note:** Do NOT include epic-level gates (cargo test, cargo clippy) in individual story criteria — they are defined once in the Quality Gates section and run on epic completion only. Story-level gates (specific unit tests, browser verification) DO belong in the relevant stories.

### 5. Functional Requirements
Numbered list of specific functionalities:
- "FR-1: The system must allow users to..."
- "FR-2: When a user runs X, the system must..."

Be explicit and unambiguous.

### 6. Non-Goals (Out of Scope)
What this feature will NOT include. Critical for managing scope.

### 7. Technical Considerations (Optional)
- Known constraints or dependencies
- Integration points with existing systems (allframe, chronis, MCP)
- Architecture layer placement (domain, application, infrastructure)

### 8. Success Metrics
How will success be measured?

### 9. Open Questions
Remaining questions or areas needing clarification.

---

## Writing for AI Agents

The PRD will be executed by AI coding agents via rewind. Therefore:

- Be explicit and unambiguous
- User stories should be small (completable in one agent context window)
- Every acceptance criterion must be a `- [ ]` checkbox the agent can verify and mark `- [x]`
- Include specific file paths if you know them (use `crates/rewind-cn-core/src/` paths)
- Reference existing code patterns in the project
- Tag stories with type (`[Domain]`, `[Application]`, `[Infrastructure]`, `[CLI]`) so story-level gates are unambiguous
- Include "Mark each item [x]..." instruction in every story
- Respect rewind's clean architecture: domain → application → infrastructure

---

## Output Format

**CRITICAL:** Wrap the final PRD in markers for parsing:

```
[PRD]
# PRD: Feature Name

## Overview
...

## Quality Gates

### Epic-Level (run once on epic completion)
...

### Story-Level (checked per story)
...

## User Stories
...
[/PRD]
```

**File naming:** Save to `./tasks/prd-[feature-name].md`

---

## Example Conversation Flow

**User:** Create a PRD for adding task dependency support

**Assistant:** I'll help you create a PRD for task dependencies. Let me ask a few questions:

1. What kind of dependencies?
   A. Simple linear (task B waits for task A)
   B. DAG (task C waits for both A and B)
   C. Both linear and DAG
   D. Other

2. How should blocked tasks behave?
   A. Stay in pending state until unblocked
   B. Show a "blocked" status with reason
   C. Both A and B

**User:** 1C, 2C

**Assistant:** Great! A couple more questions:

3. Should dependency cycles be detected?
   A. Yes, reject on creation
   B. Yes, detect and warn but allow
   C. No detection needed
   D. Other

4. What commands validate the whole codebase? (epic-level, run once at the end)
   A. cargo test
   B. cargo test && cargo clippy
   C. task ci
   D. Other: [specify]

5. For individual stories, what story-specific checks?
   A. Unit test for the specific handler/module
   B. Integration test with the engine
   C. Both A and B
   D. No story-level checks needed

**User:** 3A, 4B, 5C

**Assistant:** Perfect, I have enough to generate the PRD.

[PRD]
# PRD: Task Dependency Support

## Overview
Add dependency tracking between tasks so that rewind can schedule work in correct execution order. Tasks can declare dependencies on other tasks, and blocked tasks show their status with a reason.

## Goals
- Support both linear and DAG dependency structures
- Detect and reject dependency cycles at creation time
- Show blocked status with clear reasoning in status display

## Quality Gates

### Epic-Level (run once on epic completion)
General codebase checks that run ONCE when all stories are done:
- `cargo test` — all tests pass
- `cargo clippy` — no warnings

### Story-Level (checked per story)
- **Domain/Application stories:** Unit test for the specific handler
- **Infrastructure stories:** Integration test with RewindEngine

## User Stories

### US-001: Add dependency tracking to domain model [Domain]
**Description:** As an orchestrator, I need to track which tasks depend on which so I can determine execution order.

**Acceptance Criteria:**
- [ ] Add `dependencies: Vec<TaskId>` field to task model
- [ ] Add `TaskDependencyAdded` event variant to RewindEvent
- [ ] Unit test: creating a dependency stores it correctly
- [ ] Unit test: querying dependencies returns correct list

Mark each item [x] as you complete it. Only close when all are checked.

### US-002: Add cycle detection [Application]
**Description:** As an orchestrator, I need to reject dependency cycles to prevent deadlocks.

**Acceptance Criteria:**
- [ ] Implement cycle detection in `add_dependency` handler
- [ ] Return `RewindError::CyclicDependency` on cycle
- [ ] Unit test: A→B→C→A detected and rejected
- [ ] Unit test: A→B, A→C (no cycle) accepted

Mark each item [x] as you complete it. Only close when all are checked.

### US-003: Update scheduler for dependency awareness [Application]
**Description:** As an orchestrator, I need the scheduler to skip blocked tasks.

**Acceptance Criteria:**
- [ ] Scheduler skips tasks whose dependencies aren't completed
- [ ] Unit test: blocked task not returned by `next_task()`
- [ ] Unit test: task returned after all dependencies complete

Mark each item [x] as you complete it. Only close when all are checked.

### US-004: Show blocked status in CLI [CLI]
**Description:** As a user, I want to see which tasks are blocked and why.

**Acceptance Criteria:**
- [ ] `rewind status` shows "blocked by: TASK-X, TASK-Y" for blocked tasks
- [ ] Blocked tasks visually distinct from pending tasks
- [ ] Integration test: status output includes dependency info

Mark each item [x] as you complete it. Only close when all are checked.

## Functional Requirements
- FR-1: Tasks can declare dependencies on other tasks via `TaskDependencyAdded` events
- FR-2: Adding a dependency that creates a cycle returns an error
- FR-3: The scheduler only returns tasks whose dependencies are all completed
- FR-4: `rewind status` displays blocked tasks with their blocking reasons

## Non-Goals
- Priority-based scheduling (future enhancement)
- Cross-epic dependencies
- Automatic dependency inference from code analysis

## Technical Considerations
- Dependencies live in domain layer as pure data
- Cycle detection uses DFS in application layer (no async needed)
- Scheduler queries BacklogProjection for dependency status
- Events follow existing pattern: `RewindEvent::TaskDependencyAdded { task_id, depends_on }`

## Success Metrics
- All dependency scenarios handled correctly
- No performance regression in scheduler
- Status display clearly shows blocked state

## Open Questions
- Should we support "soft" dependencies (warnings only)?
[/PRD]

---

## Checklist

Before outputting the PRD:

- [ ] Asked clarifying questions with lettered options
- [ ] Asked about quality gates with two-tier classification (REQUIRED)
- [ ] Asked follow-up questions when needed
- [ ] Quality Gates section has Epic-Level and Story-Level subsections
- [ ] Epic-level gates are general commands (cargo test, cargo clippy) — NOT in individual stories
- [ ] Story-level gates are assigned to relevant stories (domain → unit test, CLI → integration test)
- [ ] Stories tagged with type: `[Domain]`, `[Application]`, `[Infrastructure]`, `[CLI]`, or other appropriate tags
- [ ] Every acceptance criterion is a verifiable `- [ ]` checkbox
- [ ] Every story includes "Mark each item [x]..." instruction
- [ ] User stories are small and independently completable
- [ ] Functional requirements are numbered and unambiguous
- [ ] Non-goals section defines clear boundaries
- [ ] PRD is wrapped in `[PRD]...[/PRD]` markers
