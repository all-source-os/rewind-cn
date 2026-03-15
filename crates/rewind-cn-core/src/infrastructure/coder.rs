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
use crate::infrastructure::sanitize::sanitize_user_content;

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
        let full_path = validate_path(&self.work_dir, &args.path)?;
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
        let full_path = validate_path_for_write(&self.work_dir, &args.path)?;
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
            self.work_dir
                .canonicalize()
                .map_err(|e| ToolError(format!("Failed to canonicalize work_dir: {e}")))?
        } else {
            validate_path(&self.work_dir, &args.path)?
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
            Some(p) => validate_path(&self.work_dir, p)?,
            None => self
                .work_dir
                .canonicalize()
                .map_err(|e| ToolError(format!("Failed to canonicalize work_dir: {e}")))?,
        };

        // Use grep with -F (fixed string) to prevent regex injection,
        // and -- to prevent pattern being interpreted as flags.
        // We use find + grep to avoid following symlinks: find's default
        // behavior (-P / no -L) does not follow symlinks, ensuring we only
        // search real files within the work directory.
        let mut cmd = tokio::process::Command::new("find");
        let mut find_args: Vec<std::ffi::OsString> =
            vec![search_path.as_os_str().to_owned(), "-type".into(), "f".into()];

        if let Some(ref ext) = args.file_ext {
            if !ext.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(ToolError(format!("Invalid file extension: {ext}")));
            }
            find_args.extend(["-name".into(), format!("*.{ext}").into()]);
        }

        find_args.extend([
            "-exec".into(),
            "grep".into(),
            "-nF".into(),
            "--".into(),
            args.pattern.clone().into(),
            "{}".into(),
            "+".into(),
        ]);

        cmd.args(&find_args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let output = cmd
            .output()
            .await
            .map_err(|e| ToolError(format!("Failed to run find+grep: {e}")))?;

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
        // Validate command against allowlist to prevent injection
        validate_command(&args.command)?;

        let parts = shell_words::split(&args.command)
            .map_err(|e| ToolError(format!("Failed to parse command: {e}")))?;
        if parts.is_empty() {
            return Err(ToolError("Empty command".into()));
        }
        let child = tokio::process::Command::new(&parts[0])
            .args(&parts[1..])
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
    prompt_ctx: &PromptContext<'_>,
) -> Result<String, RewindError> {
    let mut context_map = HashMap::new();
    context_map.insert("task_title".to_string(), sanitize_user_content(task_title));
    context_map.insert(
        "task_description".to_string(),
        sanitize_user_content(task_description),
    );
    context_map.insert(
        "acceptance_criteria".to_string(),
        sanitize_user_content(&format_acceptance_criteria(acceptance_criteria)),
    );
    if let Some(epic) = prompt_ctx.epic_name {
        context_map.insert("epic".to_string(), epic.to_string());
    }
    if let Some(ctx) = prompt_ctx.project_context {
        context_map.insert("project_context".to_string(), ctx.to_string());
    }
    if let Some(progress) = prompt_ctx.progress {
        context_map.insert("progress".to_string(), progress.to_string());
    }

    let default_path = PathBuf::from("/nonexistent/default_prompt.tera");
    let path = prompt_ctx.template_path.unwrap_or(&default_path);
    render_prompt(path, &context_map)
}

/// Optional prompt context for the coder agent.
#[derive(Debug, Default)]
pub struct PromptContext<'a> {
    pub epic_name: Option<&'a str>,
    pub project_context: Option<&'a str>,
    pub progress: Option<&'a str>,
    pub template_path: Option<&'a Path>,
}

/// Trait abstracting the coder agent for testability.
///
/// The orchestrator depends on this trait rather than on the concrete
/// `CoderAgent`, allowing tests to inject a mock implementation that
/// doesn't require a real LLM or the rig agent framework.
#[async_trait::async_trait]
pub trait TaskExecutor: Send + Sync {
    async fn execute_task(
        &self,
        task_title: &str,
        task_description: &str,
        acceptance_criteria: &[AcceptanceCriterion],
        work_dir: PathBuf,
        timeout_secs: u64,
        prompt_ctx: &PromptContext<'_>,
    ) -> Result<(Vec<ToolCallRecord>, String), RewindError>;
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
}

#[async_trait::async_trait]
impl TaskExecutor for CoderAgent {
    #[hotpath::measure]
    async fn execute_task(
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
            prompt_ctx,
        )?;
        let prompt_input = "Begin working on the task. Read relevant files, implement changes, and verify each acceptance criterion.";

        // Macro to avoid duplicating tool setup across provider variants.
        // Both Anthropic and OpenAI use the same agent builder API but return
        // different concrete types, so we can't abstract with a trait here.
        macro_rules! build_and_run {
            ($client:expr) => {{
                let agent = $client
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
            }};
        }

