use std::io::{self, BufRead, Write};
use std::sync::Arc;

use crate::application::commands::{CreateEpic, CreateTask};
use crate::application::planning::passthrough_plan;
use crate::application::scheduler::pick_runnable_tasks;
use crate::application::status::build_summary;
use crate::domain::error::RewindError;
use crate::domain::events::RewindEvent;
use crate::infrastructure::agent::AgentWorker;
use crate::infrastructure::toon;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::engine::RewindEngine;

/// Check if the caller requested TOON format.
fn wants_toon(args: &Value) -> bool {
    args.get("format")
        .and_then(|v| v.as_str())
        .map(|s| s == "toon")
        .unwrap_or(false)
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcResponse {
    fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<Value>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

pub struct RewindMcpServer<B: allframe::cqrs::EventStoreBackend<RewindEvent>> {
    engine: Arc<RewindEngine<B>>,
    config_path: String,
}

impl<B: allframe::cqrs::EventStoreBackend<RewindEvent>> RewindMcpServer<B> {
    pub fn new(engine: Arc<RewindEngine<B>>, config_path: String) -> Self {
        Self {
            engine,
            config_path,
        }
    }

    pub async fn run(&self) -> Result<(), RewindError> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        for line in stdin.lock().lines() {
            let line = line.map_err(|e| RewindError::Storage(format!("stdin read error: {e}")))?;
            if line.trim().is_empty() {
                continue;
            }

            let request: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    let resp = JsonRpcResponse::error(None, -32700, format!("Parse error: {e}"));
                    self.write_response(&mut stdout, &resp)?;
                    continue;
                }
            };

            let response = self.handle_request(request).await;
            self.write_response(&mut stdout, &response)?;
        }

        Ok(())
    }

    fn write_response(
        &self,
        stdout: &mut io::Stdout,
        response: &JsonRpcResponse,
    ) -> Result<(), RewindError> {
        let json = serde_json::to_string(response)
            .map_err(|e| RewindError::Storage(format!("serialize error: {e}")))?;
        writeln!(stdout, "{json}")
            .map_err(|e| RewindError::Storage(format!("stdout write error: {e}")))?;
        stdout
            .flush()
            .map_err(|e| RewindError::Storage(format!("stdout flush error: {e}")))?;
        Ok(())
    }

    async fn handle_request(&self, req: JsonRpcRequest) -> JsonRpcResponse {
        if req.jsonrpc != "2.0" {
            return JsonRpcResponse::error(req.id, -32600, "Invalid JSON-RPC version");
        }

        match req.method.as_str() {
            "initialize" => self.handle_initialize(req.id),
            "initialized" => JsonRpcResponse::success(req.id, json!({})),
            "shutdown" => JsonRpcResponse::success(req.id, json!(null)),
            "tools/list" => self.handle_tools_list(req.id),
            "tools/call" => self.handle_tools_call(req.id, req.params).await,
            "resources/list" => self.handle_resources_list(req.id),
            "resources/read" => self.handle_resources_read(req.id, req.params).await,
            _ => {
                JsonRpcResponse::error(req.id, -32601, format!("Method not found: {}", req.method))
            }
        }
    }

    fn handle_initialize(&self, id: Option<Value>) -> JsonRpcResponse {
        JsonRpcResponse::success(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": "rewind",
                    "version": "0.1.0"
                }
            }),
        )
    }

    fn handle_tools_list(&self, id: Option<Value>) -> JsonRpcResponse {
        JsonRpcResponse::success(
            id,
            json!({
                "tools": [
                    {
                        "name": "rewind_status",
                        "description": "Show project status: task counts by status and epic progress",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "format": {
                                    "type": "string",
                                    "description": "Output format: 'json' (default) or 'toon' (token-optimized, ~50% fewer tokens)"
                                }
                            }
                        }
                    },
                    {
                        "name": "rewind_plan",
                        "description": "Create an execution plan from a task description",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "description": {
                                    "type": "string",
                                    "description": "Task or PRD description to plan"
                                }
                            },
                            "required": ["description"]
                        }
                    },
                    {
                        "name": "rewind_run",
                        "description": "Execute pending tasks with agent workers",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "max_concurrent": {
                                    "type": "integer",
                                    "description": "Maximum concurrent workers (default: 3)"
                                }
                            }
                        }
                    },
                    {
                        "name": "rewind_task_list",
                        "description": "List all tasks, optionally filtered by status",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "status": {
                                    "type": "string",
                                    "description": "Filter by status: pending, assigned, in-progress, completed, failed, blocked"
                                },
                                "format": {
                                    "type": "string",
                                    "description": "Output format: 'json' (default) or 'toon' (token-optimized, ~50% fewer tokens)"
                                }
                            }
                        }
                    },
                    {
                        "name": "rewind_task_get",
                        "description": "Get details of a single task by ID",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "task_id": {
                                    "type": "string",
                                    "description": "The task ID"
                                },
                                "format": {
                                    "type": "string",
                                    "description": "Output format: 'json' (default) or 'toon' (token-optimized, ~50% fewer tokens)"
                                }
                            },
                            "required": ["task_id"]
                        }
                    }
                ]
            }),
        )
    }

    async fn handle_tools_call(&self, id: Option<Value>, params: Value) -> JsonRpcResponse {
        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        match tool_name {
            "rewind_status" => self.tool_status(id, arguments).await,
            "rewind_plan" => self.tool_plan(id, arguments).await,
            "rewind_run" => self.tool_run(id, arguments).await,
            "rewind_task_list" => self.tool_task_list(id, arguments).await,
            "rewind_task_get" => self.tool_task_get(id, arguments).await,
            _ => JsonRpcResponse::error(id, -32602, format!("Unknown tool: {tool_name}")),
        }
    }

    async fn tool_status(&self, id: Option<Value>, args: Value) -> JsonRpcResponse {
        if let Err(e) = self.engine.rebuild_projections().await {
            return JsonRpcResponse::error(id, -32000, e.to_string());
        }

        let backlog = self.engine.backlog();
        let backlog = backlog.read().await;
        let epic_progress = self.engine.epic_progress();
        let epic_progress = epic_progress.read().await;
        let summary = build_summary(&backlog, &epic_progress);

        if wants_toon(&args) {
            let text = toon::format_status(&summary);
            return JsonRpcResponse::success(
                id,
                json!({ "content": [{ "type": "text", "text": text }] }),
            );
        }

        match serde_json::to_value(&summary) {
            Ok(v) => JsonRpcResponse::success(
                id,
                json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }] }),
            ),
            Err(e) => JsonRpcResponse::error(id, -32000, e.to_string()),
        }
    }

    async fn tool_plan(&self, id: Option<Value>, args: Value) -> JsonRpcResponse {
        let description = match args.get("description").and_then(|v| v.as_str()) {
            Some(d) => d,
            None => {
                return JsonRpcResponse::error(
                    id,
                    -32602,
                    "Missing required parameter: description",
                )
            }
        };

        let plan = passthrough_plan(description);

        // Create epic
        let epic_events = match self
            .engine
            .create_epic(CreateEpic {
                title: plan.epic_title.clone(),
                description: plan.epic_description.clone(),
            })
            .await
        {
            Ok(events) => events,
            Err(e) => return JsonRpcResponse::error(id, -32000, e.to_string()),
        };

        let epic_id = match &epic_events[0] {
            RewindEvent::EpicCreated { epic_id, .. } => epic_id.clone(),
            _ => return JsonRpcResponse::error(id, -32000, "Unexpected event"),
        };

        // Create tasks
        for task in &plan.tasks {
            if let Err(e) = self
                .engine
                .create_task(CreateTask {
                    title: task.title.clone(),
                    description: task.description.clone(),
                    epic_id: Some(epic_id.clone()),
                })
                .await
            {
                return JsonRpcResponse::error(id, -32000, e.to_string());
            }
        }

        match serde_json::to_value(&plan) {
            Ok(v) => JsonRpcResponse::success(
                id,
                json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }] }),
            ),
            Err(e) => JsonRpcResponse::error(id, -32000, e.to_string()),
        }
    }

    async fn tool_run(&self, id: Option<Value>, args: Value) -> JsonRpcResponse {
        let max_concurrent = args
            .get("max_concurrent")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;

        if let Err(e) = self.engine.rebuild_projections().await {
            return JsonRpcResponse::error(id, -32000, e.to_string());
        }

        let tasks_to_run: Vec<(crate::domain::ids::TaskId, String)> = {
            let backlog = self.engine.backlog();
            let backlog = backlog.read().await;
            let runnable = pick_runnable_tasks(&backlog, max_concurrent);
            runnable
                .iter()
                .map(|t| (t.task_id.clone(), t.title.clone()))
                .collect()
        };

        if tasks_to_run.is_empty() {
            return JsonRpcResponse::success(
                id,
                json!({ "content": [{ "type": "text", "text": "No pending tasks to run." }] }),
            );
        }

        let mut completed = 0;
        let mut failed = 0;

        for (task_id, title) in &tasks_to_run {
            let worker = AgentWorker::new();
            match worker
                .execute_task(task_id.clone(), title, self.engine.as_ref())
                .await
            {
                Ok(_) => completed += 1,
                Err(_) => failed += 1,
            }
        }

        let summary = format!(
            "{} tasks executed ({} passed, {} failed)",
            tasks_to_run.len(),
            completed,
            failed
        );

        JsonRpcResponse::success(
            id,
            json!({ "content": [{ "type": "text", "text": summary }] }),
        )
    }

    async fn tool_task_list(&self, id: Option<Value>, args: Value) -> JsonRpcResponse {
        if let Err(e) = self.engine.rebuild_projections().await {
            return JsonRpcResponse::error(id, -32000, e.to_string());
        }

        let status_filter = args
            .get("status")
            .and_then(|v| v.as_str())
            .map(String::from);

        let backlog = self.engine.backlog();
        let backlog = backlog.read().await;

        let tasks: Vec<&crate::domain::model::TaskView> = backlog
            .tasks
            .values()
            .filter(|t| {
                if let Some(ref status) = status_filter {
                    t.status.to_string() == *status
                } else {
                    true
                }
            })
            .collect();

        if wants_toon(&args) {
            let text = toon::format_task_list(&tasks);
            return JsonRpcResponse::success(
                id,
                json!({ "content": [{ "type": "text", "text": text }] }),
            );
        }

        match serde_json::to_value(&tasks) {
            Ok(v) => JsonRpcResponse::success(
                id,
                json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }] }),
            ),
            Err(e) => JsonRpcResponse::error(id, -32000, e.to_string()),
        }
    }

    async fn tool_task_get(&self, id: Option<Value>, args: Value) -> JsonRpcResponse {
        let task_id = match args.get("task_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => {
                return JsonRpcResponse::error(id, -32602, "Missing required parameter: task_id")
            }
        };

        if let Err(e) = self.engine.rebuild_projections().await {
            return JsonRpcResponse::error(id, -32000, e.to_string());
        }

        let backlog = self.engine.backlog();
        let backlog = backlog.read().await;

        match backlog.tasks.get(task_id) {
            Some(task) => {
                if wants_toon(&args) {
                    let text = toon::format_task_detail(task);
                    return JsonRpcResponse::success(
                        id,
                        json!({ "content": [{ "type": "text", "text": text }] }),
                    );
                }
                match serde_json::to_value(task) {
                    Ok(v) => JsonRpcResponse::success(
                        id,
                        json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }] }),
                    ),
                    Err(e) => JsonRpcResponse::error(id, -32000, e.to_string()),
                }
            }
            None => JsonRpcResponse::error(id, -32000, format!("Task not found: {task_id}")),
        }
    }

    fn handle_resources_list(&self, id: Option<Value>) -> JsonRpcResponse {
        JsonRpcResponse::success(
            id,
            json!({
                "resources": [
                    {
                        "uri": "rewind://backlog",
                        "name": "Backlog",
                        "description": "Current task backlog",
                        "mimeType": "application/json"
                    },
                    {
                        "uri": "rewind://epics",
                        "name": "Epic Progress",
                        "description": "Epic progress tracking",
                        "mimeType": "application/json"
                    },
                    {
                        "uri": "rewind://config",
                        "name": "Configuration",
                        "description": "rewind.toml configuration",
                        "mimeType": "text/plain"
                    }
                ]
            }),
        )
    }

    async fn handle_resources_read(&self, id: Option<Value>, params: Value) -> JsonRpcResponse {
        let uri = match params.get("uri").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return JsonRpcResponse::error(id, -32602, "Missing required parameter: uri"),
        };

        match uri {
            "rewind://backlog" => {
                if let Err(e) = self.engine.rebuild_projections().await {
                    return JsonRpcResponse::error(id, -32000, e.to_string());
                }
                let backlog = self.engine.backlog();
                let backlog = backlog.read().await;
                let tasks: Vec<_> = backlog.tasks.values().collect();
                let text = serde_json::to_string_pretty(&tasks).unwrap_or_default();
                JsonRpcResponse::success(
                    id,
                    json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": "application/json",
                            "text": text
                        }]
                    }),
                )
            }
            "rewind://epics" => {
                if let Err(e) = self.engine.rebuild_projections().await {
                    return JsonRpcResponse::error(id, -32000, e.to_string());
                }
                let epics = self.engine.epic_progress();
                let epics = epics.read().await;
                let list: Vec<_> = epics.epics.values().collect();
                let text = serde_json::to_string_pretty(&list).unwrap_or_default();
                JsonRpcResponse::success(
                    id,
                    json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": "application/json",
                            "text": text
                        }]
                    }),
                )
            }
            "rewind://config" => {
                let text = std::fs::read_to_string(&self.config_path)
                    .unwrap_or_else(|_| "# Config not found".into());
                JsonRpcResponse::success(
                    id,
                    json!({
                        "contents": [{
                            "uri": uri,
                            "mimeType": "text/plain",
                            "text": text
                        }]
                    }),
                )
            }
            _ => JsonRpcResponse::error(id, -32000, format!("Unknown resource: {uri}")),
        }
    }
}
