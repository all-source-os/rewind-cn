use std::path::PathBuf;

use chrono::Utc;
use serde::Deserialize;
use tracing::{info, warn};

use crate::application::commands::CompleteEpic;
use crate::domain::error::RewindError;
use crate::domain::events::{GateTier, QualityGate, QualityGateLevel, RewindEvent};
use crate::domain::ids::EpicId;
use crate::infrastructure::engine::RewindEngine;

/// Gate configuration from rewind.toml `[gates]` section.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GateConfig {
    #[serde(default)]
    pub epic: GateTierConfig,
    #[serde(default)]
    pub story: GateTierConfig,
}

/// Commands for a single gate tier.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GateTierConfig {
    #[serde(default)]
    pub commands: Vec<String>,
}

impl GateConfig {
    /// Convert config into `QualityGate` structs for the given level.
    pub fn gates_for_level(&self, level: &QualityGateLevel) -> Vec<QualityGate> {
        let tier_config = match level {
            GateTier::Epic => &self.epic,
            GateTier::Story => &self.story,
        };
        tier_config
            .commands
            .iter()
            .map(|cmd| QualityGate {
                command: cmd.clone(),
                tier: level.clone(),
            })
            .collect()
    }
}

/// Result of running a single quality gate.
#[derive(Debug, Clone)]
pub struct GateResult {
    pub command: String,
    pub passed: bool,
    pub output: String,
}

/// Runs quality gates at both epic and story tiers.
pub struct QualityGateRunner {
    work_dir: PathBuf,
    timeout_secs: u64,
}

impl QualityGateRunner {
    pub fn new(work_dir: PathBuf, timeout_secs: u64) -> Self {
        Self {
            work_dir,
            timeout_secs,
        }
    }

    /// Run a single quality gate command and return the result.
    #[hotpath::measure]
    pub async fn run_gate(&self, gate: &QualityGate) -> GateResult {
        info!("Running quality gate: {}", gate.command);

        let child = tokio::process::Command::new("sh")
            .args(["-c", &gate.command])
            .current_dir(&self.work_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();

        let child = match child {
            Ok(c) => c,
            Err(e) => {
                return GateResult {
                    command: gate.command.clone(),
                    passed: false,
                    output: format!("Failed to spawn: {e}"),
                };
            }
        };

        let timeout = std::time::Duration::from_secs(self.timeout_secs);
        let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                return GateResult {
                    command: gate.command.clone(),
                    passed: false,
                    output: format!("Command error: {e}"),
                };
            }
            Err(_) => {
                return GateResult {
                    command: gate.command.clone(),
                    passed: false,
                    output: format!("Timed out after {}s", self.timeout_secs),
                };
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");

        GateResult {
            command: gate.command.clone(),
            passed: output.status.success(),
            output: combined,
        }
    }

    /// Run only the gates matching the specified level and emit events.
    ///
    /// Filters the provided gates by tier, then executes matching ones.
    /// Returns true if all matching gates passed.
    #[hotpath::measure]
    pub async fn run_gates<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
        &self,
        epic_id: &EpicId,
        gates: &[QualityGate],
        level: &QualityGateLevel,
        engine: &RewindEngine<B>,
    ) -> Result<bool, RewindError> {
        let filtered: Vec<&QualityGate> = gates.iter().filter(|g| &g.tier == level).collect();

        if filtered.is_empty() {
            info!("No {:?}-level gates to run", level);
            return Ok(true);
        }

        let mut all_passed = true;

        for gate in &filtered {
            let result = self.run_gate(gate).await;

            if let Err(e) = engine
                .append_events(vec![RewindEvent::QualityGateRan {
                    epic_id: epic_id.clone(),
                    command: result.command.clone(),
                    passed: result.passed,
                    output: result.output.clone(),
                    ran_at: Utc::now(),
                }])
                .await
            {
                warn!("Failed to append QualityGateRan event: {e}");
            }

            if result.passed {
                info!("Gate passed: {}", result.command);
            } else {
                warn!("Gate FAILED: {}", result.command);
                all_passed = false;
            }
        }

        Ok(all_passed)
    }

