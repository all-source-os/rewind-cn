use rig::completion::Prompt;
use rig::prelude::CompletionClient;
use rig::providers::{anthropic, openai};

use crate::domain::error::RewindError;

/// Supported LLM providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAI,
    /// Ollama (OpenAI-compatible API at localhost:11434).
    Ollama,
}

impl Provider {
    pub fn parse(s: &str) -> Result<Self, RewindError> {
        match s.to_lowercase().as_str() {
            "anthropic" | "claude" => Ok(Self::Anthropic),
            "openai" | "gpt" => Ok(Self::OpenAI),
            "ollama" => Ok(Self::Ollama),
            other => Err(RewindError::Config(format!(
                "Unknown provider '{other}'. Supported: anthropic, openai, ollama"
            ))),
        }
    }

    fn default_api_key_env(&self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAI => "OPENAI_API_KEY",
            Self::Ollama => "OLLAMA_API_KEY",
        }
    }

    fn default_base_url(&self) -> Option<&'static str> {
        match self {
            Self::Ollama => Some("http://localhost:11434/v1"),
            _ => None,
        }
    }
}

/// Runtime client wrapping a specific provider.
pub enum ProviderClient {
    Anthropic(anthropic::Client),
    OpenAI(openai::Client),
    /// Mock provider for testing. The closure receives (model, preamble, max_tokens, input)
    /// and returns the response string. Wrapped in Arc for Clone.
    #[cfg(test)]
    Mock(std::sync::Arc<dyn Fn(&str, &str, u64, &str) -> String + Send + Sync>),
}

impl ProviderClient {
    /// Send a simple prompt (no tools) — used by planner and evaluator.
    pub async fn prompt(
        &self,
        model: &str,
        preamble: &str,
        max_tokens: u64,
        input: &str,
    ) -> Result<String, RewindError> {
        match self {
            Self::Anthropic(c) => {
                let agent = c
                    .agent(model)
                    .preamble(preamble)
                    .max_tokens(max_tokens)
                    .build();
                agent
                    .prompt(input)
                    .await
                    .map_err(|e| RewindError::Config(format!("LLM call failed: {e}")))
            }
            Self::OpenAI(c) => {
                let agent = c
                    .agent(model)
                    .preamble(preamble)
                    .max_tokens(max_tokens)
                    .build();
                agent
                    .prompt(input)
                    .await
                    .map_err(|e| RewindError::Config(format!("LLM call failed: {e}")))
            }
            #[cfg(test)]
            Self::Mock(f) => Ok(f(model, preamble, max_tokens, input)),
        }
    }
}

/// Configuration for the agent layer (planner, coder, evaluator).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentConfig {
    /// Default provider for all roles (can be overridden per-role).
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Default API key env var (can be overridden per-role).
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,

    #[serde(default)]
    pub planner: ModelConfig,

    #[serde(default)]
    pub coder: ModelConfig,

    #[serde(default)]
    pub evaluator: EvaluatorModelConfig,

    /// Backend for the coder role: "api" (default, uses rig-core) or "claude-code" (shells out to claude CLI).
    #[serde(default = "default_coder_backend")]
    pub coder_backend: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelConfig {
    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Override provider for this role (e.g., "openai" for planner while coder uses "anthropic").
    #[serde(default)]
    pub provider: Option<String>,

    /// Override API key env var for this role.
    #[serde(default)]
    pub api_key_env: Option<String>,

    /// Custom base URL (e.g., "http://localhost:11434/v1" for Ollama).
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvaluatorModelConfig {
    #[serde(default = "default_evaluator_model")]
    pub model: String,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Override provider for this role.
    #[serde(default)]
    pub provider: Option<String>,

    /// Override API key env var for this role.
    #[serde(default)]
    pub api_key_env: Option<String>,

    /// Custom base URL (e.g., "http://localhost:11434/v1" for Ollama).
    #[serde(default)]
    pub base_url: Option<String>,
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

fn default_coder_backend() -> String {
    "api".into()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            api_key_env: default_api_key_env(),
            planner: ModelConfig::default(),
            coder: ModelConfig::default(),
            evaluator: EvaluatorModelConfig::default(),
            coder_backend: default_coder_backend(),
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            model: default_model(),
            max_tokens: default_max_tokens(),
            provider: None,
            api_key_env: None,
            base_url: None,
        }
    }
}

