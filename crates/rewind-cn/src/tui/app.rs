use std::io;
use std::time::Duration;

use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use tokio::sync::broadcast;

use rewind_cn_core::domain::events::RewindEvent;
use rewind_cn_core::domain::model::{BacklogProjection, EpicProgressProjection, TaskStatus};
use rewind_cn_core::infrastructure::chronis::ChronisTask;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

/// A task as displayed in the TUI.
#[derive(Debug, Clone)]
pub struct TaskState {
    pub task_id: String,
    pub title: String,
    pub status: TaskStatus,
    pub criteria_total: usize,
    pub criteria_checked: usize,
    pub tool_calls: Vec<ToolCallEntry>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolCallEntry {
    pub tool_name: String,
    pub args_summary: String,
    pub called_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Epic browser types
// ---------------------------------------------------------------------------

/// An epic entry for the browse view.
#[derive(Debug, Clone)]
pub struct EpicEntry {
    pub id: String,
    pub title: String,
    pub priority: String,
    pub status: String,
    pub children: Vec<ChronisTask>,
}

/// State for the epic browser.
pub struct BrowseApp {
    pub epics: Vec<EpicEntry>,
    pub selected: usize,
}

impl BrowseApp {
    pub fn new(epics: Vec<EpicEntry>) -> Self {
        Self {
            epics,
            selected: 0,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.epics.len() {
            self.selected += 1;
        }
    }

    pub fn selected_epic(&self) -> Option<&EpicEntry> {
        self.epics.get(self.selected)
    }
}

// ---------------------------------------------------------------------------
// Execution monitor (existing)
// ---------------------------------------------------------------------------

/// Application state for the TUI dashboard.
pub struct App {
    pub tasks: Vec<TaskState>,
    pub selected: usize,
    pub epic_title: Option<String>,
    pub epic_total: usize,
    pub epic_completed: usize,
    pub epic_failed: usize,
    pub session_started: Option<DateTime<Utc>>,
    pub log_messages: Vec<String>,
    pub should_quit: bool,
    pub detail_scroll: u16,
}

impl App {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            selected: 0,
            epic_title: None,
            epic_total: 0,
            epic_completed: 0,
            epic_failed: 0,
            session_started: None,
            log_messages: Vec::new(),
            should_quit: false,
            detail_scroll: 0,
        }
    }

    pub fn apply_event(&mut self, event: &RewindEvent) {
        match event {
            RewindEvent::EpicCreated { title, .. } => {
                self.epic_title = Some(title.clone());
            }

            RewindEvent::TaskCreated {
                task_id,
                title,
                acceptance_criteria,
                ..
            } => {
                self.tasks.push(TaskState {
                    task_id: task_id.to_string(),
                    title: title.clone(),
                    status: TaskStatus::Pending,
                    criteria_total: acceptance_criteria.len(),
                    criteria_checked: 0,
                    tool_calls: Vec::new(),
                    failure_reason: None,
                });
                self.epic_total = self.tasks.len();
            }

            RewindEvent::TaskAssigned { task_id, .. } => {
                if let Some(t) = self.find_task_mut(task_id.as_ref()) {
                    t.status = TaskStatus::Assigned;
                }
            }

            RewindEvent::TaskStarted { task_id, .. } => {
                if let Some(t) = self.find_task_mut(task_id.as_ref()) {
                    t.status = TaskStatus::InProgress;
                }
                self.log_messages.push(format!("Started: {}", task_id));
            }

            RewindEvent::TaskCompleted { task_id, .. } => {
                if let Some(t) = self.find_task_mut(task_id.as_ref()) {
                    t.status = TaskStatus::Completed;
                }
                self.epic_completed += 1;
                self.log_messages.push(format!("Completed: {}", task_id));
            }

            RewindEvent::TaskFailed {
                task_id, reason, ..
            } => {
                if let Some(t) = self.find_task_mut(task_id.as_ref()) {
                    t.status = TaskStatus::Failed;
                    t.failure_reason = Some(reason.clone());
                }
                self.epic_failed += 1;
                self.log_messages
                    .push(format!("Failed: {} ({})", task_id, reason));
            }

            RewindEvent::AgentToolCall {
                task_id,
                tool_name,
                args_summary,
                called_at,
                ..
            } => {
                if let Some(t) = self.find_task_mut(task_id.as_ref()) {
                    t.tool_calls.push(ToolCallEntry {
                        tool_name: tool_name.clone(),
                        args_summary: args_summary.clone(),
                        called_at: *called_at,
                    });
                }
            }

            RewindEvent::CriterionChecked { task_id, .. } => {
                if let Some(t) = self.find_task_mut(task_id.as_ref()) {
                    t.criteria_checked += 1;
                }
            }

            RewindEvent::SessionStarted { started_at, .. } => {
                self.session_started = Some(*started_at);
            }

            RewindEvent::QualityGateRan {
                command, passed, ..
            } => {
                let status = if *passed { "PASS" } else { "FAIL" };
                self.log_messages.push(format!("Gate {status}: {command}"));
            }

            _ => {}
        }
    }

    fn find_task_mut(&mut self, task_id: &str) -> Option<&mut TaskState> {
        self.tasks.iter_mut().find(|t| t.task_id == task_id)
    }

    pub fn selected_task(&self) -> Option<&TaskState> {
        self.tasks.get(self.selected)
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.detail_scroll = 0;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.tasks.len() {
            self.selected += 1;
            self.detail_scroll = 0;
        }
    }

    pub fn scroll_detail_up(&mut self) {
        if self.detail_scroll > 0 {
            self.detail_scroll -= 1;
        }
    }

    pub fn scroll_detail_down(&mut self) {
        self.detail_scroll += 1;
    }

    /// Seed the TUI with existing tasks from the engine backlog.
    pub fn seed_from_backlog(
        &mut self,
        backlog: &BacklogProjection,
        epic_progress: &EpicProgressProjection,
    ) {
        for task in backlog.tasks.values() {
            let (checked, _total) = backlog.criteria_checked_count(task.task_id.as_ref());
            self.tasks.push(TaskState {
                task_id: task.task_id.to_string(),
                title: task.title.clone(),
                status: task.status.clone(),
                criteria_total: task.acceptance_criteria.len(),
                criteria_checked: checked,
                tool_calls: Vec::new(),
                failure_reason: None,
            });
        }
        self.epic_total = self.tasks.len();
        self.epic_completed = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Completed)
            .count();
        self.epic_failed = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Failed)
            .count();

