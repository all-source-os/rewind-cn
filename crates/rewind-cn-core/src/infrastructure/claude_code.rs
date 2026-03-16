//! Claude Code CLI backend for task execution.
//!
//! Shells out to the `claude` CLI (Claude Code) instead of calling the
//! Anthropic API directly. This reuses Claude Code's built-in tools
//! (Read, Edit, Bash, etc.) and authentication.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::domain::error::RewindError;
use crate::domain::events::AcceptanceCriterion;
use crate::infrastructure::coder::{PromptContext, TaskExecutor, ToolCallRecord};

/// Configuration for the Claude Code CLI backend.
#[derive(Debug, Clone)]
pub struct ClaudeCodeConfig {
    /// Model to use (e.g. "claude-sonnet-4-5-20250514"). None = claude's default.
    pub model: Option<String>,
    /// Maximum number of agentic turns.
    pub max_turns: Option<u32>,
    /// Whether to use --dangerously-skip-permissions.
    pub skip_permissions: bool,
}

impl Default for ClaudeCodeConfig {
    fn default() -> Self {
        Self {
            model: None,
            max_turns: None,
            skip_permissions: true,
        }
    }
}

/// Task executor that delegates to the `claude` CLI.
pub struct ClaudeCodeExecutor {
    config: ClaudeCodeConfig,
}

impl ClaudeCodeExecutor {
    pub fn new(config: ClaudeCodeConfig) -> Self {
        Self { config }
    }

    /// Build the command-line arguments for `claude`.
    fn build_args(&self, prompt: &str, work_dir: &Path) -> Vec<String> {
        let mut args = vec![
            "--print".to_string(),
            "--verbose".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--no-session-persistence".to_string(),
        ];

        if self.config.skip_permissions {
            args.push("--dangerously-skip-permissions".to_string());
        }

        if let Some(ref model) = self.config.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }

        if let Some(max_turns) = self.config.max_turns {
            args.push("--max-turns".to_string());
            args.push(max_turns.to_string());
        }

        // Give claude access to the working directory
        args.push("--add-dir".to_string());
        args.push(work_dir.to_string_lossy().to_string());

        // The prompt is the final positional argument
        args.push(prompt.to_string());

        args
    }
}

/// Build a task prompt for Claude Code from task context.
fn build_task_prompt(
    task_title: &str,
    task_description: &str,
    acceptance_criteria: &[AcceptanceCriterion],
    prompt_ctx: &PromptContext<'_>,
) -> String {
    let mut prompt = String::new();

    if let Some(epic) = prompt_ctx.epic_name {
        prompt.push_str(&format!("## Epic: {epic}\n\n"));
    }

    if let Some(ctx) = prompt_ctx.project_context {
        prompt.push_str(&format!("## Project Context\n{ctx}\n\n"));
    }

    prompt.push_str(&format!("## Task: {task_title}\n\n{task_description}\n\n"));

    if !acceptance_criteria.is_empty() {
        prompt.push_str("## Acceptance Criteria\n");
        for (i, criterion) in acceptance_criteria.iter().enumerate() {
            prompt.push_str(&format!("{}. {}\n", i + 1, criterion.description));
        }
        prompt.push('\n');
    }

    prompt.push_str(
        "Complete this task. Verify each acceptance criterion is met before finishing. \
         Be thorough but concise.",
    );

    prompt
}

// ---------------------------------------------------------------------------
// JSONL stream parsing
// ---------------------------------------------------------------------------

/// A content block inside an assistant message.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Other,
}

/// An assistant message in the stream.
#[derive(Debug, Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

/// A user message in the stream (tool results).
#[derive(Debug, Deserialize)]
struct UserMessage {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

/// Top-level stream event from `claude --output-format stream-json`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum StreamEvent {
    #[serde(rename = "system")]
    System {},
    #[serde(rename = "assistant")]
    Assistant { message: AssistantMessage },
    #[serde(rename = "user")]
    User { message: UserMessage },
    #[serde(rename = "result")]
    Result {
        result: String,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        total_cost_usd: Option<f64>,
    },
}

/// Parsed output from a Claude Code invocation.
struct ClaudeCodeOutput {
    tool_calls: Vec<ToolCallRecord>,
    result_text: String,
    is_error: bool,
}