impl Default for EvaluatorModelConfig {
    fn default() -> Self {
        Self {
            model: default_evaluator_model(),
            max_tokens: default_max_tokens(),
            provider: None,
            api_key_env: None,
            base_url: None,
        }
    }
}

/// Resolved provider configuration for a role.
struct ResolvedConfig {
    provider: Provider,
    api_key_env: String,
    base_url: Option<String>,
}

/// Resolve the effective provider, API key env var, and base URL for a role,
/// falling back to the global config defaults.
fn resolve_provider_config(
    global_provider: &str,
    global_api_key_env: &str,
    role_provider: Option<&str>,
    role_api_key_env: Option<&str>,
    role_base_url: Option<&str>,
) -> ResolvedConfig {
    let provider_str = role_provider.unwrap_or(global_provider);
    let provider = Provider::parse(provider_str).unwrap_or(Provider::Anthropic);

    let api_key_env = role_api_key_env.map(|s| s.to_string()).unwrap_or_else(|| {
        if role_provider.is_some() {
            provider.default_api_key_env().to_string()
        } else {
            global_api_key_env.to_string()
        }
    });

    let base_url = role_base_url
        .map(|s| s.to_string())
        .or_else(|| provider.default_base_url().map(|s| s.to_string()));

    ResolvedConfig {
        provider,
        api_key_env,
        base_url,
    }
}

/// Create a ProviderClient from resolved config.
fn create_client(resolved: &ResolvedConfig) -> Result<ProviderClient, RewindError> {
    match resolved.provider {
        Provider::Ollama => {
            let base_url = resolved
                .base_url
                .as_deref()
                .unwrap_or("http://localhost:11434/v1");
            let client = openai::Client::builder()
                .api_key("ollama")
                .base_url(base_url)
                .build()
                .map_err(|e| RewindError::Config(format!("Failed to create Ollama client: {e}")))?;
            Ok(ProviderClient::OpenAI(client))
        }
        _ => {
            let api_key = std::env::var(&resolved.api_key_env).map_err(|_| {
                RewindError::Config(format!(
                    "Environment variable '{}' not set. Set it to your {:?} API key.",
                    resolved.api_key_env, resolved.provider
                ))
            })?;

            match resolved.provider {
                Provider::Anthropic => {
                    let client = anthropic::Client::new(&api_key).map_err(|e| {
                        RewindError::Config(format!("Failed to create Anthropic client: {e}"))
                    })?;
                    Ok(ProviderClient::Anthropic(client))
                }
                Provider::OpenAI => {
                    let mut builder = openai::Client::builder().api_key(&api_key);
                    if let Some(ref base_url) = resolved.base_url {
                        builder = builder.base_url(base_url);
                    }
                    let client = builder.build().map_err(|e| {
                        RewindError::Config(format!("Failed to create OpenAI client: {e}"))
                    })?;
                    Ok(ProviderClient::OpenAI(client))
                }
                Provider::Ollama => unreachable!(),
            }
        }
    }
}

/// Create a ProviderClient for the planner role.
pub fn create_planner_client(config: &AgentConfig) -> Result<ProviderClient, RewindError> {
    let resolved = resolve_provider_config(
        &config.provider,
        &config.api_key_env,
        config.planner.provider.as_deref(),
        config.planner.api_key_env.as_deref(),
        config.planner.base_url.as_deref(),
    );
    create_client(&resolved)
}