    /// Run epic-level quality gates and auto-complete epic if all pass.
    ///
    /// This is the entry point for epic completion — runs only Epic-tier gates.
    #[hotpath::measure]
    pub async fn run_epic_gates<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
        &self,
        epic_id: &EpicId,
        gates: &[QualityGate],
        engine: &RewindEngine<B>,
    ) -> Result<bool, RewindError> {
        let all_passed = self
            .run_gates(epic_id, gates, &QualityGateLevel::Epic, engine)
            .await?;

        if all_passed {
            engine
                .complete_epic(CompleteEpic {
                    epic_id: epic_id.clone(),
                })
                .await?;
            info!("All epic gates passed — epic {} completed", epic_id);
        }

        Ok(all_passed)
    }

    /// Run story-level quality gates (e.g. cargo check after each story).
    ///
    /// Does NOT complete the epic — only validates the story change.
    #[hotpath::measure]
    pub async fn run_story_gates<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
        &self,
        epic_id: &EpicId,
        gates: &[QualityGate],
        engine: &RewindEngine<B>,
    ) -> Result<bool, RewindError> {
        self.run_gates(epic_id, gates, &QualityGateLevel::Story, engine)
            .await
    }
}

/// Filter gates by tier level (useful for callers that need the list without running).
pub fn filter_gates_by_level(gates: &[QualityGate], level: &QualityGateLevel) -> Vec<QualityGate> {
    gates
        .iter()
        .filter(|g| &g.tier == level)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::events::GateTier;

    #[tokio::test]
    async fn run_gate_success() {
        let runner = QualityGateRunner::new(std::env::temp_dir(), 10);
        let gate = QualityGate {
            command: "echo ok".into(),
            tier: GateTier::Epic,
        };

        let result = runner.run_gate(&gate).await;
        assert!(result.passed);
        assert!(result.output.contains("ok"));
    }

    #[tokio::test]
    async fn run_gate_failure() {
        let runner = QualityGateRunner::new(std::env::temp_dir(), 10);
        let gate = QualityGate {
            command: "exit 1".into(),
            tier: GateTier::Epic,
        };

        let result = runner.run_gate(&gate).await;
        assert!(!result.passed);
    }

    #[tokio::test]
    async fn run_epic_gates_all_pass() {
        let runner = QualityGateRunner::new(std::env::temp_dir(), 10);
        let engine = RewindEngine::in_memory().await;

        let epic_events = engine
            .create_epic(crate::application::commands::CreateEpic {
                title: "Test Epic".into(),
                description: "".into(),
                quality_gates: vec![],
            })
            .await
            .unwrap();

        let epic_id = match &epic_events[0] {
            RewindEvent::EpicCreated { epic_id, .. } => epic_id.clone(),
            _ => panic!("Expected EpicCreated"),
        };

        let gates = vec![
            QualityGate {
                command: "echo gate1".into(),
                tier: GateTier::Epic,
            },
            QualityGate {
                command: "echo gate2".into(),
                tier: GateTier::Epic,
            },
        ];

        let passed = runner
            .run_epic_gates(&epic_id, &gates, &engine)
            .await
            .unwrap();
        assert!(passed);
    }

    #[tokio::test]
    async fn run_epic_gates_one_fails() {
        let runner = QualityGateRunner::new(std::env::temp_dir(), 10);
        let engine = RewindEngine::in_memory().await;

        let epic_events = engine
            .create_epic(crate::application::commands::CreateEpic {
                title: "Test Epic".into(),
                description: "".into(),
                quality_gates: vec![],
            })
            .await
            .unwrap();

        let epic_id = match &epic_events[0] {
            RewindEvent::EpicCreated { epic_id, .. } => epic_id.clone(),
            _ => panic!("Expected EpicCreated"),
        };

        let gates = vec![
            QualityGate {
                command: "echo ok".into(),
                tier: GateTier::Epic,
            },
            QualityGate {
                command: "exit 1".into(),
                tier: GateTier::Epic,
            },
        ];

        let passed = runner
            .run_epic_gates(&epic_id, &gates, &engine)
            .await
            .unwrap();
        assert!(!passed);
    }

    #[tokio::test]
    async fn run_gates_filters_by_level() {
        let runner = QualityGateRunner::new(std::env::temp_dir(), 10);
        let engine = RewindEngine::in_memory().await;

        let epic_events = engine
            .create_epic(crate::application::commands::CreateEpic {
                title: "Filter Test".into(),
                description: "".into(),
                quality_gates: vec![],
            })
            .await
            .unwrap();

        let epic_id = match &epic_events[0] {
            RewindEvent::EpicCreated { epic_id, .. } => epic_id.clone(),
            _ => panic!("Expected EpicCreated"),
        };

        // Mix of story and epic gates
        let gates = vec![
            QualityGate {
                command: "echo story-check".into(),
                tier: GateTier::Story,
            },
            QualityGate {
                command: "echo epic-test".into(),
                tier: GateTier::Epic,
            },
            QualityGate {
                command: "echo epic-clippy".into(),
                tier: GateTier::Epic,
            },
        ];

        // Story-level should run only the story gate
        let story_passed = runner
            .run_story_gates(&epic_id, &gates, &engine)
            .await
            .unwrap();
        assert!(story_passed);

        // Epic-level should run only the epic gates
        let epic_passed = runner
            .run_gates(&epic_id, &gates, &QualityGateLevel::Epic, &engine)
            .await
            .unwrap();
        assert!(epic_passed);

        // Verify events: 1 story gate + 2 epic gates = 3 QualityGateRan events
        let events = engine.event_store.get_all_events().await.unwrap();
        let gate_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, RewindEvent::QualityGateRan { .. }))
            .collect();
        assert_eq!(gate_events.len(), 3);

        // First event should be the story gate
        match &gate_events[0] {
            RewindEvent::QualityGateRan { command, .. } => {
                assert_eq!(command, "echo story-check");
            }
            _ => panic!("Expected QualityGateRan"),
        }

        // Second and third should be epic gates
        match &gate_events[1] {
            RewindEvent::QualityGateRan { command, .. } => {
                assert_eq!(command, "echo epic-test");
            }
            _ => panic!("Expected QualityGateRan"),
        }
    }

    #[test]
    fn filter_gates_by_level_selects_correct_tier() {
        let gates = vec![
            QualityGate {
                command: "cargo check".into(),
                tier: GateTier::Story,
            },
            QualityGate {
                command: "cargo test".into(),
                tier: GateTier::Epic,
            },
            QualityGate {
                command: "cargo clippy".into(),
                tier: GateTier::Epic,
            },
            QualityGate {
                command: "cargo fmt --check".into(),
                tier: GateTier::Epic,
            },
        ];

        let story_gates = filter_gates_by_level(&gates, &QualityGateLevel::Story);
        assert_eq!(story_gates.len(), 1);
        assert_eq!(story_gates[0].command, "cargo check");

        let epic_gates = filter_gates_by_level(&gates, &QualityGateLevel::Epic);
        assert_eq!(epic_gates.len(), 3);
        assert_eq!(epic_gates[0].command, "cargo test");
        assert_eq!(epic_gates[1].command, "cargo clippy");
        assert_eq!(epic_gates[2].command, "cargo fmt --check");
    }

    #[test]
    fn gate_config_from_toml() {
        let toml_str = r#"
            [epic]
            commands = ["cargo test", "cargo clippy -- -D warnings", "cargo fmt --check"]

            [story]
            commands = ["cargo check"]
        "#;

        let config: GateConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.epic.commands.len(), 3);
        assert_eq!(config.story.commands.len(), 1);
        assert_eq!(config.story.commands[0], "cargo check");

        let epic_gates = config.gates_for_level(&QualityGateLevel::Epic);
        assert_eq!(epic_gates.len(), 3);
        assert_eq!(epic_gates[0].tier, GateTier::Epic);

        let story_gates = config.gates_for_level(&QualityGateLevel::Story);
        assert_eq!(story_gates.len(), 1);
        assert_eq!(story_gates[0].tier, GateTier::Story);
    }

    #[test]
    fn gate_config_defaults_empty() {
        let config: GateConfig = toml::from_str("").unwrap();
        assert!(config.epic.commands.is_empty());
        assert!(config.story.commands.is_empty());
    }
}
