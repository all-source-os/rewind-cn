use std::collections::HashMap;
use std::path::Path;

use rewind_cn_core::domain::events::RewindEvent;
use rewind_cn_core::infrastructure::engine::RewindEngine;
use serde::Serialize;
use sha2::{Digest, Sha256};

const DATA_DIR: &str = ".rewind/data";
const CONFIG_FILE: &str = ".rewind/rewind.toml";

#[derive(Serialize)]
struct DiagnosticReport {
    version: &'static str,
    system: SystemInfo,
    config: Option<serde_json::Value>,
    session: Option<SessionReport>,
    events: Vec<serde_json::Value>,
}

#[derive(Serialize)]
struct SystemInfo {
    os: String,
    arch: String,
    rustc: String,
}

#[derive(Serialize)]
struct SessionReport {
    session_id: String,
    started_at: String,
    ended_at: Option<String>,
    event_count: usize,
}

pub async fn execute(session_id: Option<String>, full: bool) -> Result<(), String> {
    if !Path::new(".rewind").exists() {
        return Err("No rewind project found. Run `rewind init` first.".into());
    }

    let engine = RewindEngine::load(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    let events = engine
        .event_store
        .get_all_events()
        .await
        .map_err(|e| format!("Failed to read events: {e}"))?;

    // Find the target session
    let target_session = find_target_session(&events, session_id.as_deref());

    // Filter events to the target session (or all if no session found)
    let filtered_events = match &target_session {
        Some(report) => filter_session_events(&events, &report.session_id),
        None => events.clone(),
    };

    // Serialize events, anonymizing if needed
    let serialized_events: Vec<serde_json::Value> = filtered_events
        .iter()
        .map(|e| {
            if full {
                serde_json::to_value(e).unwrap_or_default()
            } else {
                anonymize_event(e)
            }
        })
        .collect();

    // Load and redact config
    let config = load_redacted_config();

    let report = DiagnosticReport {
        version: env!("CARGO_PKG_VERSION"),
        system: SystemInfo {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            rustc: rustc_version(),
        },
        config,
        session: target_session,
        events: serialized_events,
    };

    let json = serde_json::to_string_pretty(&report)
        .map_err(|e| format!("Failed to serialize report: {e}"))?;

    let filename = match &report.session {
        Some(s) => format!("rewind-report-{}.json", s.session_id),
        None => "rewind-report-latest.json".into(),
    };

    std::fs::write(&filename, &json).map_err(|e| format!("Failed to write report: {e}"))?;

    eprintln!("Report written to {filename}");
    eprintln!(
        "  Events: {}, Size: {} bytes",
        report.events.len(),
        json.len()
    );
    if !full {
        eprintln!("  (anonymized — use --full to include raw task titles/descriptions)");
    }

    Ok(())
}

/// Find the target session: specific ID, or the last session.
fn find_target_session(events: &[RewindEvent], target_id: Option<&str>) -> Option<SessionReport> {
    let mut sessions: HashMap<String, (String, Option<String>, usize)> = HashMap::new();
    let mut last_session_id: Option<String> = None;

    for event in events {
        match event {
            RewindEvent::SessionStarted {
                session_id,
                started_at,
            } => {
                let sid = session_id.to_string();
                sessions.insert(sid.clone(), (started_at.to_rfc3339(), None, 0));
                last_session_id = Some(sid);
            }
            RewindEvent::SessionEnded {
                session_id,
                ended_at,
            } => {
                let sid = session_id.to_string();
                if let Some(entry) = sessions.get_mut(&sid) {
                    entry.1 = Some(ended_at.to_rfc3339());
                }
            }
            _ => {}
        }
    }

    // Count events per session (approximate: events between session start/end)
    let target = match target_id {
        Some(id) => Some(id.to_string()),
        None => last_session_id,
    };

    target.and_then(|sid| {
        sessions.get(&sid).map(|(started, ended, _)| {
            let count = filter_session_events(events, &sid).len();
            SessionReport {
                session_id: sid,
                started_at: started.clone(),
                ended_at: ended.clone(),
                event_count: count,
            }
        })
    })
}

/// Filter events belonging to a session (between SessionStarted and SessionEnded).
fn filter_session_events(events: &[RewindEvent], session_id: &str) -> Vec<RewindEvent> {
    let mut in_session = false;
    let mut result = Vec::new();

    for event in events {
        match event {
            RewindEvent::SessionStarted { session_id: sid, .. } if sid.to_string() == session_id => {
                in_session = true;
                result.push(event.clone());
            }
            RewindEvent::SessionEnded { session_id: sid, .. } if sid.to_string() == session_id => {
                result.push(event.clone());
                break;
            }
            _ if in_session => {
                result.push(event.clone());
            }
            _ => {}
        }
    }

    // If no session boundaries found, return all events
    if result.is_empty() {
        return events.to_vec();
    }

    result
}

/// Anonymize an event: hash titles/descriptions, strip file contents.
fn anonymize_event(event: &RewindEvent) -> serde_json::Value {
    let mut value = serde_json::to_value(event).unwrap_or_default();

    if let Some(obj) = value.as_object_mut() {
        // Hash title fields
        if let Some(title) = obj.get("title").and_then(|v| v.as_str()) {
            obj.insert("title".into(), serde_json::Value::String(hash_string(title)));
        }

        // Strip description fields
        if obj.contains_key("description") {
            obj.insert(
                "description".into(),
                serde_json::Value::String("[redacted]".into()),
            );
        }

        // Strip acceptance criteria descriptions
        if let Some(criteria) = obj.get_mut("acceptance_criteria") {
            if let Some(arr) = criteria.as_array_mut() {
                for item in arr.iter_mut() {
                    if let Some(obj) = item.as_object_mut() {
                        if obj.contains_key("description") {
                            obj.insert(
                                "description".into(),
                                serde_json::Value::String("[redacted]".into()),
                            );
                        }
                    }
                }
            }
        }

        // Strip quality gate output (may contain secrets in env)
        if let Some(output) = obj.get("output").and_then(|v| v.as_str()) {
            let len = output.len();
            obj.insert(
                "output".into(),
                serde_json::Value::String(format!("[{len} bytes redacted]")),
            );
        }

        // Strip tool call args/results (may contain file contents)
        if let Some(args) = obj.get("args_summary").and_then(|v| v.as_str()) {
            obj.insert(
                "args_summary".into(),
                serde_json::Value::String(hash_string(args)),
            );
        }
    }

    value
}

/// SHA-256 hash, truncated to 12 hex chars.
fn hash_string(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    format!("sha256:{:.12}", hex::encode(result))
}

/// Load config from .rewind/rewind.toml, redacting API keys.
fn load_redacted_config() -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(CONFIG_FILE).ok()?;
    let mut config: toml::Value = toml::from_str(&content).ok()?;

    // Redact any key containing "key", "secret", "token", "password"
    redact_secrets(&mut config);

    serde_json::to_value(config).ok()
}

