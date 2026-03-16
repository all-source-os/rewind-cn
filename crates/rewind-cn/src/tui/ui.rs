use ratatui::prelude::*;
use ratatui::widgets::*;

use rewind_cn_core::domain::model::TaskStatus;

use super::app::{App, BrowseApp};

// ---------------------------------------------------------------------------
// Epic browser view
// ---------------------------------------------------------------------------

pub fn draw_browse(f: &mut Frame, app: &BrowseApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top bar
            Constraint::Min(10),   // Main area
            Constraint::Length(3), // Bottom bar
        ])
        .split(f.area());

    draw_browse_top(f, app, chunks[0]);
    draw_browse_main(f, app, chunks[1]);
    draw_browse_bottom(f, chunks[2]);
}

fn draw_browse_top(f: &mut Frame, app: &BrowseApp, area: Rect) {
    let total = app.epics.len();
    let open = app.epics.iter().filter(|e| e.status == "open").count();
    let title = format!(" rewind — Select an Epic  ({open} open / {total} total) ");

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Cyan));

    f.render_widget(block, area);
}

fn draw_browse_main(f: &mut Frame, app: &BrowseApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    draw_epic_list(f, app, chunks[0]);
    draw_epic_detail(f, app, chunks[1]);
}

fn draw_epic_list(f: &mut Frame, app: &BrowseApp, area: Rect) {
    let items: Vec<ListItem> = app
        .epics
        .iter()
        .map(|e| {
            let icon = match e.status.as_str() {
                "open" => "●",
                "done" | "closed" => "✓",
                _ => "○",
            };
            let style = match e.status.as_str() {
                "open" => Style::default().fg(Color::White),
                "done" | "closed" => Style::default().fg(Color::DarkGray),
                _ => Style::default(),
            };
            let children_count = e.children.len();
            ListItem::new(format!(
                "{icon} {} [{}/{}]",
                e.title,
                e.priority,
                children_count
            ))
            .style(style)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Epics "))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    state.select(Some(app.selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_epic_detail(f: &mut Frame, app: &BrowseApp, area: Rect) {
    let Some(epic) = app.selected_epic() else {
        let block = Block::default().borders(Borders::ALL).title(" Details ");
        f.render_widget(block, area);
        return;
    };

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        &epic.title,
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(format!(
        "Status: {}  Priority: {}",
        epic.status, epic.priority
    )));
    lines.push(Line::from(format!("ID: {}", epic.id)));
    lines.push(Line::from(""));

    if epic.children.is_empty() {
        lines.push(Line::from(Span::styled(
            "No tasks",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let open = epic.children.iter().filter(|c| c.status == "open").count();
        let done = epic.children.iter().filter(|c| c.status == "done").count();
        lines.push(Line::from(Span::styled(
            format!("Tasks ({} total, {} open, {} done):", epic.children.len(), open, done),
            Style::default().add_modifier(Modifier::BOLD),
        )));

        for child in &epic.children {
            let icon = match child.status.as_str() {
                "open" => "○",
                "done" | "closed" => "✓",
                "claimed" | "in-progress" => "◉",
                _ => "○",
            };
            let style = match child.status.as_str() {
                "done" | "closed" => Style::default().fg(Color::DarkGray),
                "claimed" | "in-progress" => Style::default().fg(Color::Yellow),
                _ => Style::default(),
            };
            lines.push(Line::from(Span::styled(
                format!("  {icon} {}", child.title),
                style,
            )));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", epic.id)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn draw_browse_bottom(f: &mut Frame, area: Rect) {
    let text = " ↑↓/jk = navigate   Enter = select & import   q/Esc = quit ";
    let bar = Paragraph::new(text)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray))
        .block(Block::default());
    f.render_widget(bar, area);
}

// ---------------------------------------------------------------------------
// Execution dashboard view (existing)
// ---------------------------------------------------------------------------

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top bar: epic progress
            Constraint::Min(10),   // Main area: task list + detail
            Constraint::Length(3), // Bottom bar: session info
        ])
        .split(f.area());

    draw_top_bar(f, app, chunks[0]);
    draw_main(f, app, chunks[1]);
    draw_bottom_bar(f, app, chunks[2]);
}

fn draw_top_bar(f: &mut Frame, app: &App, area: Rect) {
    let title = app.epic_title.as_deref().unwrap_or("rewind dashboard");

    let pct = if app.epic_total > 0 {
        app.epic_completed * 100 / app.epic_total
    } else {
        0
    };

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(format!(
            " {} [{}/{}] ",
            title, app.epic_completed, app.epic_total
        )))
        .gauge_style(Style::default().fg(Color::Green))
        .percent(pct as u16);

    f.render_widget(gauge, area);
}

