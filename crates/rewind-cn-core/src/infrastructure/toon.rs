use crate::application::analytics::IterationLog;
use crate::application::status::StatusSummary;
use crate::domain::model::TaskView;

/// Format a list of tasks as TOON (Token-Oriented Object Notation).
///
/// TOON uses pipe-delimited rows with a header, producing ~50% fewer tokens than JSON.
/// ```text
/// [id|title|status|epic_id|agent_id]
/// abc-123|Build auth|pending||
/// def-456|Fix login|in-progress|e-1|agent-1
/// ```
pub fn format_task_list(tasks: &[&TaskView]) -> String {
    let mut out = String::from("[id|title|status|epic_id|agent_id]\n");
    for t in tasks {
        out.push_str(&format!(
            "{}|{}|{}|{}|{}\n",
            t.task_id,
            t.title,
            t.status,
            t.epic_id
                .as_ref()
                .map(|e| e.to_string())
                .unwrap_or_default(),
            t.agent_id
                .as_ref()
                .map(|a| a.to_string())
                .unwrap_or_default(),
        ));
    }
    out
}

/// Format a single task as TOON key-value lines.
pub fn format_task_detail(task: &TaskView) -> String {
    let mut out = String::new();
    out.push_str(&format!("id:{}\n", task.task_id));
    out.push_str(&format!("title:{}\n", task.title));
    out.push_str(&format!("status:{}\n", task.status));
    if !task.description.is_empty() {
        out.push_str(&format!("desc:{}\n", task.description));
    }
    if let Some(ref eid) = task.epic_id {
        out.push_str(&format!("epic:{}\n", eid));
    }
    if let Some(ref aid) = task.agent_id {
        out.push_str(&format!("agent:{}\n", aid));
    }
    out.push_str(&format!("created:{}\n", task.created_at));
    out
}

/// Format a status summary as TOON.
/// ```text
/// total:5
/// pending:3
/// completed:2
/// ---
/// [epic_id|title|done|total|pct|completed]
/// e-1|Sprint 1|2|5|40|false
/// ```
pub fn format_status(summary: &StatusSummary) -> String {
    let mut out = String::new();
    out.push_str(&format!("total:{}\n", summary.total_tasks));

    let statuses = [
        "pending",
        "assigned",
        "in-progress",
        "completed",
        "failed",
        "blocked",
    ];
    for s in &statuses {
        if let Some(&count) = summary.by_status.get(*s) {
            if count > 0 {
                out.push_str(&format!("{s}:{count}\n"));
            }
        }
    }

    if !summary.epics.is_empty() {
        out.push_str("---\n");
        out.push_str("[epic_id|title|done|total|pct|completed]\n");
        for e in &summary.epics {
            let pct = if e.total_tasks == 0 {
                0
            } else {
                (e.completed_tasks * 100) / e.total_tasks
            };
            out.push_str(&format!(
                "{}|{}|{}|{}|{}|{}\n",
                e.epic_id, e.title, e.completed_tasks, e.total_tasks, pct, e.is_completed
            ));
        }
    }
    out
}

/// Format iteration logs as TOON.
pub fn format_iteration_list(iterations: &[&IterationLog]) -> String {
    let mut out = String::from("[iter|task_id|duration_ms|output]\n");
    for it in iterations {
        let truncated = truncate_output(&it.agent_output, 120);
        out.push_str(&format!(
            "{}|{}|{}|{}\n",
            it.iteration_number, it.task_id, it.duration_ms, truncated
        ));
    }
    out
}

/// Truncate output to max_len characters, replacing the tail with "…" if needed.
fn truncate_output(s: &str, max_len: usize) -> String {
    let single_line = s.replace('\n', " ");
    if single_line.len() <= max_len {
        single_line
    } else {
        format!("{}…", &single_line[..max_len])
    }
}

/// Format an epic list as TOON.
pub fn format_epic_list(epics: &[&crate::domain::model::EpicProgress]) -> String {
    let mut out = String::from("[epic_id|title|done|total|completed]\n");
    for e in epics {
        out.push_str(&format!(
            "{}|{}|{}|{}|{}\n",
            e.epic_id, e.title, e.completed_tasks, e.total_tasks, e.is_completed
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::status::EpicSummary;
    use crate::domain::ids::TaskId;
    use crate::domain::model::TaskStatus;
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn format_task_list_produces_toon() {
        let task = TaskView {
            task_id: TaskId::new("t-1"),
            title: "Build auth".into(),
            description: "".into(),
            status: TaskStatus::Pending,
            epic_id: None,
            agent_id: None,
            created_at: Utc::now(),
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        };
        let tasks = vec![&task];
        let output = format_task_list(&tasks);
        assert!(output.starts_with("[id|title|status|epic_id|agent_id]\n"));
        assert!(output.contains("t-1|Build auth|pending||"));
    }

    #[test]
    fn format_status_produces_toon() {
        let mut by_status = HashMap::new();
        by_status.insert("pending".to_string(), 3);
        by_status.insert("completed".to_string(), 2);

        let summary = StatusSummary {
            total_tasks: 5,
            by_status,
            epics: vec![EpicSummary {
                epic_id: "e-1".into(),
                title: "Sprint 1".into(),
                total_tasks: 5,
                completed_tasks: 2,
                is_completed: false,
            }],
        };

        let output = format_status(&summary);
        assert!(output.contains("total:5"));
        assert!(output.contains("pending:3"));
        assert!(output.contains("completed:2"));
        assert!(output.contains("e-1|Sprint 1|2|5|40|false"));
    }

    #[test]
    fn format_task_detail_produces_kv() {
        let task = TaskView {
            task_id: TaskId::new("t-1"),
            title: "Build auth".into(),
            description: "Implement OAuth".into(),
            status: TaskStatus::InProgress,
            epic_id: None,
            agent_id: None,
            created_at: Utc::now(),
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        };
        let output = format_task_detail(&task);
        assert!(output.contains("id:t-1"));
        assert!(output.contains("title:Build auth"));
        assert!(output.contains("status:in-progress"));
        assert!(output.contains("desc:Implement OAuth"));
    }
}
