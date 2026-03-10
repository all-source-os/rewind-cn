use std::path::PathBuf;

use chrono::Utc;
use tracing::{info, warn};

use crate::application::commands::CompleteEpic;
use crate::domain::error::RewindError;
use crate::domain::events::{QualityGate, RewindEvent};
use crate::domain::ids::EpicId;
use crate::infrastructure::engine::RewindEngine;

/// Result of running a single quality gate.
#[derive(Debug, Clone)]
pub struct GateResult {
    pub command: String,
    pub passed: bool,
    pub output: String,
}

/// Runs epic-level quality gates after all tasks complete.
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

    /// Run all epic-level quality gates and emit events.
    ///
    /// Returns true if all gates passed (epic can be completed).
    pub async fn run_epic_gates<B: allframe::cqrs::EventStoreBackend<RewindEvent>>(
        &self,
        epic_id: &EpicId,
        gates: &[QualityGate],
        engine: &RewindEngine<B>,
    ) -> Result<bool, RewindError> {
        let mut all_passed = true;

        for gate in gates {
            let result = self.run_gate(gate).await;

            // Emit gate event
            let _ = engine
                .append_events(vec![RewindEvent::QualityGateRan {
                    epic_id: epic_id.clone(),
                    command: result.command.clone(),
                    passed: result.passed,
                    output: result.output.clone(),
                    ran_at: Utc::now(),
                }])
                .await;

            if result.passed {
                info!("Gate passed: {}", result.command);
            } else {
                warn!("Gate FAILED: {}", result.command);
                all_passed = false;
            }
        }

        if all_passed {
            engine
                .complete_epic(CompleteEpic {
                    epic_id: epic_id.clone(),
                })
                .await?;
            info!("All gates passed — epic {} completed", epic_id);
        }

        Ok(all_passed)
    }
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

        // Create an epic first
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
}
