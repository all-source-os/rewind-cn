//! Integration tests for MCP server (US-005).

use std::sync::Arc;

use rewind_cn_core::application::commands::CreateTask;
use rewind_cn_core::infrastructure::engine::RewindEngine;
use rewind_cn_core::infrastructure::mcp_server::{JsonRpcRequest, RewindMcpServer};
use serde_json::{json, Value};

fn make_request(method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: method.into(),
        params,
    }
}

#[tokio::test]
async fn tools_list_returns_all_tool_names() {
    let engine = Arc::new(RewindEngine::in_memory().await);
    let server = RewindMcpServer::new(engine, "/dev/null".into());
    let resp = server
        .handle_request(make_request("tools/list", json!({})))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let tools = result["tools"].as_array().unwrap();

    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();

    assert!(tool_names.contains(&"rewind_status"));
    assert!(tool_names.contains(&"rewind_task_list"));
    assert!(tool_names.contains(&"rewind_task_get"));
    assert!(tool_names.contains(&"rewind_list_iterations"));
    assert!(tool_names.contains(&"rewind_list_progress"));
    assert!(tool_names.contains(&"rewind_plan"));
    assert!(tool_names.contains(&"rewind_run"));
    assert!(tool_names.contains(&"rewind_feedback"));
}

#[tokio::test]
async fn tools_call_task_list_returns_content() {
    let engine = Arc::new(RewindEngine::in_memory().await);
    let server = RewindMcpServer::new(engine, "/dev/null".into());
    let resp = server
        .handle_request(make_request(
            "tools/call",
            json!({ "name": "rewind_task_list", "arguments": {} }),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert!(result["content"].is_array());
}

#[tokio::test]
async fn tools_call_status_returns_content() {
    let engine = Arc::new(RewindEngine::in_memory().await);
    let server = RewindMcpServer::new(engine, "/dev/null".into());
    let resp = server
        .handle_request(make_request(
            "tools/call",
            json!({ "name": "rewind_status", "arguments": {} }),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(!text.is_empty());
}

#[tokio::test]
async fn tools_call_unknown_tool_returns_error() {
    let engine = Arc::new(RewindEngine::in_memory().await);
    let server = RewindMcpServer::new(engine, "/dev/null".into());
    let resp = server
        .handle_request(make_request(
            "tools/call",
            json!({ "name": "nonexistent_tool", "arguments": {} }),
        ))
        .await;

    assert!(resp.error.is_some());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32602);
    assert!(err.message.contains("Unknown tool"));
}

#[tokio::test]
async fn tools_call_toon_format_differs_from_json() {
    let engine = RewindEngine::in_memory().await;

    // Create a task so there's data to format
    engine
        .create_task(CreateTask {
            title: "TOON test task".into(),
            description: "Test".into(),
            epic_id: None,
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        })
        .await
        .unwrap();

    let server = RewindMcpServer::new(Arc::new(engine), "/dev/null".into());

    // JSON format
    let json_resp = server
        .handle_request(make_request(
            "tools/call",
            json!({ "name": "rewind_task_list", "arguments": {} }),
        ))
        .await;
    let json_text = json_resp.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();

    // TOON format
    let toon_resp = server
        .handle_request(make_request(
            "tools/call",
            json!({ "name": "rewind_task_list", "arguments": { "format": "toon" } }),
        ))
        .await;
    let toon_text = toon_resp.result.unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();

    assert_ne!(json_text, toon_text, "TOON and JSON formats should differ");
    assert!(toon_text.contains('|'));
}

#[tokio::test]
async fn initialize_returns_server_info() {
    let engine = Arc::new(RewindEngine::in_memory().await);
    let server = RewindMcpServer::new(engine, "/dev/null".into());
    let resp = server
        .handle_request(make_request("initialize", json!({})))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["serverInfo"]["name"], "rewind");
    assert!(result["capabilities"]["tools"].is_object());
}

#[tokio::test]
async fn method_not_found_returns_error() {
    let engine = Arc::new(RewindEngine::in_memory().await);
    let server = RewindMcpServer::new(engine, "/dev/null".into());
    let resp = server
        .handle_request(make_request("nonexistent/method", json!({})))
        .await;

    assert!(resp.error.is_some());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32601);
    assert!(err.message.contains("Method not found"));
}