        // Set epic title from the first epic in the progress projection
        if let Some(epic) = epic_progress.epics.values().next() {
            self.epic_title = Some(epic.title.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal helpers
// ---------------------------------------------------------------------------

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Epic browser TUI
// ---------------------------------------------------------------------------

/// Launch the epic browser TUI. Returns the selected epic ID, or None if user quit.
pub async fn run_epic_browser(epics: Vec<EpicEntry>) -> io::Result<Option<String>> {
    let mut terminal = setup_terminal()?;

    let mut app = BrowseApp::new(epics);

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    let result = loop {
        terminal.draw(|f| super::ui::draw_browse(f, &app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            break None;
                        }
                        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                        KeyCode::Enter => {
                            if let Some(epic) = app.selected_epic() {
                                break Some(epic.id.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    };

    restore_terminal(&mut terminal)?;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Execution dashboard TUI
// ---------------------------------------------------------------------------

/// Run the TUI dashboard with a broadcast receiver for events.
///
/// Accepts optional backlog and epic progress snapshots to seed the initial state,
/// so tasks imported before the TUI subscribes are visible immediately.
pub async fn run_dashboard(
    mut event_rx: broadcast::Receiver<RewindEvent>,
    backlog: Option<&BacklogProjection>,
    epic_progress: Option<&EpicProgressProjection>,
) -> io::Result<()> {
    let mut terminal = setup_terminal()?;

    let mut app = App::new();
    if let (Some(bl), Some(ep)) = (backlog, epic_progress) {
        app.seed_from_backlog(bl, ep);
    }

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    loop {
        terminal.draw(|f| super::ui::draw(f, &app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.should_quit = true;
                        }
                        KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                        KeyCode::PageUp => app.scroll_detail_up(),
                        KeyCode::PageDown => app.scroll_detail_down(),
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }

        // Drain all available events from the broadcast channel
        loop {
            match event_rx.try_recv() {
                Ok(ev) => app.apply_event(&ev),
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    app.log_messages
                        .push(format!("Skipped {n} events (TUI too slow)"));
                    break;
                }
                Err(broadcast::error::TryRecvError::Closed) => {
                    app.log_messages.push("Execution complete.".into());
                    break;
                }
            }
        }
    }

    restore_terminal(&mut terminal)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rewind_cn_core::domain::events::{AcceptanceCriterion, RewindEvent};
    use rewind_cn_core::domain::ids::{EpicId, TaskId};

    fn make_backlog_with_events(
        events: &[RewindEvent],
    ) -> (BacklogProjection, EpicProgressProjection) {
        let mut backlog = BacklogProjection::default();
        let mut epic_progress = EpicProgressProjection::default();
        for ev in events {
            backlog.apply_event(ev);
            epic_progress.apply_event(ev);
        }
        (backlog, epic_progress)
    }

    #[test]
    fn seed_from_backlog_populates_tasks() {
        let epic_id = EpicId::new("epic-1");
        let task1 = TaskId::new("task-1");
        let task2 = TaskId::new("task-2");

        let events = vec![
            RewindEvent::EpicCreated {
                epic_id: epic_id.clone(),
                title: "Test Epic".into(),
                description: String::new(),
                created_at: Utc::now(),
                quality_gates: vec![],
            },
            RewindEvent::TaskCreated {
                task_id: task1.clone(),
                title: "Task One".into(),
                description: "First task".into(),
                epic_id: Some(epic_id.clone()),
                created_at: Utc::now(),
                acceptance_criteria: vec![
                    AcceptanceCriterion {
                        description: "Criterion A".into(),
                        checked: false,
                    },
                    AcceptanceCriterion {
                        description: "Criterion B".into(),
                        checked: false,
                    },
                ],
                story_type: None,
                depends_on: vec![],
            },
            RewindEvent::TaskCreated {
                task_id: task2.clone(),
                title: "Task Two".into(),
                description: "Second task".into(),
                epic_id: Some(epic_id.clone()),
                created_at: Utc::now(),
                acceptance_criteria: vec![AcceptanceCriterion {
                    description: "Criterion C".into(),
                    checked: false,
                }],
                story_type: None,
                depends_on: vec![task1.clone()],
            },
        ];

        let (backlog, epic_progress) = make_backlog_with_events(&events);
        let mut app = App::new();
        app.seed_from_backlog(&backlog, &epic_progress);

        assert_eq!(app.tasks.len(), 2, "should have 2 tasks");
        assert_eq!(app.epic_total, 2);
        assert_eq!(app.epic_completed, 0);
        assert_eq!(app.epic_failed, 0);
        assert_eq!(app.epic_title, Some("Test Epic".into()));
    }

    #[test]
    fn seed_from_backlog_counts_completed_and_failed() {
        let epic_id = EpicId::new("epic-1");
        let task1 = TaskId::new("task-1");
        let task2 = TaskId::new("task-2");
        let task3 = TaskId::new("task-3");

        let events = vec![
            RewindEvent::EpicCreated {
                epic_id: epic_id.clone(),
                title: "Epic".into(),
                description: String::new(),
                created_at: Utc::now(),
                quality_gates: vec![],
            },
            RewindEvent::TaskCreated {
                task_id: task1.clone(),
                title: "Done Task".into(),
                description: String::new(),
                epic_id: Some(epic_id.clone()),
                created_at: Utc::now(),
                acceptance_criteria: vec![],
                story_type: None,
                depends_on: vec![],
            },
            RewindEvent::TaskCompleted {
                task_id: task1.clone(),
                completed_at: Utc::now(),
            },
            RewindEvent::TaskCreated {
                task_id: task2.clone(),
                title: "Failed Task".into(),
                description: String::new(),
                epic_id: Some(epic_id.clone()),
                created_at: Utc::now(),
                acceptance_criteria: vec![],
                story_type: None,
                depends_on: vec![],
            },
            RewindEvent::TaskFailed {
                task_id: task2.clone(),
                reason: "oops".into(),
                failed_at: Utc::now(),
            },
            RewindEvent::TaskCreated {
                task_id: task3.clone(),
                title: "Pending Task".into(),
                description: String::new(),
                epic_id: Some(epic_id.clone()),
                created_at: Utc::now(),
                acceptance_criteria: vec![],
                story_type: None,
                depends_on: vec![],
            },
        ];

        let (backlog, epic_progress) = make_backlog_with_events(&events);
        let mut app = App::new();
        app.seed_from_backlog(&backlog, &epic_progress);

        assert_eq!(app.tasks.len(), 3);
        assert_eq!(app.epic_total, 3);
        assert_eq!(app.epic_completed, 1);
        assert_eq!(app.epic_failed, 1);
    }

    #[test]
    fn seed_from_backlog_tracks_checked_criteria() {
        let epic_id = EpicId::new("epic-1");
        let task_id = TaskId::new("task-1");

        let events = vec![
            RewindEvent::EpicCreated {
                epic_id: epic_id.clone(),
                title: "Epic".into(),
                description: String::new(),
                created_at: Utc::now(),
                quality_gates: vec![],
            },
            RewindEvent::TaskCreated {
                task_id: task_id.clone(),
                title: "Task".into(),
                description: String::new(),
                epic_id: Some(epic_id.clone()),
                created_at: Utc::now(),
                acceptance_criteria: vec![
                    AcceptanceCriterion {
                        description: "A".into(),
                        checked: false,
                    },
                    AcceptanceCriterion {
                        description: "B".into(),
                        checked: false,
                    },
                    AcceptanceCriterion {
                        description: "C".into(),
                        checked: false,
                    },
                ],
                story_type: None,
                depends_on: vec![],
            },
            RewindEvent::CriterionChecked {
                task_id: task_id.clone(),
                criterion_index: 0,
                checked_at: Utc::now(),
            },
            RewindEvent::CriterionChecked {
                task_id: task_id.clone(),
                criterion_index: 2,
                checked_at: Utc::now(),
            },
        ];

        let (backlog, epic_progress) = make_backlog_with_events(&events);
        let mut app = App::new();
        app.seed_from_backlog(&backlog, &epic_progress);

        assert_eq!(app.tasks.len(), 1);
        let task = &app.tasks[0];
        assert_eq!(task.criteria_total, 3);
        assert_eq!(task.criteria_checked, 2, "should show 2 of 3 criteria checked");
    }

    #[test]
    fn seed_empty_backlog_is_noop() {
        let backlog = BacklogProjection::default();
        let epic_progress = EpicProgressProjection::default();
        let mut app = App::new();
        app.seed_from_backlog(&backlog, &epic_progress);

        assert!(app.tasks.is_empty());
        assert_eq!(app.epic_total, 0);
        assert_eq!(app.epic_title, None);
    }
}
