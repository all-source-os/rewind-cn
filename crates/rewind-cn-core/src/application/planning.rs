use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::domain::error::RewindError;
use crate::domain::events::{QualityGate, StoryType};

/// The output of plan decomposition — an epic with child stories.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Plan {
    pub epic_title: String,
    pub epic_description: String,
    #[serde(default)]
    pub quality_gates: Vec<QualityGate>,
    pub stories: Vec<PlannedStory>,
}

/// A single story within a plan.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlannedStory {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub story_type: Option<StoryType>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    /// Indices into the `stories` vec that this story depends on (0-based).
    #[serde(default)]
    pub depends_on: Vec<usize>,
}

// Keep the old PlannedTask as a type alias for backwards compat in plan.rs CLI
pub type PlannedTask = PlannedStory;

/// Trait for plan generators — decompose a description into an epic with stories.
#[async_trait]
pub trait PlanGenerator: Send + Sync {
    async fn decompose(&self, input: &str) -> Result<Plan, RewindError>;
}

/// Phase 1 fallback: wraps the input as a single epic + single task.
/// Used when no LLM agent config is present.
pub struct PassthroughPlanGenerator;

#[async_trait]
impl PlanGenerator for PassthroughPlanGenerator {
    async fn decompose(&self, input: &str) -> Result<Plan, RewindError> {
        Ok(passthrough_plan(input))
    }
}

/// Phase 1: wraps the input as a single epic + single task.
pub fn passthrough_plan(input: &str) -> Plan {
    let first_line = input.lines().next().unwrap_or(input);
    let title = if first_line.len() > 80 {
        format!("{}...", &first_line[..77])
    } else {
        first_line.to_string()
    };

    Plan {
        epic_title: title.clone(),
        epic_description: input.to_string(),
        quality_gates: vec![],
        stories: vec![PlannedStory {
            title,
            description: input.to_string(),
            story_type: None,
            acceptance_criteria: vec![],
            depends_on: vec![],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::events::GateTier;

    #[test]
    fn passthrough_single_line() {
        let plan = passthrough_plan("Build auth");
        assert_eq!(plan.epic_title, "Build auth");
        assert_eq!(plan.stories.len(), 1);
        assert_eq!(plan.stories[0].title, "Build auth");
    }

    #[test]
    fn passthrough_multi_line() {
        let plan = passthrough_plan("Build auth\nNeeds OAuth + JWT");
        assert_eq!(plan.epic_title, "Build auth");
        assert_eq!(plan.stories[0].description, "Build auth\nNeeds OAuth + JWT");
    }

    #[test]
    fn passthrough_truncates_long_title() {
        let long = "x".repeat(100);
        let plan = passthrough_plan(&long);
        assert!(plan.epic_title.len() <= 80);
        assert!(plan.epic_title.ends_with("..."));
    }

    #[tokio::test]
    async fn passthrough_generator_trait() {
        let gen = PassthroughPlanGenerator;
        let plan = gen.decompose("Build auth").await.unwrap();
        assert_eq!(plan.epic_title, "Build auth");
        assert_eq!(plan.stories.len(), 1);
    }

    #[test]
    fn plan_deserializes_from_json() {
        let json = r#"{
            "epic_title": "Add user authentication",
            "epic_description": "Implement OAuth2 authentication flow",
            "quality_gates": [
                {"command": "cargo test", "tier": "Epic"},
                {"command": "cargo clippy", "tier": "Epic"}
            ],
            "stories": [
                {
                    "title": "US-001: Add auth schema",
                    "description": "Create users table and session model",
                    "story_type": "Schema",
                    "acceptance_criteria": [
                        "Migration file creates users table",
                        "Session model defined with expires_at field"
                    ],
                    "depends_on": []
                },
                {
                    "title": "US-002: Add auth middleware",
                    "description": "JWT verification middleware",
                    "story_type": "Backend",
                    "acceptance_criteria": [
                        "Middleware rejects requests without valid JWT",
                        "Middleware sets user context on valid JWT"
                    ],
                    "depends_on": [0]
                }
            ]
        }"#;

        let plan: Plan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.epic_title, "Add user authentication");
        assert_eq!(plan.quality_gates.len(), 2);
        assert_eq!(plan.quality_gates[0].tier, GateTier::Epic);
        assert_eq!(plan.stories.len(), 2);
        assert_eq!(plan.stories[0].story_type, Some(StoryType::Schema));
        assert_eq!(plan.stories[1].depends_on, vec![0]);
        assert_eq!(plan.stories[1].acceptance_criteria.len(), 2);
    }
}
