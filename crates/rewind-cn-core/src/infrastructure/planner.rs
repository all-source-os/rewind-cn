use async_trait::async_trait;
use tracing::warn;

use crate::application::planning::{Plan, PlanGenerator};
use crate::domain::error::RewindError;
use crate::infrastructure::llm::{AgentConfig, ProviderClient};
use crate::infrastructure::sanitize::sanitize_user_content;

const MAX_PARSE_RETRIES: usize = 2;

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

/// Strip markdown code fences from LLM response and parse as JSON.
fn strip_fences_and_parse(response: &str) -> Result<Plan, serde_json::Error> {
    let trimmed = response.trim();
    let json_str = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s: &str| s.strip_suffix("```"))
        .unwrap_or(trimmed);
    serde_json::from_str::<Plan>(json_str)
}

/// Build a corrective prompt that includes the original input and the parse error.
fn corrective_prompt(original_input: &str, error: &serde_json::Error) -> String {
    format!(
        "{original_input}\n\n\
        Your previous response was not valid JSON. \
        The parse error was: {error}\n\
        Please respond with valid JSON only."
    )
}

#[async_trait]
impl PlanGenerator for PlannerAgent {
    #[hotpath::measure]
    async fn decompose(&self, input: &str) -> Result<Plan, RewindError> {
        let sanitized_input = sanitize_user_content(input);
        let mut user_prompt = sanitized_input.clone();
        let mut last_error: Option<serde_json::Error> = None;
        let mut last_response = String::new();

        for attempt in 0..=MAX_PARSE_RETRIES {
            let response = self
                .client
                .prompt(
                    &self.config.planner.model,
                    PLANNER_SYSTEM_PROMPT,
                    self.config.planner.max_tokens as u64,
                    &user_prompt,
                )
                .await?;

            match strip_fences_and_parse(&response) {
                Ok(plan) => return Ok(plan),
                Err(e) => {
                    last_response = response;
                    if attempt < MAX_PARSE_RETRIES {
                        warn!(
                            attempt = attempt + 1,
                            max_retries = MAX_PARSE_RETRIES,
                            error = %e,
                            "Planner returned invalid JSON, retrying with corrective prompt"
                        );
                        user_prompt = corrective_prompt(&sanitized_input, &e);
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(RewindError::Config(format!(
            "Failed to parse planner output as Plan after {MAX_PARSE_RETRIES} retries: {}\n\nRaw output:\n{last_response}",
            last_error.unwrap(),
        )))
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

    const VALID_PLAN_JSON: &str = r#"{
        "epic_title": "Test Feature",
        "epic_description": "A test",
        "quality_gates": [],
        "stories": [{
            "title": "US-001: Do it [Backend]",
            "description": "Do it",
            "story_type": "Backend",
            "acceptance_criteria": ["Done"],
            "depends_on": []
        }]
    }"#;

    #[test]
    fn strip_fences_and_parse_valid_json() {
        let plan = strip_fences_and_parse(VALID_PLAN_JSON).unwrap();
        assert_eq!(plan.epic_title, "Test Feature");
    }

    #[test]
    fn strip_fences_and_parse_invalid_json() {
        assert!(strip_fences_and_parse("not json at all").is_err());
    }

    #[test]
    fn corrective_prompt_contains_error_info() {
        let err = strip_fences_and_parse("bad").unwrap_err();
        let prompt = corrective_prompt("original input", &err);
        assert!(prompt.starts_with("original input"));
        assert!(prompt.contains("not valid JSON"));
    }

    #[tokio::test]
    async fn planner_retry_succeeds_on_second_attempt() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();
        let valid = VALID_PLAN_JSON.to_string();

        let client = ProviderClient::Mock(Arc::new(move |_, _, _, _| {
            let n = cc.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                "not json".into()
            } else {
                valid.clone()
            }
        }));

        let config = AgentConfig::default();
        let agent = PlannerAgent::new(client, config);

        let result = agent.decompose("test input").await;
        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn planner_fails_after_max_retries() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();

        let client = ProviderClient::Mock(Arc::new(move |_, _, _, _| {
            cc.fetch_add(1, Ordering::SeqCst);
            "never valid".into()
        }));

        let config = AgentConfig::default();
        let agent = PlannerAgent::new(client, config);

        let result = agent.decompose("test input").await;
        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 3); // 1 initial + 2 retries
    }

    #[tokio::test]
    async fn planner_no_retry_on_first_success() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let call_count = Arc::new(AtomicUsize::new(0));
        let cc = call_count.clone();
        let valid = VALID_PLAN_JSON.to_string();

        let client = ProviderClient::Mock(Arc::new(move |_, _, _, _| {
            cc.fetch_add(1, Ordering::SeqCst);
            valid.clone()
        }));

        let config = AgentConfig::default();
        let agent = PlannerAgent::new(client, config);

        let result = agent.decompose("test input").await;
        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }
}
