use async_trait::async_trait;

use crate::application::planning::{Plan, PlanGenerator};
use crate::domain::error::RewindError;
use crate::infrastructure::llm::{AgentConfig, ProviderClient};

const PLANNER_SYSTEM_PROMPT: &str = r#"You are a software project planner. Given a feature description or PRD, decompose it into an epic with child stories.

## Output Format
Return ONLY a JSON object with this exact structure (no markdown, no code fences):

{
  "epic_title": "Short descriptive title for the epic",
  "epic_description": "Detailed description of the overall feature",
  "quality_gates": [
    {"command": "cargo test", "tier": "Epic"},
    {"command": "cargo clippy -- -D warnings", "tier": "Epic"}
  ],
  "stories": [
    {
      "title": "US-001: Short title [Type]",
      "description": "As a [user], I want [feature] so that [benefit].\n\nDetails...",
      "story_type": "Schema",
      "acceptance_criteria": [
        "Specific verifiable criterion 1",
        "Specific verifiable criterion 2"
      ],
      "depends_on": []
    },
    {
      "title": "US-002: Another story [Type]",
      "description": "...",
      "story_type": "Backend",
      "acceptance_criteria": ["..."],
      "depends_on": [0]
    }
  ]
}

## Rules

1. **Right-sized stories**: Each story must be completable in one focused coding session (~1 agent context window). If a story is too big, split it.
2. **Story types**: Use one of: Schema, Backend, UI, Integration, Infrastructure
3. **Verifiable criteria**: Every acceptance criterion must be something an agent can concretely verify (file exists, test passes, endpoint returns expected response). Avoid vague criteria like "works correctly" or "good UX".
4. **Dependencies**: `depends_on` contains 0-based indices into the stories array. Schema stories come first, then backend, then UI.
5. **Quality gates**: Two tiers:
   - `Epic` tier: General codebase checks (typecheck, lint, test suite) — run once when all stories complete
   - `Story` tier: Story-specific checks — run per story
6. **Ordering**: Schema → Backend → UI → Integration. Earlier stories should not depend on later ones.
7. **3-10 stories**: Aim for 3-10 stories per epic. Fewer than 3 means the stories are too big. More than 10 means the scope is too large — suggest splitting into multiple epics.
8. **Title format**: "US-NNN: Short description [Type]" where Type matches story_type
"#;

/// LLM-powered plan generator using rig-core.
pub struct PlannerAgent {
    client: ProviderClient,
    config: AgentConfig,
}

impl PlannerAgent {
    pub fn new(client: ProviderClient, config: AgentConfig) -> Self {
        Self { client, config }
    }
}

#[async_trait]
impl PlanGenerator for PlannerAgent {
    #[hotpath::measure]
    async fn decompose(&self, input: &str) -> Result<Plan, RewindError> {
        let response: String = self
            .client
            .prompt(
                &self.config.planner.model,
                PLANNER_SYSTEM_PROMPT,
                self.config.planner.max_tokens as u64,
                input,
            )
            .await?;

        // Strip markdown code fences if present
        let trimmed = response.trim();
        let json_str = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .and_then(|s: &str| s.strip_suffix("```"))
            .unwrap_or(trimmed);

        serde_json::from_str::<Plan>(json_str).map_err(|e| {
            RewindError::Config(format!(
                "Failed to parse planner output as Plan: {e}\n\nRaw output:\n{response}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::events::{GateTier, StoryType};

    #[test]
    fn parse_plan_from_llm_json() {
        let llm_response = r#"{
            "epic_title": "User Authentication",
            "epic_description": "Implement OAuth2-based authentication with JWT sessions",
            "quality_gates": [
                {"command": "cargo test", "tier": "Epic"},
                {"command": "cargo clippy -- -D warnings", "tier": "Epic"}
            ],
            "stories": [
                {
                    "title": "US-001: Add users table [Schema]",
                    "description": "Create users table with email, password_hash, created_at columns",
                    "story_type": "Schema",
                    "acceptance_criteria": [
                        "Migration file creates users table with email, password_hash, created_at",
                        "Migration runs successfully against test database"
                    ],
                    "depends_on": []
                },
                {
                    "title": "US-002: Add auth middleware [Backend]",
                    "description": "JWT verification middleware that extracts user from token",
                    "story_type": "Backend",
                    "acceptance_criteria": [
                        "Middleware rejects requests without Authorization header with 401",
                        "Middleware rejects expired JWT tokens with 401",
                        "Middleware sets user_id in request context on valid JWT"
                    ],
                    "depends_on": [0]
                },
                {
                    "title": "US-003: Login endpoint [Backend]",
                    "description": "POST /api/auth/login endpoint that validates credentials and returns JWT",
                    "story_type": "Backend",
                    "acceptance_criteria": [
                        "POST /api/auth/login with valid credentials returns 200 with JWT in body",
                        "POST /api/auth/login with invalid credentials returns 401"
                    ],
                    "depends_on": [0, 1]
                }
            ]
        }"#;

        let plan: Plan = serde_json::from_str(llm_response).unwrap();

        assert_eq!(plan.epic_title, "User Authentication");
        assert_eq!(plan.quality_gates.len(), 2);
        assert_eq!(plan.quality_gates[0].command, "cargo test");
        assert_eq!(plan.quality_gates[0].tier, GateTier::Epic);
        assert_eq!(plan.stories.len(), 3);

        assert_eq!(plan.stories[0].story_type, Some(StoryType::Schema));
        assert_eq!(plan.stories[0].acceptance_criteria.len(), 2);
        assert!(plan.stories[0].depends_on.is_empty());

        assert_eq!(plan.stories[1].story_type, Some(StoryType::Backend));
        assert_eq!(plan.stories[1].depends_on, vec![0]);

        assert_eq!(plan.stories[2].depends_on, vec![0, 1]);
    }

    #[test]
    fn parse_plan_with_code_fences_stripped() {
        let llm_response = r#"```json
{
    "epic_title": "Simple Feature",
    "epic_description": "A simple feature",
    "quality_gates": [],
    "stories": [
        {
            "title": "US-001: Do the thing [Backend]",
            "description": "Do it",
            "story_type": "Backend",
            "acceptance_criteria": ["It is done"],
            "depends_on": []
        }
    ]
}
```"#;

        // Simulate the stripping logic
        let json_str = llm_response
            .trim()
            .strip_prefix("```json")
            .or_else(|| llm_response.trim().strip_prefix("```"))
            .and_then(|s| s.strip_suffix("```"))
            .unwrap_or(llm_response.trim());

        let plan: Plan = serde_json::from_str(json_str).unwrap();
        assert_eq!(plan.epic_title, "Simple Feature");
        assert_eq!(plan.stories.len(), 1);
    }

    #[test]
    fn system_prompt_is_reasonable() {
        assert!(PLANNER_SYSTEM_PROMPT.contains("quality_gates"));
        assert!(PLANNER_SYSTEM_PROMPT.contains("acceptance_criteria"));
        assert!(PLANNER_SYSTEM_PROMPT.contains("depends_on"));
        assert!(PLANNER_SYSTEM_PROMPT.contains("story_type"));
        assert!(PLANNER_SYSTEM_PROMPT.contains("Right-sized stories"));
    }
}
