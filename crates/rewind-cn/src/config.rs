use rewind_cn_core::infrastructure::llm::AgentConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TracingDetail {
    Minimal,
    Normal,
    Verbose,
}

impl Default for TracingDetail {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GateConfig {
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GatesConfig {
    #[serde(default)]
    pub epic: GateConfig,

    #[serde(default)]
    pub story: GateConfig,
}

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

    #[serde(default)]
    pub telemetry: TelemetryConfig,

    /// Path to a custom .tera prompt template file.
    #[serde(default)]
    pub prompt_template: Option<PathBuf>,

    /// Maximum number of agent iterations per task.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Level of detail for subagent tracing output.
    #[serde(default)]
    pub subagent_tracing_detail: TracingDetail,

    /// Gate commands that must pass before completion.
    #[serde(default)]
    pub gates: GatesConfig,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Enable anonymous telemetry (default: false)
    #[serde(default)]
    pub enabled: bool,

    /// PostHog project API key
    #[serde(default)]
    pub posthog_key: Option<String>,

    /// PostHog host (default: https://us.i.posthog.com)
    #[serde(default = "default_posthog_host")]
    pub posthog_host: String,
}

fn default_posthog_host() -> String {
    "https://us.i.posthog.com".into()
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            posthog_key: None,
            posthog_host: default_posthog_host(),
        }
    }
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

fn default_max_iterations() -> u32 {
    10
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
            telemetry: TelemetryConfig::default(),
            prompt_template: None,
            max_iterations: default_max_iterations(),
            subagent_tracing_detail: TracingDetail::default(),
            gates: GatesConfig::default(),
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
        assert!(config.agent.is_none());
        assert_eq!(config.llm.provider, Some("anthropic".into()));
    }

    #[test]
    fn default_config_has_no_agent() {
        let config = RewindConfig::default();
        assert!(config.agent.is_none());
        assert_eq!(config.execution.max_retries, 2);
    }

    #[test]
    fn default_config_has_telemetry_disabled() {
        let config = RewindConfig::default();
        assert!(!config.telemetry.enabled);
        assert!(config.telemetry.posthog_key.is_none());
        assert_eq!(config.telemetry.posthog_host, "https://us.i.posthog.com");
    }

    #[test]
    fn parse_config_with_telemetry() {
        let toml_str = r#"
            project_name = "test"

            [telemetry]
            enabled = true
            posthog_key = "phc_test123"
            posthog_host = "https://selfhosted.example.com"
        "#;

        let config: RewindConfig = toml::from_str(toml_str).unwrap();
        assert!(config.telemetry.enabled);
        assert_eq!(config.telemetry.posthog_key, Some("phc_test123".into()));
        assert_eq!(
            config.telemetry.posthog_host,
            "https://selfhosted.example.com"
        );
    }

    #[test]
    fn parse_config_with_all_new_fields() {
        let toml_str = r#"
            project_name = "full-config"
            prompt_template = "prompts/custom.tera"
            max_iterations = 25
            subagent_tracing_detail = "verbose"

            [gates.epic]
            commands = ["cargo test", "cargo clippy"]

            [gates.story]
            commands = ["cargo check"]
        "#;

        let config: RewindConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.prompt_template,
            Some(PathBuf::from("prompts/custom.tera"))
        );
        assert_eq!(config.max_iterations, 25);
        assert_eq!(config.subagent_tracing_detail, TracingDetail::Verbose);
        assert_eq!(config.gates.epic.commands, vec!["cargo test", "cargo clippy"]);
        assert_eq!(config.gates.story.commands, vec!["cargo check"]);
    }

    #[test]
    fn execution_config_defaults_are_correct() {
        let config = ExecutionConfig::default();
        assert_eq!(config.max_concurrent, 3);
        assert_eq!(config.timeout_secs, 300);
        assert_eq!(config.max_retries, 2);
    }

    #[test]
    fn load_missing_file_returns_error() {
        let result = RewindConfig::load(Path::new("/nonexistent/rewind.toml"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read config"));
    }

    #[test]
    fn load_invalid_toml_returns_parse_error() {
        let path = std::env::temp_dir().join("rewind-test-bad.toml");
        std::fs::write(&path, "this is not = [valid toml").unwrap();
        let result = RewindConfig::load(&path);
        std::fs::remove_file(&path).ok();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse config"));
    }

    #[test]
    fn parse_config_with_no_new_fields_applies_defaults() {
        let toml_str = r#"
            project_name = "minimal"
        "#;

        let config: RewindConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.prompt_template, None);
        assert_eq!(config.max_iterations, 10);
        assert_eq!(config.subagent_tracing_detail, TracingDetail::Normal);
        assert!(config.gates.epic.commands.is_empty());
        assert!(config.gates.story.commands.is_empty());
    }
}
