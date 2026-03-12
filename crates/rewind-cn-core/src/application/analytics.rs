use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::domain::events::RewindEvent;
use crate::domain::ids::{EpicId, SessionId, TaskId};

/// Per-iteration log entry.
#[derive(Debug, Clone, Serialize)]
pub struct IterationLog {
    pub session_id: SessionId,
    pub task_id: TaskId,
    pub iteration_number: u32,
    pub agent_output: String,
    pub duration_ms: u64,
}

/// Per-task execution metrics.
#[derive(Debug, Clone, Serialize)]
pub struct TaskMetrics {
    pub task_id: TaskId,
    pub title: String,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_secs: Option<f64>,
    pub outcome: TaskOutcome,
    pub tool_call_count: usize,
    pub criteria_total: usize,
    pub criteria_checked: usize,
    pub failure_reason: Option<String>,
    pub epic_id: Option<EpicId>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum TaskOutcome {
    Pending,
    InProgress,
    Passed,
    Failed,
}

/// Per-epic aggregate metrics.
#[derive(Debug, Clone, Serialize)]
pub struct EpicMetrics {
    pub epic_id: EpicId,
    pub title: String,
    pub total_tasks: usize,
    pub completed: usize,
    pub failed: usize,
    pub gates_total: usize,
    pub gates_passed: usize,
    pub is_completed: bool,
}

/// Per-session metrics.
#[derive(Debug, Clone, Serialize)]
pub struct SessionMetrics {
    pub session_id: SessionId,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_secs: Option<f64>,
    pub tasks_executed: usize,
}

/// Tool usage statistics.
#[derive(Debug, Clone, Serialize)]
pub struct ToolUsage {
    pub tool_name: String,
    pub call_count: usize,
}

/// Projection that aggregates analytics metrics from events.
#[derive(Debug, Default)]
pub struct AnalyticsProjection {
    pub tasks: HashMap<String, TaskMetrics>,
    pub epics: HashMap<String, EpicMetrics>,
    pub sessions: HashMap<String, SessionMetrics>,
    pub tool_counts: HashMap<String, usize>,
    /// Map task_id → epic_id for cross-referencing.
    task_to_epic: HashMap<String, String>,
    /// Iteration logs keyed by session_id.
    pub iterations: HashMap<String, Vec<IterationLog>>,
}

impl AnalyticsProjection {
    pub fn apply_event(&mut self, event: &RewindEvent) {
        match event {
            RewindEvent::TaskCreated {
                task_id,
                title,
                epic_id,
                acceptance_criteria,
                ..
            } => {
                if let Some(eid) = epic_id {
                    self.task_to_epic
                        .insert(task_id.to_string(), eid.to_string());
                }
                self.tasks.insert(
                    task_id.to_string(),
                    TaskMetrics {
                        task_id: task_id.clone(),
                        title: title.clone(),
                        started_at: None,
                        ended_at: None,
                        duration_secs: None,
                        outcome: TaskOutcome::Pending,
                        tool_call_count: 0,
                        criteria_total: acceptance_criteria.len(),
                        criteria_checked: 0,
                        failure_reason: None,
                        epic_id: epic_id.clone(),
                    },
                );
                if let Some(eid) = epic_id {
                    if let Some(epic) = self.epics.get_mut(eid.as_ref()) {
                        epic.total_tasks += 1;
                    }
                }
            }

            RewindEvent::TaskStarted {
                task_id,
                started_at,
            } => {
                if let Some(t) = self.tasks.get_mut(task_id.as_ref()) {
                    t.started_at = Some(*started_at);
                    t.outcome = TaskOutcome::InProgress;
                }
            }

            RewindEvent::TaskCompleted {
                task_id,
                completed_at,
            } => {
                if let Some(t) = self.tasks.get_mut(task_id.as_ref()) {
                    t.ended_at = Some(*completed_at);
                    t.outcome = TaskOutcome::Passed;
                    if let Some(start) = t.started_at {
                        t.duration_secs =
                            Some((*completed_at - start).num_milliseconds() as f64 / 1000.0);
                    }
                }
                if let Some(eid) = self.task_to_epic.get(task_id.as_ref()) {
                    if let Some(epic) = self.epics.get_mut(eid) {
                        epic.completed += 1;
                    }
                }
            }

            RewindEvent::TaskFailed {
                task_id,
                reason,
                failed_at,
            } => {
                if let Some(t) = self.tasks.get_mut(task_id.as_ref()) {
                    t.ended_at = Some(*failed_at);
                    t.outcome = TaskOutcome::Failed;
                    t.failure_reason = Some(reason.clone());
                    if let Some(start) = t.started_at {
                        t.duration_secs =
                            Some((*failed_at - start).num_milliseconds() as f64 / 1000.0);
                    }
                }
                if let Some(eid) = self.task_to_epic.get(task_id.as_ref()) {
                    if let Some(epic) = self.epics.get_mut(eid) {
                        epic.failed += 1;
                    }
                }
            }

            RewindEvent::EpicCreated {
                epic_id,
                title,
                quality_gates,
                ..
            } => {
                self.epics.insert(
                    epic_id.to_string(),
                    EpicMetrics {
                        epic_id: epic_id.clone(),
                        title: title.clone(),
                        total_tasks: 0,
                        completed: 0,
                        failed: 0,
                        gates_total: quality_gates.len(),
                        gates_passed: 0,
                        is_completed: false,
                    },
                );
            }

            RewindEvent::EpicCompleted { epic_id, .. } => {
                if let Some(epic) = self.epics.get_mut(epic_id.as_ref()) {
                    epic.is_completed = true;
                }
            }

            RewindEvent::QualityGateRan {
                epic_id, passed, ..
            } => {
                if *passed {
                    if let Some(epic) = self.epics.get_mut(epic_id.as_ref()) {
                        epic.gates_passed += 1;
                    }
                }
            }

            RewindEvent::SessionStarted {
                session_id,
                started_at,
            } => {
                self.sessions.insert(
                    session_id.to_string(),
                    SessionMetrics {
                        session_id: session_id.clone(),
                        started_at: *started_at,
                        ended_at: None,
                        duration_secs: None,
                        tasks_executed: 0,
                    },
                );
            }

            RewindEvent::SessionEnded {
                session_id,
                ended_at,
            } => {
                if let Some(s) = self.sessions.get_mut(session_id.as_ref()) {
                    s.ended_at = Some(*ended_at);
                    s.duration_secs =
                        Some((*ended_at - s.started_at).num_milliseconds() as f64 / 1000.0);
                }
            }

            RewindEvent::AgentToolCall {
                task_id, tool_name, ..
            } => {
                if let Some(t) = self.tasks.get_mut(task_id.as_ref()) {
                    t.tool_call_count += 1;
                }
                *self.tool_counts.entry(tool_name.clone()).or_insert(0) += 1;
            }

            RewindEvent::CriterionChecked { task_id, .. } => {
                if let Some(t) = self.tasks.get_mut(task_id.as_ref()) {
                    t.criteria_checked += 1;
                }
            }

            RewindEvent::IterationLogged {
                session_id,
                task_id,
                iteration_number,
                agent_output,
                duration_ms,
            } => {
                self.iterations
                    .entry(session_id.to_string())
                    .or_default()
                    .push(IterationLog {
                        session_id: session_id.clone(),
                        task_id: task_id.clone(),
                        iteration_number: *iteration_number,
                        agent_output: agent_output.clone(),
                        duration_ms: *duration_ms,
                    });
            }

            _ => {}
        }
    }

