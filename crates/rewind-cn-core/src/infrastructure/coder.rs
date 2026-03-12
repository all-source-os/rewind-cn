use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rig::completion::{Prompt, ToolDefinition};
use rig::prelude::CompletionClient;
use rig::tool::Tool;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::domain::error::RewindError;
use crate::domain::events::AcceptanceCriterion;
use crate::infrastructure::llm::{AgentConfig, ProviderClient};
use crate::infrastructure::prompt_template::render_prompt;

/// A recorded tool call for audit trail.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub args_summary: String,
    pub result_summary: String,
}

/// Shared log for recording tool calls during agent execution.
pub type ToolCallLog = Arc<Mutex<Vec<ToolCallRecord>>>;

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/// Read a file from the filesystem.
pub struct ReadFileTool {
    work_dir: PathBuf,
    log: ToolCallLog,
}

#[derive(Deserialize)]
pub struct ReadFileArgs {
    /// File path relative to the project root.
    path: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ToolError(pub String);

impl ReadFileTool {
    pub fn new(work_dir: PathBuf, log: ToolCallLog) -> Self {
        Self { work_dir, log }
    }
}

impl Tool for ReadFileTool {
    const NAME: &'static str = "read_file";
    type Error = ToolError;
    type Args = ReadFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".into(),
            description: "Read the contents of a file. Returns the file content as a string."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the project root"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    #[hotpath::measure]
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let full_path = self.work_dir.join(&args.path);
        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| ToolError(format!("Failed to read {}: {e}", args.path)))?;

        let summary = format!("{} bytes", content.len());
        self.log.lock().await.push(ToolCallRecord {
            tool_name: "read_file".into(),
            args_summary: args.path,
            result_summary: summary,
        });

        Ok(content)
    }
}

/// Write content to a file.
pub struct WriteFileTool {
    work_dir: PathBuf,
    log: ToolCallLog,
}

#[derive(Deserialize)]
pub struct WriteFileArgs {
    /// File path relative to the project root.
    path: String,
    /// Content to write.
    content: String,
}

impl WriteFileTool {
    pub fn new(work_dir: PathBuf, log: ToolCallLog) -> Self {
        Self { work_dir, log }
    }
}

impl Tool for WriteFileTool {
    const NAME: &'static str = "write_file";
    type Error = ToolError;
    type Args = WriteFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".into(),
            description: "Write content to a file. Creates parent directories if needed. Overwrites existing content.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path relative to the project root"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    #[hotpath::measure]
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let full_path = self.work_dir.join(&args.path);
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError(format!("Failed to create dirs: {e}")))?;
        }
        let bytes = args.content.len();
        tokio::fs::write(&full_path, &args.content)
            .await
            .map_err(|e| ToolError(format!("Failed to write {}: {e}", args.path)))?;

        let result = format!("Wrote {} bytes to {}", bytes, args.path);
        self.log.lock().await.push(ToolCallRecord {
            tool_name: "write_file".into(),
            args_summary: args.path,
            result_summary: result.clone(),
        });

        Ok(result)
    }
}

/// List files in a directory.
pub struct ListFilesTool {
    work_dir: PathBuf,
    log: ToolCallLog,
}

#[derive(Deserialize)]
pub struct ListFilesArgs {
    /// Directory path relative to the project root. Defaults to "." if empty.
    #[serde(default)]
    path: String,
    /// Optional glob pattern to filter files (e.g., "*.rs").
    #[serde(default)]
    pattern: Option<String>,
}

impl ListFilesTool {
    pub fn new(work_dir: PathBuf, log: ToolCallLog) -> Self {
        Self { work_dir, log }
    }
}

impl Tool for ListFilesTool {
    const NAME: &'static str = "list_files";
    type Error = ToolError;
    type Args = ListFilesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "list_files".into(),
            description: "List files in a directory. Returns one path per line.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path relative to project root (default: \".\")"
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Optional glob pattern to filter files (e.g., \"*.rs\")"
                    }
                }
            }),
        }
    }

    #[hotpath::measure]
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let dir = if args.path.is_empty() {
            self.work_dir.clone()
        } else {
            self.work_dir.join(&args.path)
        };

        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| ToolError(format!("Failed to list {}: {e}", dir.display())))?;

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| ToolError(format!("Failed to read entry: {e}")))?
        {
            let name = entry.file_name().to_string_lossy().to_string();

            // Simple glob matching if pattern provided
            if let Some(ref pat) = args.pattern {
                if !simple_glob_match(pat, &name) {
                    continue;
                }
            }

            let file_type = entry
                .file_type()
                .await
                .map_err(|e| ToolError(format!("Failed to get file type: {e}")))?;
            let suffix = if file_type.is_dir() { "/" } else { "" };
            entries.push(format!("{name}{suffix}"));
        }

        entries.sort();
        let result = entries.join("\n");
        let summary = format!("{} entries", entries.len());

        self.log.lock().await.push(ToolCallRecord {
            tool_name: "list_files".into(),
            args_summary: format!("{} (pattern: {:?})", args.path, args.pattern),
            result_summary: summary,
        });

        Ok(result)
    }
}