fn draw_main(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40), // Task list
            Constraint::Percentage(60), // Detail panel
        ])
        .split(area);

    draw_task_list(f, app, chunks[0]);
    draw_detail_panel(f, app, chunks[1]);
}

fn draw_task_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .tasks
        .iter()
        .map(|t| {
            let icon = match t.status {
                TaskStatus::Pending => "○",
                TaskStatus::Assigned => "◎",
                TaskStatus::InProgress => "◉",
                TaskStatus::Completed => "✓",
                TaskStatus::Failed => "✗",
                TaskStatus::Blocked => "⊘",
            };

            let criteria = if t.criteria_total > 0 {
                format!(" ({}/{})", t.criteria_checked, t.criteria_total)
            } else {
                String::new()
            };

            let style = match t.status {
                TaskStatus::InProgress => Style::default().fg(Color::Yellow),
                TaskStatus::Completed => Style::default().fg(Color::Green),
                TaskStatus::Failed => Style::default().fg(Color::Red),
                TaskStatus::Blocked => Style::default().fg(Color::DarkGray),
                _ => Style::default(),
            };

            ListItem::new(format!("{icon} {}{criteria}", t.title)).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Tasks "))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    state.select(Some(app.selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_detail_panel(f: &mut Frame, app: &App, area: Rect) {
    let Some(task) = app.selected_task() else {
        let block = Block::default().borders(Borders::ALL).title(" Details ");
        f.render_widget(block, area);
        return;
    };

    let mut lines: Vec<Line> = Vec::new();

    // Title and status
    lines.push(Line::from(Span::styled(
        &task.title,
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(format!("Status: {:?}", task.status)));
    lines.push(Line::from(""));

    // Acceptance criteria
    if task.criteria_total > 0 {
        lines.push(Line::from(Span::styled(
            "Acceptance Criteria:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for i in 0..task.criteria_total {
            let check = if i < task.criteria_checked {
                "[x]"
            } else {
                "[ ]"
            };
            lines.push(Line::from(format!("  {check} Criterion {}", i + 1)));
        }
        lines.push(Line::from(""));
    }

    // Failure reason
    if let Some(reason) = &task.failure_reason {
        lines.push(Line::from(Span::styled(
            format!("Failed: {reason}"),
            Style::default().fg(Color::Red),
        )));
        lines.push(Line::from(""));
    }

    // Recent tool calls (last 10)
    if !task.tool_calls.is_empty() {
        lines.push(Line::from(Span::styled(
            "Tool Calls:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        let start = task.tool_calls.len().saturating_sub(10);
        for call in &task.tool_calls[start..] {
            let time = call.called_at.format("%H:%M:%S");
            let args = if call.args_summary.len() > 40 {
                format!("{}…", &call.args_summary[..39])
            } else {
                call.args_summary.clone()
            };
            lines.push(Line::from(format!("  [{time}] {} {args}", call.tool_name)));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", task.task_id)),
        )
        .scroll((app.detail_scroll, 0))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn draw_bottom_bar(f: &mut Frame, app: &App, area: Rect) {
    let duration = app
        .session_started
        .map(|s| {
            let elapsed = chrono::Utc::now() - s;
            format!("{}s", elapsed.num_seconds())
        })
        .unwrap_or_else(|| "-".into());

    let status = format!(
        " Session: {} | Done: {} | Failed: {} | q=quit j/k=nav",
        duration, app.epic_completed, app.epic_failed
    );

    let bar = Paragraph::new(status)
        .style(Style::default().fg(Color::White).bg(Color::DarkGray))
        .block(Block::default());

    f.render_widget(bar, area);
}