/// Parse JSONL output from `claude --output-format stream-json`.
fn parse_stream_output(output: &str) -> ClaudeCodeOutput {
    let mut tool_calls = Vec::new();
    let mut result_text = String::new();
    let mut is_error = false;
    let mut text_parts: Vec<String> = Vec::new();

    // Track pending tool_use calls to match with results
    let mut pending_tools: Vec<(String, String)> = Vec::new(); // (name, args_summary)

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let event: StreamEvent = match serde_json::from_str(trimmed) {
            Ok(e) => e,
            Err(_) => continue, // Skip non-JSON lines
        };

        match event {
            StreamEvent::Assistant { message } => {
                for block in message.content {
                    match block {
                        ContentBlock::Text { text } => {
                            text_parts.push(text);
                        }
                        ContentBlock::ToolUse { name, input } => {
                            let args_summary = summarize_json(&input, 120);
                            pending_tools.push((name, args_summary));
                        }
                        _ => {}
                    }
                }
            }
            StreamEvent::User { message } => {
                for block in message.content {
                    if let ContentBlock::ToolResult { content, is_error } = block {
                        let result_summary = summarize_json(&content, 120);
                        if let Some((name, args_summary)) = pending_tools.pop() {
                            tool_calls.push(ToolCallRecord {
                                tool_name: name,
                                args_summary,
                                result_summary: if is_error {
                                    format!("ERROR: {result_summary}")
                                } else {
                                    result_summary
                                },
                            });
                        }
                    }
                }
            }
            StreamEvent::Result {
                result,
                is_error: err,
                duration_ms,
                total_cost_usd,
            } => {
                result_text = result;
                is_error = err;
                if let Some(ms) = duration_ms {
                    debug!(duration_ms = ms, "Claude Code execution duration");
                }
                if let Some(cost) = total_cost_usd {
                    debug!(cost_usd = cost, "Claude Code execution cost");
                }
            }
            StreamEvent::System {} => {}
        }
    }

    // Flush any unmatched pending tools
    for (name, args_summary) in pending_tools {
        tool_calls.push(ToolCallRecord {
            tool_name: name,
            args_summary,
            result_summary: "(no result captured)".into(),
        });
    }

    // If no result event, use concatenated text
    if result_text.is_empty() && !text_parts.is_empty() {
        result_text = text_parts.join("\n");
    }

    ClaudeCodeOutput {
        tool_calls,
        result_text,
        is_error,
    }
}

/// Summarize a JSON value to a bounded string for logging.
fn summarize_json(value: &serde_json::Value, max_len: usize) -> String {
    let s = match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if s.len() <= max_len {
        s
    } else {
        format!("{}…", &s[..max_len])
    }
}