/// Search for a pattern in files using grep-like functionality.
pub struct SearchCodeTool {
    work_dir: PathBuf,
    log: ToolCallLog,
}

#[derive(Deserialize)]
pub struct SearchCodeArgs {
    /// The text pattern to search for.
    pattern: String,
    /// Optional file path or directory to search in (default: project root).
    #[serde(default)]
    path: Option<String>,
    /// Optional file extension filter (e.g., "rs", "ts").
    #[serde(default)]
    file_ext: Option<String>,
}

impl SearchCodeTool {
    pub fn new(work_dir: PathBuf, log: ToolCallLog) -> Self {
        Self { work_dir, log }
    }
}

impl Tool for SearchCodeTool {
    const NAME: &'static str = "search_code";
    type Error = ToolError;
    type Args = SearchCodeArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "search_code".into(),
            description:
                "Search for a text pattern in files. Returns matching lines with file:line:content."
                    .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The text pattern to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "File or directory to search in (default: project root)"
                    },
                    "file_ext": {
                        "type": "string",
                        "description": "File extension filter (e.g., \"rs\", \"ts\")"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    #[hotpath::measure]
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let search_path = match &args.path {
            Some(p) => self.work_dir.join(p),
            None => self.work_dir.clone(),
        };

        // Use grep/rg if available, fall back to basic search
        let mut cmd = tokio::process::Command::new("grep");
        cmd.args(["-rn", &args.pattern])
            .arg(&search_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(ref ext) = args.file_ext {
            cmd.args(["--include", &format!("*.{ext}")]);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| ToolError(format!("Failed to run grep: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Strip the work_dir prefix for cleaner output
        let prefix = self.work_dir.to_string_lossy();
        let result: String = stdout
            .lines()
            .take(100) // Limit output
            .map(|line| line.strip_prefix(prefix.as_ref()).unwrap_or(line))
            .map(|line| line.strip_prefix('/').unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");

        let match_count = result.lines().count();
        let summary = format!("{} matches for '{}'", match_count, args.pattern);

        self.log.lock().await.push(ToolCallRecord {
            tool_name: "search_code".into(),
            args_summary: format!("{} in {:?}", args.pattern, args.path),
            result_summary: summary,
        });

        Ok(if result.is_empty() {
            "No matches found.".into()
        } else {
            result
        })
    }
}

/// Run a shell command.
pub struct RunCommandTool {
    work_dir: PathBuf,
    timeout_secs: u64,
    log: ToolCallLog,
}

#[derive(Deserialize)]
pub struct RunCommandArgs {
    /// The shell command to execute.
    command: String,
}

impl RunCommandTool {
    pub fn new(work_dir: PathBuf, timeout_secs: u64, log: ToolCallLog) -> Self {
        Self {
            work_dir,
            timeout_secs,
            log,
        }
    }
}

impl Tool for RunCommandTool {
    const NAME: &'static str = "run_command";
    type Error = ToolError;
    type Args = RunCommandArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "run_command".into(),
            description: format!(
                "Run a shell command in the project directory. Timeout: {}s. Returns stdout + stderr.",
                self.timeout_secs
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute (e.g., \"cargo test\", \"ls -la\")"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    #[hotpath::measure]
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let child = tokio::process::Command::new("sh")
            .args(["-c", &args.command])
            .current_dir(&self.work_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError(format!("Failed to spawn command: {e}")))?;

        let timeout = std::time::Duration::from_secs(self.timeout_secs);
        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| ToolError(format!("Command timed out after {}s", self.timeout_secs)))?
            .map_err(|e| ToolError(format!("Command failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        let result =
            format!("exit_code: {exit_code}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}");

        // Truncate result for summary
        let summary = if output.status.success() {
            format!("exit 0, {} bytes output", stdout.len() + stderr.len())
        } else {
            format!("exit {exit_code}")
        };

        self.log.lock().await.push(ToolCallRecord {
            tool_name: "run_command".into(),
            args_summary: args.command,
            result_summary: summary,
        });

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// CoderAgent
// ---------------------------------------------------------------------------

/// Format acceptance criteria as a checkbox list.
fn format_acceptance_criteria(criteria: &[AcceptanceCriterion]) -> String {
    criteria
        .iter()
        .map(|c| {
            let check = if c.checked { "x" } else { " " };
            format!("- [{}] {}", check, c.description)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Builds the system prompt for the coder agent using the Tera template engine.
///
/// If `template_path` points to an existing file, that template is used;
/// otherwise the embedded default template is rendered.
fn build_coder_prompt(
    task_title: &str,
    task_description: &str,
    acceptance_criteria: &[AcceptanceCriterion],
    epic_name: Option<&str>,
    project_context: Option<&str>,
    template_path: Option<&Path>,
) -> Result<String, RewindError> {
    let mut context_map = HashMap::new();
    context_map.insert("task_title".to_string(), task_title.to_string());
    context_map.insert("task_description".to_string(), task_description.to_string());
    context_map.insert(
        "acceptance_criteria".to_string(),
        format_acceptance_criteria(acceptance_criteria),
    );
    if let Some(epic) = epic_name {
        context_map.insert("epic".to_string(), epic.to_string());
    }
    if let Some(ctx) = project_context {
        context_map.insert("project_context".to_string(), ctx.to_string());
    }

    let default_path = PathBuf::from("/nonexistent/default_prompt.tera");
    let path = template_path.unwrap_or(&default_path);
    render_prompt(path, &context_map)
}

/// Optional prompt context for the coder agent.
#[derive(Debug, Default)]
pub struct PromptContext<'a> {
    pub epic_name: Option<&'a str>,
    pub project_context: Option<&'a str>,
    pub template_path: Option<&'a Path>,
}

/// LLM-powered coder agent using rig-core with tool-use.
pub struct CoderAgent {
    client: ProviderClient,
    config: AgentConfig,
}

impl CoderAgent {
    pub fn new(client: ProviderClient, config: AgentConfig) -> Self {
        Self { client, config }
    }

    /// Execute a task using the coder agent with tool-use loop.
    ///
    /// Returns the tool call log and the agent's final response.
    #[hotpath::measure]
    pub async fn execute_task(
        &self,
        task_title: &str,
        task_description: &str,
        acceptance_criteria: &[AcceptanceCriterion],
        work_dir: PathBuf,
        timeout_secs: u64,
        prompt_ctx: &PromptContext<'_>,
    ) -> Result<(Vec<ToolCallRecord>, String), RewindError> {
        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));

        let system_prompt = build_coder_prompt(
            task_title,
            task_description,
            acceptance_criteria,
            prompt_ctx.epic_name,
            prompt_ctx.project_context,
            prompt_ctx.template_path,
        )?;
        let prompt_input = "Begin working on the task. Read relevant files, implement changes, and verify each acceptance criterion.";

        let response: String = match &self.client {
            ProviderClient::Anthropic(c) => {
                let agent = c
                    .agent(&self.config.coder.model)
                    .preamble(&system_prompt)
                    .max_tokens(self.config.coder.max_tokens as u64)
                    .tool(ReadFileTool::new(work_dir.clone(), log.clone()))
                    .tool(WriteFileTool::new(work_dir.clone(), log.clone()))
                    .tool(ListFilesTool::new(work_dir.clone(), log.clone()))
                    .tool(SearchCodeTool::new(work_dir.clone(), log.clone()))
                    .tool(RunCommandTool::new(work_dir, timeout_secs, log.clone()))
                    .build();
                agent
                    .prompt(prompt_input)
                    .max_turns(20)
                    .await
                    .map_err(|e| RewindError::Config(format!("Coder agent failed: {e}")))?
            }
            ProviderClient::OpenAI(c) => {
                let agent = c
                    .agent(&self.config.coder.model)
                    .preamble(&system_prompt)
                    .max_tokens(self.config.coder.max_tokens as u64)
                    .tool(ReadFileTool::new(work_dir.clone(), log.clone()))
                    .tool(WriteFileTool::new(work_dir.clone(), log.clone()))
                    .tool(ListFilesTool::new(work_dir.clone(), log.clone()))
                    .tool(SearchCodeTool::new(work_dir.clone(), log.clone()))
                    .tool(RunCommandTool::new(work_dir, timeout_secs, log.clone()))
                    .build();
                agent
                    .prompt(prompt_input)
                    .max_turns(20)
                    .await
                    .map_err(|e| RewindError::Config(format!("Coder agent failed: {e}")))?
            }
        };

        let records = log.lock().await.clone();
        Ok((records, response))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Simple glob match supporting only `*` wildcard prefix/suffix.
fn simple_glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    pattern == name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_glob_match_works() {
        assert!(simple_glob_match("*.rs", "main.rs"));
        assert!(simple_glob_match("*.rs", "lib.rs"));
        assert!(!simple_glob_match("*.rs", "main.ts"));
        assert!(simple_glob_match("src*", "src"));
        assert!(simple_glob_match("src*", "src_test"));
        assert!(simple_glob_match("*", "anything"));
        assert!(simple_glob_match("exact.txt", "exact.txt"));
        assert!(!simple_glob_match("exact.txt", "other.txt"));
    }

    #[test]
    fn build_coder_prompt_includes_criteria() {
        let criteria = vec![
            AcceptanceCriterion {
                description: "File exists".into(),
                checked: false,
            },
            AcceptanceCriterion {
                description: "Tests pass".into(),
                checked: true,
            },
        ];

        let prompt =
            build_coder_prompt("My Task", "Do the thing", &criteria, None, None, None).unwrap();
        assert!(prompt.contains("My Task"));
        assert!(prompt.contains("Do the thing"));
        assert!(prompt.contains("- [ ] File exists"));
        assert!(prompt.contains("- [x] Tests pass"));
    }

    #[test]
    fn build_coder_prompt_includes_epic_and_project_context() {
        let criteria = vec![AcceptanceCriterion {
            description: "It works".into(),
            checked: false,
        }];

        let prompt = build_coder_prompt(
            "My Task",
            "Do the thing",
            &criteria,
            Some("Epic-42: Platform Overhaul"),
            Some("Rust CQRS service"),
            None,
        )
        .unwrap();
        assert!(prompt.contains("Epic-42: Platform Overhaul"));
        assert!(prompt.contains("Rust CQRS service"));
    }

    #[test]
    fn build_coder_prompt_with_custom_template() {
        let dir = tempfile::tempdir().unwrap();
        let tpl_path = dir.path().join("custom.tera");
        std::fs::write(&tpl_path, "Custom: {{ task_title }} - {{ epic }}").unwrap();

        let criteria = vec![];
        let prompt = build_coder_prompt(
            "My Task",
            "desc",
            &criteria,
            Some("Epic-1"),
            None,
            Some(tpl_path.as_path()),
        )
        .unwrap();
        assert_eq!(prompt, "Custom: My Task - Epic-1");
    }

    #[tokio::test]
    async fn tool_call_log_records_entries() {
        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));

        log.lock().await.push(ToolCallRecord {
            tool_name: "read_file".into(),
            args_summary: "src/main.rs".into(),
            result_summary: "1024 bytes".into(),
        });

        log.lock().await.push(ToolCallRecord {
            tool_name: "run_command".into(),
            args_summary: "cargo test".into(),
            result_summary: "exit 0, 500 bytes output".into(),
        });

        let records = log.lock().await;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].tool_name, "read_file");
        assert_eq!(records[1].tool_name, "run_command");
    }

    #[tokio::test]
    async fn read_file_tool_reads_existing_file() {
        let dir = std::env::temp_dir().join("rewind-test-read");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello world")
            .await
            .unwrap();

        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));
        let tool = ReadFileTool::new(dir.clone(), log.clone());

        let result = tool
            .call(ReadFileArgs {
                path: "test.txt".into(),
            })
            .await
            .unwrap();

        assert_eq!(result, "hello world");
        assert_eq!(log.lock().await.len(), 1);
        assert_eq!(log.lock().await[0].tool_name, "read_file");

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn write_file_tool_creates_and_writes() {
        let dir = std::env::temp_dir().join("rewind-test-write");
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));
        let tool = WriteFileTool::new(dir.clone(), log.clone());

        let result = tool
            .call(WriteFileArgs {
                path: "subdir/out.txt".into(),
                content: "written content".into(),
            })
            .await
            .unwrap();

        assert!(result.contains("Wrote 15 bytes"));
        let content = tokio::fs::read_to_string(dir.join("subdir/out.txt"))
            .await
            .unwrap();
        assert_eq!(content, "written content");

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn run_command_tool_executes() {
        let dir = std::env::temp_dir().join("rewind-test-cmd");
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));
        let tool = RunCommandTool::new(dir.clone(), 10, log.clone());

        let result = tool
            .call(RunCommandArgs {
                command: "echo hello".into(),
            })
            .await
            .unwrap();

        assert!(result.contains("exit_code: 0"));
        assert!(result.contains("hello"));
        assert_eq!(log.lock().await[0].tool_name, "run_command");

        tokio::fs::remove_dir_all(&dir).await.ok();
    }
}
