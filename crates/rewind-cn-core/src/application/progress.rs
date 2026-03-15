use crate::domain::events::{ProgressNoteType, RewindEvent};

/// Default number of progress notes to include in the projection.
pub const DEFAULT_PROGRESS_LIMIT: usize = 20;

/// Project recent `ProgressNoted` events into a markdown-formatted summary.
///
/// Scans the event stream in order, collects `ProgressNoted` entries,
/// and returns the most recent `limit` notes as a markdown bullet list.
pub fn project_progress_notes(events: &[RewindEvent], limit: usize) -> String {
    let notes: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            RewindEvent::ProgressNoted {
                task_id,
                note,
                note_type,
                ..
            } => Some((task_id, note, note_type)),
            _ => None,
        })
        .collect();

    if notes.is_empty() {
        return String::new();
    }

    // Take the last `limit` notes (most recent)
    let start = notes.len().saturating_sub(limit);
    let recent = &notes[start..];

    let mut lines = Vec::with_capacity(recent.len());
    for (task_id, note, note_type) in recent {
        let tag = match note_type {
            ProgressNoteType::TaskCompleted => "completed",
            ProgressNoteType::TaskFailed => "failed",
            ProgressNoteType::Discretionary => "note",
        };
        let task_ref = match task_id {
            Some(id) => format!(" (task: {})", id),
            None => String::new(),
        };
        lines.push(format!("- **[{tag}]**{task_ref} {note}"));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::events::RewindEvent;
    use crate::domain::ids::{SessionId, TaskId};

    fn make_progress_event(
        task_id: Option<&str>,
        note: &str,
        note_type: ProgressNoteType,
    ) -> RewindEvent {
        RewindEvent::ProgressNoted {
            session_id: SessionId::new("sess-1"),
            task_id: task_id.map(TaskId::new),
            note: note.into(),
            note_type,
            noted_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn empty_events_returns_empty_string() {
        assert_eq!(project_progress_notes(&[], 20), "");
    }

    #[test]
    fn ignores_non_progress_events() {
        let events = vec![RewindEvent::SessionStarted {
            session_id: SessionId::new("sess-1"),
            started_at: chrono::Utc::now(),
        }];
        assert_eq!(project_progress_notes(&events, 20), "");
    }

    #[test]
    fn formats_progress_notes_as_markdown() {
        let events = vec![
            make_progress_event(
                Some("task-1"),
                "Task task-1 completed",
                ProgressNoteType::TaskCompleted,
            ),
            make_progress_event(
                Some("task-2"),
                "Timeout needs increasing",
                ProgressNoteType::Discretionary,
            ),
            make_progress_event(None, "General observation", ProgressNoteType::Discretionary),
        ];

        let result = project_progress_notes(&events, 20);
        assert!(result.contains("- **[completed]** (task: task-1) Task task-1 completed"));
        assert!(result.contains("- **[note]** (task: task-2) Timeout needs increasing"));
        assert!(result.contains("- **[note]** General observation"));
        // No task ref for None task_id
        assert!(!result.contains("(task:) General"));
    }

    #[test]
    fn respects_limit() {
        let events: Vec<_> = (0..30)
            .map(|i| {
                make_progress_event(
                    Some(&format!("task-{i}")),
                    &format!("Note {i}"),
                    ProgressNoteType::Discretionary,
                )
            })
            .collect();

        let result = project_progress_notes(&events, 5);
        let line_count = result.lines().count();
        assert_eq!(line_count, 5);
        // Should contain the last 5 (25-29)
        assert!(result.contains("Note 25"));
        assert!(result.contains("Note 29"));
        assert!(!result.contains("Note 24"));
    }

    #[test]
    fn failed_note_type_tagged_correctly() {
        let events = vec![make_progress_event(
            Some("task-3"),
            "Build failed: missing dep",
            ProgressNoteType::TaskFailed,
        )];

        let result = project_progress_notes(&events, 20);
        assert!(result.contains("**[failed]**"));
    }
}
