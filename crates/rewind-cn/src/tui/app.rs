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
use rewind_cn_core::domain::model::TaskStatus;

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
}

/// Run the TUI dashboard with a broadcast receiver for events.
pub async fn run_dashboard(mut event_rx: broadcast::Receiver<RewindEvent>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    // Set up panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    loop {
        terminal.draw(|f| super::ui::draw(f, &app))?;

        // Poll for events with timeout
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
                    // Engine dropped — orchestrator finished
                    app.log_messages.push("Execution complete.".into());
                    // Keep running so user can see final state
                    break;
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