        let response: String = match &self.client {
            ProviderClient::Anthropic(c) => build_and_run!(c),
            ProviderClient::OpenAI(c) => build_and_run!(c),
            #[cfg(test)]
            ProviderClient::Mock(f) => {
                // In test mode, skip the rig agent framework entirely.
                // Call the mock function with coder model as the "model" param.
                f(
                    &self.config.coder.model,
                    &system_prompt,
                    self.config.coder.max_tokens as u64,
                    prompt_input,
                )
            }
        };

        let records = log.lock().await.clone();
        Ok((records, response))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate that a requested path stays within the work directory.
///
/// Joins `work_dir` with `requested`, canonicalizes both, and checks the
/// result is a descendant of the canonical `work_dir`. Returns the validated
/// canonical path or an error if the path escapes the sandbox.
///
/// **Known limitation (TOCTOU):** There is a time-of-check-time-of-use window
/// between this validation and the subsequent file operation. A symlink created
/// after canonicalization but before the read/write could escape the sandbox.
/// This risk is mitigated by the agent running in a controlled environment
/// (dedicated worktree) where no concurrent actor is manipulating the filesystem.
fn validate_path(work_dir: &Path, requested: &str) -> Result<PathBuf, ToolError> {
    let canonical_work_dir = work_dir.canonicalize().map_err(|e| {
        ToolError(format!(
            "Failed to canonicalize work_dir {}: {e}",
            work_dir.display()
        ))
    })?;

    let requested_path = Path::new(requested);

    // If the requested path is absolute, check it directly instead of joining
    let joined = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        work_dir.join(requested)
    };

    let canonical = joined.canonicalize().map_err(|e| {
        ToolError(format!(
            "Failed to canonicalize path {}: {e}",
            joined.display()
        ))
    })?;

    if !canonical.starts_with(&canonical_work_dir) {
        return Err(ToolError(format!(
            "Path traversal denied: {} escapes work directory {}",
            requested,
            work_dir.display()
        )));
    }

    Ok(canonical)
}

/// Validate a path for write operations where the file may not exist yet.
///
/// Similar to `validate_path` but canonicalizes the parent directory instead,
/// since the target file may not exist yet.
fn validate_path_for_write(work_dir: &Path, requested: &str) -> Result<PathBuf, ToolError> {
    let canonical_work_dir = work_dir.canonicalize().map_err(|e| {
        ToolError(format!(
            "Failed to canonicalize work_dir {}: {e}",
            work_dir.display()
        ))
    })?;

    let requested_path = Path::new(requested);

    let joined = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        work_dir.join(requested)
    };

    // For writes, the file may not exist yet, so canonicalize the parent
    let parent = joined.parent().ok_or_else(|| {
        ToolError(format!(
            "Path has no parent directory: {}",
            joined.display()
        ))
    })?;

    // The parent must exist (or we need to check what we can resolve)
    // Walk up until we find an existing ancestor, canonicalize that,
    // then re-append the remaining components
    let mut existing = parent.to_path_buf();
    let mut remaining = Vec::new();
    while !existing.exists() {
        if let Some(file_name) = existing.file_name() {
            remaining.push(file_name.to_owned());
            existing = existing
                .parent()
                .ok_or_else(|| ToolError(format!("Cannot resolve path: {}", joined.display())))?
                .to_path_buf();
        } else {
            return Err(ToolError(format!(
                "Cannot resolve path: {}",
                joined.display()
            )));
        }
    }

    let mut canonical_parent = existing.canonicalize().map_err(|e| {
        ToolError(format!(
            "Failed to canonicalize {}: {e}",
            existing.display()
        ))
    })?;

    // Re-append the non-existing components
    for component in remaining.into_iter().rev() {
        canonical_parent.push(component);
    }

    if !canonical_parent.starts_with(&canonical_work_dir) {
        return Err(ToolError(format!(
            "Path traversal denied: {} escapes work directory {}",
            requested,
            work_dir.display()
        )));
    }

    // Return the full path (parent + filename)
    if let Some(file_name) = joined.file_name() {
        Ok(canonical_parent.join(file_name))
    } else {
        Ok(canonical_parent)
    }
}

/// Allowed command prefixes for RunCommandTool.
const ALLOWED_COMMANDS: &[&str] = &[
    "cargo", "rustfmt", "rustc", "make", "git", "ls", "cat", "head", "tail", "grep", "rg", "find",
    "wc", "sort", "uniq", "diff", "echo", "pwd", "env", "mkdir", "cp", "mv", "touch", "rm", "tree",
    "which", "test",
];

/// Shell metacharacters that indicate injection attempts.
const DANGEROUS_CHARS: &[char] = &['|', ';', '&', '`', '$', '(', ')', '{', '}', '<', '>'];

