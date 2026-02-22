use crate::cdp::{build_method, CdpDomain, RemoteObject};
use crate::connection::CdpConnection;
use crate::error::{BrowserError, BrowserResult};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub struct Page {
    target_id: String,
    ws_url: String,
    connection: Option<CdpConnection>,
    event_receiver: broadcast::Receiver<crate::cdp::CdpEvent>,
    pages: Arc<RwLock<HashMap<String, Page>>>,
}

impl Page {
    pub async fn new(
        ws_url: &str,
        target_id: String,
        pages: Arc<RwLock<HashMap<String, Page>>>,
    ) -> BrowserResult<Self> {
        let connection = CdpConnection::connect(ws_url).await?;
        
        let event_receiver = connection.subscribe();

        let page = Self {
            target_id: target_id.clone(),
            ws_url: ws_url.to_string(),
            connection: Some(connection),
            event_receiver,
            pages,
        };

        page.enable_domains().await?;

        Ok(page)
    }

    async fn enable_domains(&self) -> BrowserResult<()> {
        if let Some(conn) = &self.connection {
            conn.enable_domains(&[
                "Page",
                "Network",
                "Runtime",
                "Console",
            ]).await?;
        }
        Ok(())
    }

    pub async fn navigate(&mut self, url: &str) -> BrowserResult<String> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let params = serde_json::json!({
            "url": url
        });

        let response = conn.send_command(
            &build_method(CdpDomain::Page, "navigate"),
            Some(params),
        ).await?;

        let frame_id = response.result
            .as_ref()
            .and_then(|r| r.get("frameId"))
            .and_then(|f| f.as_str())
            .map(|s| s.to_string());

        Ok(frame_id.unwrap_or_default())
    }

    pub async fn reload(&mut self) -> BrowserResult<()> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        conn.send_command(
            &build_method(CdpDomain::Page, "reload"),
            None,
        ).await?;

        Ok(())
    }

    pub async fn go_back(&mut self) -> BrowserResult<()> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        conn.send_command(
            &build_method(CdpDomain::Page, "navigateToHistoryEntry"),
            Some(serde_json::json!({
                "entryId": -1
            })),
        ).await?;

        Ok(())
    }

    pub async fn go_forward(&mut self) -> BrowserResult<()> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        conn.send_command(
            &build_method(CdpDomain::Page, "navigateToHistoryEntry"),
            Some(serde_json::json!({
                "entryId": 1
            })),
        ).await?;

        Ok(())
    }

    pub async fn evaluate(&self, expression: &str) -> BrowserResult<RemoteObject> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let params = serde_json::json!({
            "expression": expression,
            "includeCommandLineAPI": true,
            "silent": false,
            "returnByValue": false
        });

        let response = conn.send_command(
            &build_method(CdpDomain::Runtime, "evaluate"),
            Some(params),
        ).await?;

        let result = response.result
            .as_ref()
            .and_then(|r| r.get("result"))
            .cloned()
            .ok_or_else(|| BrowserError::ExecutionError("No result in response".to_string()))?;

        Ok(serde_json::from_value(result).unwrap_or(RemoteObject {
            object_type: "undefined".to_string(),
            subtype: None,
            class_name: None,
            value: None,
            unserializable_value: None,
            description: None,
            object_id: None,
        }))
    }

    pub async fn evaluate_and_wait(
        &self,
        expression: &str,
        wait_for: &str,
        timeout_ms: u64,
    ) -> BrowserResult<RemoteObject> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let params = serde_json::json!({
            "expression": format!("({}) === true", wait_for),
            "returnByValue": true,
            "awaitPromise": true
        });

        let start = std::time::Instant::now();
        loop {
            let response = conn.send_command(
                &build_method(CdpDomain::Runtime, "evaluate"),
                Some(params.clone()),
            ).await?;

            if let Some(result) = response.result.as_ref().and_then(|r| r.get("result"))
                && let Some(value) = result.get("value").and_then(|v| v.as_bool())
                && value
            {
                return self.evaluate(expression).await;
            }

            if start.elapsed().as_millis() > timeout_ms as u128 {
                return Err(BrowserError::Timeout(format!(
                    "Timeout after {}ms waiting for {}",
                    timeout_ms, wait_for
                )));
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    pub async fn take_screenshot(&self) -> BrowserResult<Vec<u8>> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let params = serde_json::json!({
            "format": "png",
            "captureBeyondViewport": true
        });

        let response = conn.send_command(
            &build_method(CdpDomain::Page, "captureScreenshot"),
            Some(params),
        ).await?;

        let data = response.result
            .as_ref()
            .and_then(|r| r.get("data"))
            .and_then(|d| d.as_str())
            .ok_or_else(|| BrowserError::PageError("No screenshot data".to_string()))?;

        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| BrowserError::ProtocolError(e.to_string()))
    }

    pub async fn get_document(&self) -> BrowserResult<String> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let response = conn.send_command(
            &build_method(CdpDomain::DOM, "getDocument"),
            None,
        ).await?;

        let root_node_id = response.result
            .as_ref()
            .and_then(|r| r.get("root"))
            .and_then(|root| root.get("nodeId"))
            .and_then(|id| id.as_i64())
            .ok_or_else(|| BrowserError::PageError("No document root".to_string()))?;

        Ok(root_node_id.to_string())
    }

    pub async fn click_element(&self, selector: &str) -> BrowserResult<()> {
        self.evaluate(&format!(
            r#"document.querySelector('{}').click()"#,
            selector
        )).await?;
        Ok(())
    }

    pub async fn type_text(&self, selector: &str, text: &str) -> BrowserResult<()> {
        self.evaluate(&format!(
            r#"
            {{
                const el = document.querySelector('{}');
                el.value = '{}';
                el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            }}
            "#,
            selector,
            text.replace('\'', "\\\'")
        )).await?;
        Ok(())
    }

    pub async fn get_cookies(&self) -> BrowserResult<Vec<serde_json::Value>> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let response = conn.send_command(
            &build_method(CdpDomain::Network, "getAllCookies"),
            None,
        ).await?;

        let cookies = response.result
            .as_ref()
            .and_then(|r| r.get("cookies"))
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();

        Ok(cookies)
    }

    pub async fn set_cookies(&self, cookies: Vec<serde_json::Value>) -> BrowserResult<()> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let params = serde_json::json!({
            "cookies": cookies
        });

        conn.send_command(
            &build_method(CdpDomain::Network, "setCookies"),
            Some(params),
        ).await?;

        Ok(())
    }

    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    pub async fn close(self) -> BrowserResult<()> {
        drop(self.connection);
        let mut pages = self.pages.write().await;
        pages.remove(&self.target_id);
        Ok(())
    }
}

impl Clone for Page {
    fn clone(&self) -> Self {
        Self {
            target_id: self.target_id.clone(),
            ws_url: self.ws_url.clone(),
            connection: None,
            event_receiver: self.event_receiver.resubscribe(),
            pages: Arc::clone(&self.pages),
        }
    }
}
