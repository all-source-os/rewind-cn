use rewind_cn_core::infrastructure::llm::AgentConfig;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct RewindConfig {
    #[serde(default = "default_project_name")]
    pub project_name: String,

    /// Legacy LLM config — kept for backwards compatibility.
    /// Prefer `[agent]` section for new configs.
    #[serde(default)]
    pub llm: LlmConfig,

    /// Agent configuration (planner, coder, evaluator models).
    /// When present, enables LLM-powered plan decomposition and agent execution.
    #[serde(default)]
    pub agent: Option<AgentConfig>,

    #[serde(default)]
    pub execution: ExecutionConfig,
}

fn default_project_name() -> String {
    "rewind-project".into()
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key_env: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecutionConfig {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_max_concurrent() -> usize {
    3
}

fn default_timeout_secs() -> u64 {
    300
}

fn default_max_retries() -> u32 {
    2
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            max_concurrent: default_max_concurrent(),
            timeout_secs: default_timeout_secs(),
            max_retries: default_max_retries(),
        }
    }
}

impl Default for RewindConfig {
    fn default() -> Self {
        Self {
            project_name: default_project_name(),
            llm: LlmConfig::default(),
            agent: None,
            execution: ExecutionConfig::default(),
        }
    }
}

impl RewindConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read config: {e}"))?;
        toml::from_str(&content).map_err(|e| format!("Failed to parse config: {e}"))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let content =
            toml::to_string_pretty(self).map_err(|e| format!("Failed to serialize config: {e}"))?;
        std::fs::write(path, content).map_err(|e| format!("Failed to write config: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_with_agent_section() {
        let toml_str = r#"
            project_name = "test-project"

            [agent]
            provider = "anthropic"
            api_key_env = "ANTHROPIC_API_KEY"

            [agent.planner]
            model = "claude-opus-4-20250514"

            [agent.coder]
            model = "claude-sonnet-4-5-20250514"
            max_tokens = 32768

            [agent.evaluator]
            model = "claude-haiku-4-5-20251001"

            [execution]
            max_concurrent = 5
            timeout_secs = 600
            max_retries = 3
        "#;

        let config: RewindConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.project_name, "test-project");
        assert!(config.agent.is_some());

        let agent = config.agent.unwrap();
        assert_eq!(agent.planner.model, "claude-opus-4-20250514");
        assert_eq!(agent.coder.max_tokens, 32768);
        assert_eq!(agent.evaluator.model, "claude-haiku-4-5-20251001");

        assert_eq!(config.execution.max_concurrent, 5);
        assert_eq!(config.execution.max_retries, 3);
    }

    #[test]
    fn parse_config_without_agent_section() {
        let toml_str = r#"
            project_name = "legacy-project"

            [llm]
            provider = "anthropic"
        "#;

        let config: RewindConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.agent.is_some());
        assert_eq!(config.llm.provider, Some("anthropic".into()));
    }

    #[test]
    fn default_config_has_no_agent() {
        let config = RewindConfig::default();
        assert!(!config.agent.is_some());
        assert_eq!(config.execution.max_retries, 2);
    }
}