fn redact_secrets(value: &mut toml::Value) {
    match value {
        toml::Value::Table(table) => {
            for (key, val) in table.iter_mut() {
                let key_lower = key.to_lowercase();
                let is_sensitive = key_lower.contains("key")
                    || key_lower.contains("secret")
                    || key_lower.contains("token")
                    || key_lower.contains("password");

                if is_sensitive && val.is_str() {
                    *val = toml::Value::String("[REDACTED]".into());
                } else {
                    redact_secrets(val);
                }
            }
        }
        toml::Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_secrets(item);
            }
        }
        _ => {}
    }
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymize_hashes_titles() {
        let event = RewindEvent::TaskCreated {
            task_id: rewind_cn_core::domain::ids::TaskId::new("t-1"),
            title: "Secret task name".into(),
            description: "Sensitive description".into(),
            epic_id: None,
            created_at: chrono::Utc::now(),
            acceptance_criteria: vec![],
            story_type: None,
            depends_on: vec![],
        };

        let anon = anonymize_event(&event);
        let obj = anon.as_object().unwrap();

        // Title should be hashed
        let title = obj.get("title").unwrap().as_str().unwrap();
        assert!(title.starts_with("sha256:"));
        assert!(!title.contains("Secret"));

        // Description should be redacted
        let desc = obj.get("description").unwrap().as_str().unwrap();
        assert_eq!(desc, "[redacted]");
    }

    #[test]
    fn redact_secrets_strips_keys() {
        let toml_str = r#"
            project_name = "test"
            [agent]
            api_key_env = "ANTHROPIC_API_KEY"
            [secrets]
            password = "hunter2"
            token = "abc123"
        "#;

        let mut config: toml::Value = toml::from_str(toml_str).unwrap();
        redact_secrets(&mut config);

        let table = config.as_table().unwrap();
        let agent = table.get("agent").unwrap().as_table().unwrap();
        assert_eq!(
            agent.get("api_key_env").unwrap().as_str().unwrap(),
            "[REDACTED]"
        );

        let secrets = table.get("secrets").unwrap().as_table().unwrap();
        assert_eq!(
            secrets.get("password").unwrap().as_str().unwrap(),
            "[REDACTED]"
        );
        assert_eq!(
            secrets.get("token").unwrap().as_str().unwrap(),
            "[REDACTED]"
        );

        // project_name should not be redacted
        assert_eq!(
            table.get("project_name").unwrap().as_str().unwrap(),
            "test"
        );
    }
}