/// Create a ProviderClient for the coder role.
pub fn create_coder_client(config: &AgentConfig) -> Result<ProviderClient, RewindError> {
    let resolved = resolve_provider_config(
        &config.provider,
        &config.api_key_env,
        config.coder.provider.as_deref(),
        config.coder.api_key_env.as_deref(),
        config.coder.base_url.as_deref(),
    );
    create_client(&resolved)
}

/// Create a ProviderClient for the evaluator role.
pub fn create_evaluator_client(config: &AgentConfig) -> Result<ProviderClient, RewindError> {
    let resolved = resolve_provider_config(
        &config.provider,
        &config.api_key_env,
        config.evaluator.provider.as_deref(),
        config.evaluator.api_key_env.as_deref(),
        config.evaluator.base_url.as_deref(),
    );
    create_client(&resolved)
}

/// Create an Anthropic client from the agent config (backward-compatible convenience).
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

    #[test]
    fn per_role_provider_override_toml() {
        let toml_str = r#"
            provider = "anthropic"
            api_key_env = "ANTHROPIC_API_KEY"

            [planner]
            model = "gpt-4o"
            provider = "openai"
            api_key_env = "OPENAI_API_KEY"

            [coder]
            model = "claude-sonnet-4-5-20250514"

            [evaluator]
            model = "claude-haiku-4-5-20251001"
        "#;

        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.planner.provider, Some("openai".into()));
        assert_eq!(config.planner.api_key_env, Some("OPENAI_API_KEY".into()));
        assert_eq!(config.coder.provider, None);
    }

    #[test]
    fn resolve_provider_uses_role_override() {
        let resolved =
            resolve_provider_config("anthropic", "ANTHROPIC_API_KEY", Some("openai"), None, None);
        assert_eq!(resolved.provider, Provider::OpenAI);
        assert_eq!(resolved.api_key_env, "OPENAI_API_KEY");
    }

    #[test]
    fn resolve_provider_falls_back_to_global() {
        let resolved = resolve_provider_config("anthropic", "MY_KEY", None, None, None);
        assert_eq!(resolved.provider, Provider::Anthropic);
        assert_eq!(resolved.api_key_env, "MY_KEY");
    }

    #[test]
    fn resolve_provider_role_key_overrides_all() {
        let resolved = resolve_provider_config(
            "anthropic",
            "ANTHROPIC_API_KEY",
            Some("openai"),
            Some("CUSTOM_KEY"),
            None,
        );
        assert_eq!(resolved.provider, Provider::OpenAI);
        assert_eq!(resolved.api_key_env, "CUSTOM_KEY");
    }

    #[test]
    fn resolve_ollama_provider() {
        let resolved = resolve_provider_config("ollama", "unused", None, None, None);
        assert_eq!(resolved.provider, Provider::Ollama);
        assert_eq!(
            resolved.base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
    }

    #[test]
    fn ollama_config_from_toml() {
        let toml_str = r#"
            provider = "anthropic"
            coder_backend = "claude-code"

            [evaluator]
            provider = "ollama"
            model = "llama3"

            [planner]
            provider = "ollama"
            model = "llama3"
        "#;

        let config: AgentConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.evaluator.provider, Some("ollama".into()));
        assert_eq!(config.evaluator.model, "llama3");
        assert_eq!(config.planner.provider, Some("ollama".into()));
    }

    #[test]
    fn provider_from_str_variants() {
        assert_eq!(Provider::parse("anthropic").unwrap(), Provider::Anthropic);
        assert_eq!(Provider::parse("claude").unwrap(), Provider::Anthropic);
        assert_eq!(Provider::parse("openai").unwrap(), Provider::OpenAI);
        assert_eq!(Provider::parse("gpt").unwrap(), Provider::OpenAI);
        assert_eq!(Provider::parse("OpenAI").unwrap(), Provider::OpenAI);
        assert!(Provider::parse("unknown").is_err());
    }
}