#[async_trait::async_trait]
impl TaskExecutor for ClaudeCodeExecutor {
    async fn execute_task(
        &self,
        task_title: &str,
        task_description: &str,
        acceptance_criteria: &[AcceptanceCriterion],
        work_dir: PathBuf,
        timeout_secs: u64,
        prompt_ctx: &PromptContext<'_>,
    ) -> Result<(Vec<ToolCallRecord>, String), RewindError> {
        let prompt = build_task_prompt(task_title, task_description, acceptance_criteria, prompt_ctx);
        let args = self.build_args(&prompt, &work_dir);

        info!(
            task = task_title,
            work_dir = %work_dir.display(),
            "Spawning Claude Code CLI"
        );
        debug!(args = ?args, "Claude Code arguments");

        let child = tokio::process::Command::new("claude")
            .args(&args)
            .current_dir(&work_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| {
                RewindError::Config(format!(
                    "Failed to spawn 'claude' CLI. Is Claude Code installed? Error: {e}"
                ))
            })?;

        // Apply timeout
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| {
            // Kill the child on timeout
            RewindError::Config(format!(
                "Claude Code timed out after {timeout_secs}s on task: {task_title}"
            ))
        })?
        .map_err(|e| RewindError::Config(format!("Claude Code process error: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !stderr.is_empty() {
            debug!(stderr = %stderr, "Claude Code stderr");
        }

        if !output.status.success() {
            warn!(
                exit_code = output.status.code(),
                stderr = %stderr,
                "Claude Code exited with non-zero status"
            );
        }

        let parsed = parse_stream_output(&stdout);

        if parsed.is_error {
            return Err(RewindError::Config(format!(
                "Claude Code reported error: {}",
                parsed.result_text
            )));
        }

        info!(
            task = task_title,
            tool_calls = parsed.tool_calls.len(),
            result_len = parsed.result_text.len(),
            "Claude Code execution complete"
        );

        Ok((parsed.tool_calls, parsed.result_text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_minimal() {
        let executor = ClaudeCodeExecutor::new(ClaudeCodeConfig::default());
        let args = executor.build_args("do stuff", &PathBuf::from("/tmp/work"));

        assert!(args.contains(&"--print".to_string()));
        assert!(args.contains(&"--verbose".to_string()));
        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.contains(&"--no-session-persistence".to_string()));
        assert!(args.contains(&"--add-dir".to_string()));
        assert!(args.contains(&"/tmp/work".to_string()));
        assert_eq!(args.last().unwrap(), "do stuff");
    }

    #[test]
    fn build_args_with_model_and_max_turns() {
        let executor = ClaudeCodeExecutor::new(ClaudeCodeConfig {
            model: Some("claude-opus-4-6".into()),
            max_turns: Some(5),
            skip_permissions: false,
        });
        let args = executor.build_args("task", &PathBuf::from("/work"));

        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"claude-opus-4-6".to_string()));
        assert!(args.contains(&"--max-turns".to_string()));
        assert!(args.contains(&"5".to_string()));
        assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn parse_stream_result_event() {
        let jsonl = r#"{"type":"result","subtype":"success","is_error":false,"result":"Task completed successfully","duration_ms":5000,"total_cost_usd":0.05,"session_id":"test"}"#;
        let output = parse_stream_output(jsonl);

        assert!(!output.is_error);
        assert_eq!(output.result_text, "Task completed successfully");
        assert!(output.tool_calls.is_empty());
    }

    #[test]
    fn parse_stream_with_tool_calls() {
        let jsonl = concat!(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"src/main.rs"}}]},"session_id":"s1"}"#,
            "\n",
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"fn main() {}","is_error":false}]},"session_id":"s1"}"#,
            "\n",
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Done","duration_ms":1000,"session_id":"s1"}"#,
        );
        let output = parse_stream_output(jsonl);

        assert_eq!(output.tool_calls.len(), 1);
        assert_eq!(output.tool_calls[0].tool_name, "Read");
        assert!(output.tool_calls[0].args_summary.contains("src/main.rs"));
        assert_eq!(output.tool_calls[0].result_summary, "fn main() {}");
        assert_eq!(output.result_text, "Done");
    }

    #[test]
    fn parse_stream_error_result() {
        let jsonl = r#"{"type":"result","subtype":"error","is_error":true,"result":"Something went wrong","session_id":"s1"}"#;
        let output = parse_stream_output(jsonl);

        assert!(output.is_error);
        assert_eq!(output.result_text, "Something went wrong");
    }

    #[test]
    fn parse_stream_skips_invalid_lines() {
        let jsonl = "not json\n{\"type\":\"result\",\"is_error\":false,\"result\":\"ok\"}\ngarbage";
        let output = parse_stream_output(jsonl);

        assert_eq!(output.result_text, "ok");
    }

    #[test]
    fn build_task_prompt_includes_all_sections() {
        let criteria = vec![
            AcceptanceCriterion {
                description: "File exists".into(),
                checked: false,
            },
            AcceptanceCriterion {
                description: "Tests pass".into(),
                checked: false,
            },
        ];
        let ctx = PromptContext {
            epic_name: Some("My Epic"),
            project_context: Some("Rust project"),
            ..Default::default()
        };

        let prompt = build_task_prompt("Add feature", "Do the thing", &criteria, &ctx);

        assert!(prompt.contains("My Epic"));
        assert!(prompt.contains("Rust project"));
        assert!(prompt.contains("Add feature"));
        assert!(prompt.contains("Do the thing"));
        assert!(prompt.contains("File exists"));
        assert!(prompt.contains("Tests pass"));
    }

    #[test]
    fn summarize_json_truncates_long_values() {
        let long = serde_json::Value::String("x".repeat(200));
        let summary = summarize_json(&long, 50);
        assert!(summary.len() <= 53); // 50 chars + "…" (3 bytes UTF-8)
    }
}
