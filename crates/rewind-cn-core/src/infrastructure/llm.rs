use rig::providers::anthropic;

use crate::domain::error::RewindError;

/// Configuration for the agent layer (planner, coder, evaluator).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_provider")]
    pub provider: String,

    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,

    #[serde(default)]
    pub planner: ModelConfig,

    #[serde(default)]
    pub coder: ModelConfig,

    #[serde(default)]
    pub evaluator: EvaluatorModelConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelConfig {
    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvaluatorModelConfig {
    #[serde(default = "default_evaluator_model")]
    pub model: String,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
}

fn default_provider() -> String {
    "anthropic".into()
}

fn default_api_key_env() -> String {
    "ANTHROPIC_API_KEY".into()
}

fn default_model() -> String {
    "claude-sonnet-4-5-20250514".into()
}

fn default_evaluator_model() -> String {
    "claude-haiku-4-5-20251001".into()
}

fn default_max_tokens() -> usize {
    16384
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            api_key_env: default_api_key_env(),
            planner: ModelConfig::default(),
            coder: ModelConfig::default(),
            evaluator: EvaluatorModelConfig::default(),
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            max_tokens: default_max_tokens(),
        }
    }
}

impl Default for EvaluatorModelConfig {
    fn default() -> Self {
        Self {
            model: default_evaluator_model(),
            max_tokens: default_max_tokens(),
        }
    }
}

/// Create an Anthropic client from the agent config.
///
/// Reads the API key from the environment variable specified in `config.api_key_env`.
pub fn create_anthropic_client(config: &AgentConfig) -> Result<anthropic::Client, RewindError> {
    let api_key = std::env::var(&config.api_key_env).map_err(|_| {
        RewindError::Config(format!(
            "Environment variable '{}' not set. Set it to your Anthropic API key.",
            config.api_key_env
        ))
    })?;

    anthropic::Client::new(&api_key)
        .map_err(|e| RewindError::Config(format!("Failed to create Anthropic client: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_agent_config() {
        let config = AgentConfig::default();
        assert_eq!(config.provider, "anthropic");
        assert_eq!(config.api_key_env, "ANTHROPIC_API_KEY");
        assert_eq!(config.planner.model, "claude-sonnet-4-5-20250514");
        assert_eq!(config.coder.model, "claude-sonnet-4-5-20250514");
        assert_eq!(config.evaluator.model, "claude-haiku-4-5-20251001");
        assert_eq!(config.coder.max_tokens, 16384);
    }

    #[test]
    fn agent_config_from_toml() {
        let toml_str = r#"
            provider = "anthropic"
            api_key_env = "MY_CUSTOM_KEY"

            [planner]
            model = "claude-opus-4-20250514"

            [coder]
            model = "claude-sonnet-4-5-20250514"
            max_tokens = 32768

            [evaluator]
            model = "claude-haiku-4-5-20251001"
        "#;

        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.api_key_env, "MY_CUSTOM_KEY");
        assert_eq!(config.planner.model, "claude-opus-4-20250514");
        assert_eq!(config.coder.max_tokens, 32768);
        assert_eq!(config.evaluator.model, "claude-haiku-4-5-20251001");
    }

    #[test]
    fn agent_config_partial_toml() {
        let toml_str = r#"
            [planner]
            model = "claude-opus-4-20250514"
        "#;

        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.provider, "anthropic");
        assert_eq!(config.planner.model, "claude-opus-4-20250514");
        // Defaults for unspecified fields
        assert_eq!(config.coder.model, "claude-sonnet-4-5-20250514");
        assert_eq!(config.evaluator.model, "claude-haiku-4-5-20251001");
    }

    #[test]
    fn create_client_missing_env_var() {
        let config = AgentConfig {
            api_key_env: "REWIND_TEST_NONEXISTENT_KEY_12345".into(),
            ..Default::default()
        };
        let result = create_anthropic_client(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("REWIND_TEST_NONEXISTENT_KEY_12345"));
    }
}