    /// Get task summary: per-task metrics sorted by start time.
    pub fn task_summary(&self, epic_filter: Option<&str>) -> Vec<&TaskMetrics> {
        let mut tasks: Vec<&TaskMetrics> = self
            .tasks
            .values()
            .filter(|t| match epic_filter {
                Some(eid) => t.epic_id.as_ref().is_some_and(|e| e.as_ref() == eid),
                None => true,
            })
            .collect();
        tasks.sort_by_key(|t| t.started_at);
        tasks
    }

    /// Get epic summary.
    pub fn epic_summary(&self) -> Vec<&EpicMetrics> {
        self.epics.values().collect()
    }

    /// Get tool usage ranked by count (descending).
    pub fn tool_usage(&self) -> Vec<ToolUsage> {
        let mut usage: Vec<ToolUsage> = self
            .tool_counts
            .iter()
            .map(|(name, count)| ToolUsage {
                tool_name: name.clone(),
                call_count: *count,
            })
            .collect();
        usage.sort_by(|a, b| b.call_count.cmp(&a.call_count));
        usage
    }

    /// Get iteration logs for a session, sorted by iteration number.
    pub fn iteration_history(&self, session_id: &str) -> Vec<&IterationLog> {
        match self.iterations.get(session_id) {
            Some(logs) => {
                let mut sorted: Vec<&IterationLog> = logs.iter().collect();
                sorted.sort_by_key(|l| l.iteration_number);
                sorted
            }
            None => vec![],
        }
    }

