use std::process::Command;

use tracing::{debug, warn};

use crate::domain::error::RewindError;

/// Bridge to the chronis (`cn`) CLI for task readiness and lifecycle tracking.
///
/// Chronis owns the backlog (definitions, dependencies, readiness).
/// Rewind owns execution state (sessions, agent assignments, events).
pub struct ChronisBridge;

/// A task returned by `cn ready --toon`.
#[derive(Debug, Clone)]
pub struct ChronisTask {
    pub id: String,
    pub title: String,
    pub task_type: String,
    pub priority: String,
    pub status: String,
}

impl ChronisBridge {
    /// Check if the `cn` CLI is available on PATH.
    pub fn is_available() -> bool {
        Command::new("cn")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get open + unblocked tasks via `cn ready --toon`.
    /// Returns parsed TOON rows as `ChronisTask` structs.
    pub fn ready_tasks() -> Result<Vec<ChronisTask>, RewindError> {
        let output = run_cn(&["ready", "--toon"])?;
        parse_toon_list(&output)
    }

    /// Claim a task: `cn claim <id> --toon`.
    /// Returns the acknowledgement string (e.g. "ok:claimed:<id>").
    pub fn claim(task_id: &str) -> Result<String, RewindError> {
        let output = run_cn(&["claim", task_id, "--toon"])?;
        Ok(output.trim().to_string())
    }

    /// Mark a task done: `cn done <id> --toon`.
    /// Returns the acknowledgement string (e.g. "ok:done:<id>").
    pub fn done(task_id: &str) -> Result<String, RewindError> {
        let output = run_cn(&["done", task_id, "--toon"])?;
        Ok(output.trim().to_string())
    }

    /// Mark a task failed: `cn done <id> --reason "msg" --toon`.
    pub fn fail(task_id: &str, reason: &str) -> Result<String, RewindError> {
        let output = run_cn(&["done", task_id, "--reason", reason, "--toon"])?;
        Ok(output.trim().to_string())
    }

    /// List all tasks: `cn list --toon`.
    pub fn list_tasks() -> Result<Vec<ChronisTask>, RewindError> {
        let output = run_cn(&["list", "--toon"])?;
        parse_toon_list(&output)
    }

    /// Show a single task: `cn show <id> --toon`.
    pub fn show_task(task_id: &str) -> Result<String, RewindError> {
        run_cn(&["show", task_id, "--toon"])
    }
}

/// Execute a `cn` command and return stdout.
fn run_cn(args: &[&str]) -> Result<String, RewindError> {
    debug!("Running: cn {}", args.join(" "));

    let output = Command::new("cn")
        .args(args)
        .output()
        .map_err(|e| RewindError::Storage(format!("Failed to execute cn: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("cn {} failed: {}", args.join(" "), stderr);
        return Err(RewindError::Storage(format!(
            "cn {} failed: {}",
            args.join(" "),
            stderr.trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parse TOON pipe-delimited output into `ChronisTask` structs.
///
/// TOON format:
/// ```text
/// [id|type|title|pri|status]
/// abc-123|feat|Build auth|high|open
/// def-456|bug|Fix login|med|open
/// ```
fn parse_toon_list(output: &str) -> Result<Vec<ChronisTask>, RewindError> {
    let mut tasks = Vec::new();
    let lines: Vec<&str> = output.lines().collect();

    if lines.is_empty() {
        return Ok(tasks);
    }

    // Skip header line (starts with '[')
    let data_start = if lines[0].starts_with('[') { 1 } else { 0 };

    for line in &lines[data_start..] {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() >= 5 {
            tasks.push(ChronisTask {
                id: fields[0].to_string(),
                task_type: fields[1].to_string(),
                title: fields[2].to_string(),
                priority: fields[3].to_string(),
                status: fields[4].to_string(),
            });
        } else if fields.len() >= 2 {
            // Minimal format: id|title
            tasks.push(ChronisTask {
                id: fields[0].to_string(),
                title: fields.get(1).unwrap_or(&"").to_string(),
                task_type: String::new(),
                priority: String::new(),
                status: String::new(),
            });
        }
    }

    Ok(tasks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_toon_with_header() {
        let input = "[id|type|title|pri|status]\nabc-123|feat|Build auth|high|open\ndef-456|bug|Fix login|med|open\n";
        let tasks = parse_toon_list(input).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "abc-123");
        assert_eq!(tasks[0].title, "Build auth");
        assert_eq!(tasks[0].task_type, "feat");
        assert_eq!(tasks[1].id, "def-456");
        assert_eq!(tasks[1].status, "open");
    }

    #[test]
    fn parse_toon_empty() {
        let tasks = parse_toon_list("").unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn parse_toon_no_header() {
        let input = "abc-123|feat|Build auth|high|open\n";
        let tasks = parse_toon_list(input).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "abc-123");
    }

    #[test]
    fn cn_availability_check() {
        // This test just verifies the function doesn't panic.
        // It returns true/false depending on whether cn is installed.
        let _ = ChronisBridge::is_available();
    }
}
