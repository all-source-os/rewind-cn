use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A telemetry event to be sent to PostHog.
#[derive(Debug, Clone)]
pub struct TelemetryEvent {
    pub event: String,
    pub properties: HashMap<String, serde_json::Value>,
}

/// Configuration for the telemetry client.
#[derive(Debug, Clone)]
pub struct TelemetryClientConfig {
    pub enabled: bool,
    pub posthog_key: Option<String>,
    pub posthog_host: String,
    pub distinct_id: String,
}

/// PostHog telemetry client. Feature-gated: when `telemetry` feature is off,
/// all methods are no-ops. When enabled but config.enabled is false, also no-ops.
#[derive(Clone)]
#[allow(dead_code)]
pub struct TelemetryClient {
    config: TelemetryClientConfig,
    #[cfg(feature = "telemetry")]
    buffer: Arc<Mutex<Vec<TelemetryEvent>>>,
    #[cfg(not(feature = "telemetry"))]
    _phantom: std::marker::PhantomData<Arc<Mutex<Vec<TelemetryEvent>>>>,
}

impl TelemetryClient {
    /// Create a new telemetry client. If telemetry is disabled or feature is off,
    /// this is essentially free.
    pub fn new(config: TelemetryClientConfig) -> Self {
        Self {
            config,
            #[cfg(feature = "telemetry")]
            buffer: Arc::new(Mutex::new(Vec::new())),
            #[cfg(not(feature = "telemetry"))]
            _phantom: std::marker::PhantomData,
        }
    }

    /// Create a disabled client (convenience for when no config is available).
    pub fn disabled() -> Self {
        Self::new(TelemetryClientConfig {
            enabled: false,
            posthog_key: None,
            posthog_host: String::new(),
            distinct_id: String::new(),
        })
    }

    /// Returns true if telemetry is active (feature compiled + config enabled + key present).
    pub fn is_active(&self) -> bool {
        #[cfg(feature = "telemetry")]
        {
            self.config.enabled && self.config.posthog_key.is_some()
        }
        #[cfg(not(feature = "telemetry"))]
        {
            false
        }
    }

    /// Capture a telemetry event. Buffered for batch sending.
    /// No-op if telemetry is disabled.
    pub async fn capture(&self, event: &str, properties: HashMap<String, serde_json::Value>) {
        if !self.is_active() {
            return;
        }

        #[cfg(feature = "telemetry")]
        {
            let mut buf = self.buffer.lock().await;
            buf.push(TelemetryEvent {
                event: event.into(),
                properties,
            });

            // Auto-flush every 20 events
            if buf.len() >= 20 {
                let events = std::mem::take(&mut *buf);
                drop(buf);
                self.send_batch(events).await;
            }
        }

        #[cfg(not(feature = "telemetry"))]
        {
            let _ = (event, properties);
        }
    }

    /// Convenience: capture with simple string properties.
    pub async fn capture_simple(&self, event: &str, props: &[(&str, &str)]) {
        if !self.is_active() {
            return;
        }
        let properties: HashMap<String, serde_json::Value> = props
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
            .collect();
        self.capture(event, properties).await;
    }

    /// Flush all buffered events to PostHog.
    pub async fn flush(&self) {
        #[cfg(feature = "telemetry")]
        {
            if !self.is_active() {
                return;
            }
            let events = {
                let mut buf = self.buffer.lock().await;
                std::mem::take(&mut *buf)
            };
            if !events.is_empty() {
                self.send_batch(events).await;
            }
        }
    }

    /// Send a batch of events to PostHog /batch endpoint.
    #[cfg(feature = "telemetry")]
    async fn send_batch(&self, events: Vec<TelemetryEvent>) {
        let Some(api_key) = &self.config.posthog_key else {
            return;
        };

        let batch: Vec<serde_json::Value> = events
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "event": e.event,
                    "properties": {
                        "distinct_id": self.config.distinct_id,
                        "$lib": "rewind-cn",
                        "$lib_version": env!("CARGO_PKG_VERSION"),
                    },
                    "additional_properties": e.properties,
                })
            })
            .collect();

        let body = serde_json::json!({
            "api_key": api_key,
            "batch": batch,
        });

        let url = format!("{}/batch/", self.config.posthog_host.trim_end_matches('/'));

        // Fire and forget — don't block on telemetry failures
        let _ = reqwest::Client::new()
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_client_is_noop() {
        let client = TelemetryClient::disabled();
        assert!(!client.is_active());

        // Should not panic or do anything
        client.capture("test.event", HashMap::new()).await;
        client.flush().await;
    }

    #[tokio::test]
    async fn capture_simple_convenience() {
        let client = TelemetryClient::disabled();
        client
            .capture_simple("test.event", &[("key", "value")])
            .await;
        // No panic = success for disabled client
    }
}