    /// Get session history sorted by start time.
    pub fn session_history(&self) -> Vec<&SessionMetrics> {
        let mut sessions: Vec<&SessionMetrics> = self.sessions.values().collect();
        sessions.sort_by_key(|s| s.started_at);
        sessions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::events::AcceptanceCriterion;
    use crate::domain::ids::{EpicId, SessionId, TaskId};
    use chrono::{Duration, Utc};

    #[test]
    fn full_lifecycle_produces_correct_metrics() {
        let mut proj = AnalyticsProjection::default();
        let now = Utc::now();
        let epic_id = EpicId::new("e-1");
        let task_id = TaskId::new("t-1");
        let session_id = SessionId::new("s-1");

        // Create epic
        proj.apply_event(&RewindEvent::EpicCreated {
            epic_id: epic_id.clone(),
            title: "Test Epic".into(),
            description: "".into(),
            created_at: now,
            quality_gates: vec![],
        });

        // Create task with criteria
        proj.apply_event(&RewindEvent::TaskCreated {
            task_id: task_id.clone(),
            title: "Task 1".into(),
            description: "".into(),
            epic_id: Some(epic_id.clone()),
            created_at: now,
            acceptance_criteria: vec![
                AcceptanceCriterion {
                    description: "A".into(),
                    checked: false,
                },
                AcceptanceCriterion {
                    description: "B".into(),
                    checked: false,
                },
            ],
            story_type: None,
            depends_on: vec![],
        });

        // Session start
        proj.apply_event(&RewindEvent::SessionStarted {
            session_id: session_id.clone(),
            started_at: now,
        });

        // Task lifecycle
        let start = now + Duration::seconds(1);
        proj.apply_event(&RewindEvent::TaskStarted {
            task_id: task_id.clone(),
            started_at: start,
        });

        // Tool calls
        proj.apply_event(&RewindEvent::AgentToolCall {
            task_id: task_id.clone(),
            tool_name: "read_file".into(),
            args_summary: "".into(),
            result_summary: "".into(),
            called_at: now,
        });
        proj.apply_event(&RewindEvent::AgentToolCall {
            task_id: task_id.clone(),
            tool_name: "read_file".into(),
            args_summary: "".into(),
            result_summary: "".into(),
            called_at: now,
        });
        proj.apply_event(&RewindEvent::AgentToolCall {
            task_id: task_id.clone(),
            tool_name: "run_command".into(),
            args_summary: "".into(),
            result_summary: "".into(),
            called_at: now,
        });

        // Criterion checked
        proj.apply_event(&RewindEvent::CriterionChecked {
            task_id: task_id.clone(),
            criterion_index: 0,
            checked_at: now,
        });

        // Complete
        let end = start + Duration::seconds(30);
        proj.apply_event(&RewindEvent::TaskCompleted {
            task_id: task_id.clone(),
            completed_at: end,
        });

        // Session end
        proj.apply_event(&RewindEvent::SessionEnded {
            session_id: session_id.clone(),
            ended_at: end,
        });

        // Verify task metrics
        let task = &proj.tasks["t-1"];
        assert_eq!(task.outcome, TaskOutcome::Passed);
        assert_eq!(task.tool_call_count, 3);
        assert_eq!(task.criteria_total, 2);
        assert_eq!(task.criteria_checked, 1);
        assert!(task.duration_secs.unwrap() > 29.0);

        // Verify epic metrics
        let epic = &proj.epics["e-1"];
        assert_eq!(epic.total_tasks, 1);
        assert_eq!(epic.completed, 1);

        // Verify tool usage
        let usage = proj.tool_usage();
        assert_eq!(usage[0].tool_name, "read_file");
        assert_eq!(usage[0].call_count, 2);
        assert_eq!(usage[1].tool_name, "run_command");
        assert_eq!(usage[1].call_count, 1);

        // Verify session
        let sessions = proj.session_history();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].duration_secs.is_some());
    }

    #[test]
    fn task_failure_tracks_correctly() {
        let mut proj = AnalyticsProjection::default();
        let now = Utc::now();
        let task_id = TaskId::new("t-fail");

        proj.apply_event(&RewindEvent::TaskCreated {
            task_id: task_id.clone(),
            title: "Failing task".into(),
            description: "".into(),
            epic_id: None,
            created_at: now,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        });

        proj.apply_event(&RewindEvent::TaskStarted {
            task_id: task_id.clone(),
            started_at: now,
        });

        proj.apply_event(&RewindEvent::TaskFailed {
            task_id: task_id.clone(),
            reason: "Tests failed".into(),
            failed_at: now + Duration::seconds(10),
        });

        let task = &proj.tasks["t-fail"];
        assert_eq!(task.outcome, TaskOutcome::Failed);
        assert_eq!(task.failure_reason.as_deref(), Some("Tests failed"));
        assert!(task.duration_secs.unwrap() > 9.0);
    }

    #[test]
    fn epic_filter_works() {
        let mut proj = AnalyticsProjection::default();
        let now = Utc::now();

        proj.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-1"),
            title: "In epic".into(),
            description: "".into(),
            epic_id: Some(EpicId::new("e-1")),
            created_at: now,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        });

        proj.apply_event(&RewindEvent::TaskCreated {
            task_id: TaskId::new("t-2"),
            title: "No epic".into(),
            description: "".into(),
            epic_id: None,
            created_at: now,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        });

        assert_eq!(proj.task_summary(None).len(), 2);
        assert_eq!(proj.task_summary(Some("e-1")).len(), 1);
    }
}
