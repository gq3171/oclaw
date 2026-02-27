/// Gateway HTTP client for TUI — talks to the OCLAWS gateway REST API.
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub url: String,
    pub token: Option<String>,
    pub session: String,
    pub model: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            url: "http://127.0.0.1:8081".to_string(),
            token: None,
            session: "default".to_string(),
            model: "default".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum GatewayEvent {
    Connected,
    Chunk(String),
    Done(String),
    Error(String),
    ModelsLoaded(Vec<String>),
    SessionsLoaded(Vec<SessionInfo>),
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub key: String,
    pub message_count: u64,
}

#[derive(Debug, Clone)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
}

pub struct GatewayClient {
    config: GatewayConfig,
    client: reqwest::Client,
}

impl GatewayClient {
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn config(&self) -> &GatewayConfig {
        &self.config
    }

    pub fn set_model(&mut self, model: &str) {
        self.config.model = model.to_string();
    }

    pub fn set_session(&mut self, session: &str) {
        self.config.session = session.to_string();
    }

    pub async fn health(&self) -> Result<bool, String> {
        let resp = self
            .client
            .get(format!("{}/health", self.config.url))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(resp.status().is_success())
    }

    /// Send a chat message with SSE streaming. Chunks arrive on the returned receiver.
    pub async fn send_message(
        &self,
        text: &str,
        tx: mpsc::Sender<GatewayEvent>,
    ) -> Result<(), String> {
        let messages = vec![serde_json::json!({"role": "user", "content": text})];
        self.send_messages_raw(messages, tx).await
    }

    /// Send full conversation history with SSE streaming.
    pub async fn send_messages(
        &self,
        messages: &[(String, String)],
        tx: mpsc::Sender<GatewayEvent>,
    ) -> Result<(), String> {
        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .map(|(role, content)| serde_json::json!({"role": role, "content": content}))
            .collect();
        self.send_messages_raw(msgs, tx).await
    }

    /// Internal: send a messages array to the chat completions endpoint.
    async fn send_messages_raw(
        &self,
        messages: Vec<serde_json::Value>,
        tx: mpsc::Sender<GatewayEvent>,
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "stream": true,
        });

        let mut req = self
            .client
            .post(format!("{}/v1/chat/completions", self.config.url))
            .header("X-Session-Id", &self.config.session)
            .json(&body);

        if let Some(ref token) = self.config.token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await.map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let _ = tx
                .send(GatewayEvent::Error(format!("HTTP {}: {}", status, text)))
                .await;
            return Err(format!("HTTP {}", status));
        }

        // Check content-type: server may force non-streaming (e.g. during hatching)
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !content_type.contains("text/event-stream") {
            // Non-streaming JSON response — parse and emit as single chunk
            let text = resp.text().await.map_err(|e| e.to_string())?;
            let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if let Some(content) = json
                .pointer("/choices/0/message/content")
                .and_then(|v| v.as_str())
            {
                let _ = tx.send(GatewayEvent::Chunk(content.to_string())).await;
                let _ = tx.send(GatewayEvent::Done(content.to_string())).await;
            } else {
                let _ = tx.send(GatewayEvent::Done(String::new())).await;
            }
            return Ok(());
        }

        // Parse SSE stream
        let mut full_text = String::new();
        let mut stream = resp.bytes_stream();
        use futures::StreamExt;
        let mut buf = String::new();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| e.to_string())?;
            buf.push_str(&String::from_utf8_lossy(&bytes));

            // Process complete SSE lines
            while let Some(pos) = buf.find("\n\n") {
                let event_block = buf[..pos].to_string();
                buf = buf[pos + 2..].to_string();

                for line in event_block.lines() {
                    let Some(data) = line.strip_prefix("data: ") else {
                        continue;
                    };
                    if data.trim() == "[DONE]" {
                        let _ = tx.send(GatewayEvent::Done(full_text.clone())).await;
                        return Ok(());
                    }
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(data)
                        && let Some(content) = json
                            .pointer("/choices/0/delta/content")
                            .and_then(|v| v.as_str())
                    {
                        full_text.push_str(content);
                        let _ = tx.send(GatewayEvent::Chunk(content.to_string())).await;
                    }
                }
            }
        }

        // If stream ended without [DONE]
        if !full_text.is_empty() {
            let _ = tx.send(GatewayEvent::Done(full_text)).await;
        }
        Ok(())
    }

    /// Non-streaming message send, returns full response.
    pub async fn send_message_sync(&self, text: &str) -> Result<String, String> {
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [{"role": "user", "content": text}],
            "stream": false,
        });

        let mut req = self
            .client
            .post(format!("{}/v1/chat/completions", self.config.url))
            .header("X-Session-Id", &self.config.session)
            .json(&body);

        if let Some(ref token) = self.config.token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await.map_err(|e| e.to_string())?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| e.to_string())?;

        if !status.is_success() {
            return Err(format!("HTTP {}: {}", status, text));
        }

        let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        Ok(json
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or(&text)
            .to_string())
    }

    /// Fetch recent transcript history for a session.
    pub async fn fetch_history(
        &self,
        session_key: &str,
        limit: usize,
    ) -> Result<Vec<HistoryMessage>, String> {
        let mut req = self.client.get(format!(
            "{}/transcript/{}?limit={}",
            self.config.url, session_key, limit
        ));
        if let Some(ref token) = self.config.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let text = resp.text().await.map_err(|e| e.to_string())?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let messages = json["messages"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        Some(HistoryMessage {
                            role: v["role"].as_str()?.to_string(),
                            content: v["content"].as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(messages)
    }

    /// Query the agent status endpoint (includes hatching state).
    pub async fn check_agent_status(&self) -> Result<serde_json::Value, String> {
        let mut req = self.client.get(format!("{}/agent/status", self.config.url));
        if let Some(ref token) = self.config.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        let text = resp.text().await.map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    pub async fn list_models(&self) -> Result<Vec<String>, String> {
        let mut req = self.client.get(format!("{}/models", self.config.url));
        if let Some(ref token) = self.config.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        let text = resp.text().await.map_err(|e| e.to_string())?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let models = json["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v["id"].as_str().or(v.as_str()))
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        Ok(models)
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionInfo>, String> {
        let mut req = self.client.get(format!("{}/sessions", self.config.url));
        if let Some(ref token) = self.config.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        let text = resp.text().await.map_err(|e| e.to_string())?;
        let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let sessions = json["sessions"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|v| SessionInfo {
                        key: v["key"].as_str().unwrap_or("").to_string(),
                        message_count: v["messageCount"].as_u64().unwrap_or(0),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(sessions)
    }
}