/// Validate a command string against the allowlist and reject shell metacharacters.
fn validate_command(command: &str) -> Result<(), ToolError> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err(ToolError("Empty command".into()));
    }

    // Check for shell metacharacters
    if trimmed.chars().any(|c| DANGEROUS_CHARS.contains(&c)) {
        return Err(ToolError(format!(
            "Command contains disallowed shell metacharacters: {trimmed}"
        )));
    }

    let program = trimmed.split_whitespace().next().unwrap_or("");
    // Extract basename to prevent path-based bypass (e.g., /bin/sh)
    let basename = std::path::Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    if !ALLOWED_COMMANDS.contains(&basename) {
        return Err(ToolError(format!(
            "Command not allowed: {program}. Allowed: {}",
            ALLOWED_COMMANDS.join(", ")
        )));
    }

    Ok(())
}

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

        let prompt = build_coder_prompt(
            "My Task",
            "Do the thing",
            &criteria,
            &PromptContext::default(),
        )
        .unwrap();
        // User content is now wrapped in <user-task> delimiters
        assert!(prompt.contains("<user-task>"));
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

        let ctx = PromptContext {
            epic_name: Some("Epic-42: Platform Overhaul"),
            project_context: Some("Rust CQRS service"),
            ..Default::default()
        };
        let prompt = build_coder_prompt("My Task", "Do the thing", &criteria, &ctx).unwrap();
        assert!(prompt.contains("Epic-42: Platform Overhaul"));
        assert!(prompt.contains("Rust CQRS service"));
    }

    #[test]
    fn build_coder_prompt_with_custom_template() {
        let dir = tempfile::tempdir().unwrap();
        let tpl_path = dir.path().join("custom.tera");
        std::fs::write(&tpl_path, "Custom: {{ task_title }} - {{ epic }}").unwrap();

        let criteria = vec![];
        let ctx = PromptContext {
            epic_name: Some("Epic-1"),
            template_path: Some(tpl_path.as_path()),
            ..Default::default()
        };
        let prompt = build_coder_prompt("My Task", "desc", &criteria, &ctx).unwrap();
        // task_title is sanitized with <user-task> delimiters; epic is not user content
        assert!(prompt.contains("My Task"));
        assert!(prompt.contains("Epic-1"));
        assert!(prompt.starts_with("Custom: "));
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

    #[test]
    fn validate_command_allows_safe_commands() {
        assert!(validate_command("cargo test").is_ok());
        assert!(validate_command("cargo clippy --all-targets").is_ok());
        assert!(validate_command("ls -la").is_ok());
        assert!(validate_command("grep -rn pattern .").is_ok());
        assert!(validate_command("git status").is_ok());
    }

    #[test]
    fn validate_command_rejects_disallowed_commands() {
        assert!(validate_command("curl http://evil.com").is_err());
        assert!(validate_command("wget http://evil.com").is_err());
        assert!(validate_command("python -c 'import os'").is_err());
    }

    #[test]
    fn validate_command_rejects_shell_metacharacters() {
        assert!(validate_command("cargo test; rm -rf /").is_err());
        assert!(validate_command("echo hello | cat").is_err());
        assert!(validate_command("echo $(whoami)").is_err());
        assert!(validate_command("echo `whoami`").is_err());
        assert!(validate_command("cargo test && curl evil").is_err());
    }

    #[test]
    fn validate_command_rejects_path_based_invocation() {
        assert!(validate_command("/bin/sh -c whoami").is_err());
        assert!(validate_command("/usr/bin/python -c 'import os'").is_err());
        assert!(validate_command("/bin/bash --norc").is_err());
        assert!(validate_command("/usr/bin/curl http://evil.com").is_err());
    }

    #[test]
    fn validate_command_allows_path_to_allowed_command() {
        // Path-based invocation of allowed commands should work
        assert!(validate_command("/usr/bin/git status").is_ok());
        assert!(validate_command("/usr/bin/ls -la").is_ok());
    }

    #[test]
    fn shell_words_split_handles_quoted_args() {
        // Verify shell_words correctly splits commands with quoted arguments
        let parts = shell_words::split(r#"cargo test --test "my test with spaces""#).unwrap();
        assert_eq!(parts, vec!["cargo", "test", "--test", "my test with spaces"]);

        let parts = shell_words::split("echo 'hello world'").unwrap();
        assert_eq!(parts, vec!["echo", "hello world"]);

        let parts = shell_words::split("grep -rn \"fn main\" src/").unwrap();
        assert_eq!(parts, vec!["grep", "-rn", "fn main", "src/"]);
    }

    #[test]
    fn validate_command_rejects_empty() {
        assert!(validate_command("").is_err());
        assert!(validate_command("   ").is_err());
    }

    #[tokio::test]
    async fn search_code_handles_metacharacters_safely() {
        let dir = std::env::temp_dir().join("rewind-test-search-injection");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.rs"), "let x = $(whoami); fn main() {}")
            .await
            .unwrap();

        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));
        let tool = SearchCodeTool::new(dir.clone(), log);

        // Pattern with shell metacharacters should be treated as literal text (via -F flag)
        let result = tool
            .call(SearchCodeArgs {
                pattern: "$(whoami)".into(),
                path: None,
                file_ext: Some("rs".into()),
            })
            .await
            .unwrap();

        assert!(result.contains("$(whoami)"));
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn search_code_rejects_invalid_file_ext() {
        let dir = std::env::temp_dir().join("rewind-test-search-ext");
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));
        let tool = SearchCodeTool::new(dir.clone(), log);

        let result = tool
            .call(SearchCodeArgs {
                pattern: "test".into(),
                path: None,
                file_ext: Some("rs; rm -rf /".into()),
            })
            .await;

        assert!(result.is_err());
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn search_code_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        // Create a secret file outside the work dir
        std::fs::write(outside.path().join("secret.txt"), "SECRET_DATA_HERE").unwrap();

        // Create a symlink inside work_dir pointing to the outside dir
        symlink(outside.path(), dir.path().join("escape")).unwrap();

        // Also create a real file inside work_dir so grep has something to find
        std::fs::write(dir.path().join("real.txt"), "normal data").unwrap();

        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));
        let tool = SearchCodeTool::new(dir.path().to_path_buf(), log);

        let result = tool
            .call(SearchCodeArgs {
                pattern: "SECRET_DATA_HERE".into(),
                path: None,
                file_ext: None,
            })
            .await
            .unwrap();

        // find -type f does not follow symlinks, so SECRET_DATA_HERE should not appear
        assert!(
            !result.contains("SECRET_DATA_HERE"),
            "grep should not follow symlinks into outside directory, got: {result}"
        );
    }

    // -----------------------------------------------------------------------
    // Path traversal protection tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_path_allows_normal_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        std::fs::write(&file, "hi").unwrap();

        let result = validate_path(dir.path(), "hello.txt");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), file.canonicalize().unwrap());
    }

    #[test]
    fn validate_path_rejects_dotdot_escape() {
        let dir = tempfile::tempdir().unwrap();
        // Create a file inside, then try to escape
        std::fs::write(dir.path().join("a.txt"), "").unwrap();

        let result = validate_path(dir.path(), "../../../etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("traversal denied") || err.contains("Failed to canonicalize"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_path_rejects_absolute_path_outside() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_path(dir.path(), "/etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("traversal denied") || err.contains("Failed to canonicalize"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_path_allows_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("f.txt"), "data").unwrap();

        let result = validate_path(dir.path(), "sub/f.txt");
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn validate_path_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();

        // Create a symlink inside work_dir pointing outside
        symlink(outside.path(), dir.path().join("escape_link")).unwrap();

        let result = validate_path(dir.path(), "escape_link/secret.txt");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("traversal denied"), "unexpected error: {err}");
    }

    #[test]
    fn validate_path_for_write_allows_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_path_for_write(dir.path(), "new_file.txt");
        assert!(result.is_ok());
        // Should end with new_file.txt
        assert!(result.unwrap().ends_with("new_file.txt"));
    }

    #[test]
    fn validate_path_for_write_allows_new_subdir() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_path_for_write(dir.path(), "newdir/file.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_path_for_write_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_path_for_write(dir.path(), "../../evil.txt");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("traversal denied"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn read_file_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ok.txt"), "safe").unwrap();

        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));
        let tool = ReadFileTool::new(dir.path().to_path_buf(), log);

        let result = tool
            .call(ReadFileArgs {
                path: "../../etc/passwd".into(),
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn write_file_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();

        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));
        let tool = WriteFileTool::new(dir.path().to_path_buf(), log);

        let result = tool
            .call(WriteFileArgs {
                path: "../../tmp/evil.txt".into(),
                content: "pwned".into(),
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn list_files_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();

        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));
        let tool = ListFilesTool::new(dir.path().to_path_buf(), log);

        let result = tool
            .call(ListFilesArgs {
                path: "../../etc".into(),
                pattern: None,
            })
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn search_code_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();

        let log: ToolCallLog = Arc::new(Mutex::new(Vec::new()));
        let tool = SearchCodeTool::new(dir.path().to_path_buf(), log);

        let result = tool
            .call(SearchCodeArgs {
                pattern: "root".into(),
                path: Some("../../etc".into()),
                file_ext: None,
            })
            .await;
        assert!(result.is_err());
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
