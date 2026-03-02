use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct RalphConfig {
    #[serde(default = "default_project_name")]
    pub project_name: String,

    #[serde(default)]
    pub llm: LlmConfig,

    #[serde(default)]
    pub agents: AgentsConfig,
}

fn default_project_name() -> String {
    "ralph-project".into()
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key_env: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentsConfig {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_max_concurrent() -> usize {
    3
}

fn default_timeout_secs() -> u64 {
    300
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            max_concurrent: default_max_concurrent(),
            timeout_secs: default_timeout_secs(),
        }
    }
}

impl Default for RalphConfig {
    fn default() -> Self {
        Self {
            project_name: default_project_name(),
            llm: LlmConfig::default(),
            agents: AgentsConfig::default(),
        }
    }
}

impl RalphConfig {
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
