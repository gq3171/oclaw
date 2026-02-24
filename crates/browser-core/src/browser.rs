use crate::cdp::{build_method, CdpDomain, TargetInfo};
use crate::connection::CdpConnection;
use crate::error::{BrowserError, BrowserResult};
use crate::page::Page;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

#[derive(Debug, Clone)]
pub struct BrowserProfile {
    pub id: String,
    pub name: String,
    pub data_dir: Option<String>,
    pub args: Vec<String>,
    pub proxy: Option<ProxyConfig>,
}

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub server: String,
    pub bypass_list: Option<Vec<String>>,
}

pub struct BrowserManager {
    cdp_url: String,
    connection: Option<CdpConnection>,
    pages: Arc<RwLock<HashMap<String, Page>>>,
    profiles: Arc<RwLock<HashMap<String, BrowserProfile>>>,
    _browser_context_id: Option<String>,
}

impl BrowserManager {
    /// Discover the browser WebSocket debugger URL from the CDP HTTP endpoint.
    async fn discover_ws_url(cdp_url: &str) -> BrowserResult<String> {
        // If already a full devtools WS path, use as-is
        if cdp_url.starts_with("ws://") && cdp_url.contains("/devtools/") {
            return Ok(cdp_url.to_string());
        }

        // Derive HTTP base from whatever URL form was given
        let base = cdp_url
            .replace("ws://", "http://")
            .replace("wss://", "https://")
            .trim_end_matches('/')
            .to_string();

        let version_url = format!("{}/json/version", base);
        debug!("Discovering CDP WebSocket URL from {}", version_url);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .map_err(|e| BrowserError::ConnectionError(e.to_string()))?;

        let resp = client.get(&version_url).send().await
            .map_err(|e| BrowserError::ConnectionError(format!("CDP discovery failed: {}", e)))?;

        let json: serde_json::Value = resp.json().await
            .map_err(|e| BrowserError::ConnectionError(format!("CDP discovery parse error: {}", e)))?;

        json.get("webSocketDebuggerUrl")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| BrowserError::ConnectionError(
                "CDP /json/version missing webSocketDebuggerUrl".into()
            ))
    }

    pub async fn new(cdp_url: &str) -> BrowserResult<Self> {
        let ws_url = Self::discover_ws_url(cdp_url).await?;
        let connection = CdpConnection::connect(&ws_url).await?;

        let browser = Self {
            cdp_url: cdp_url.to_string(),
            connection: Some(connection),
            pages: Arc::new(RwLock::new(HashMap::new())),
            profiles: Arc::new(RwLock::new(HashMap::new())),
            _browser_context_id: None,
        };

        Ok(browser)
    }

    pub async fn create_profile(&self, name: &str, data_dir: Option<&str>) -> BrowserProfile {
        let profile = BrowserProfile {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            data_dir: data_dir.map(String::from),
            args: Vec::new(),
            proxy: None,
        };
        
        let mut profiles = self.profiles.write().await;
        profiles.insert(profile.id.clone(), profile.clone());
        
        profile
    }

    pub async fn get_profile(&self, id: &str) -> Option<BrowserProfile> {
        let profiles = self.profiles.read().await;
        profiles.get(id).cloned()
    }

    pub async fn list_profiles(&self) -> Vec<BrowserProfile> {
        let profiles = self.profiles.read().await;
        profiles.values().cloned().collect()
    }

    pub async fn delete_profile(&self, id: &str) -> bool {
        let mut profiles = self.profiles.write().await;
        profiles.remove(id).is_some()
    }

    pub async fn update_profile(&self, id: &str, profile: BrowserProfile) -> bool {
        let mut profiles = self.profiles.write().await;
        if profiles.contains_key(id) {
            profiles.insert(id.to_string(), profile);
            true
        } else {
            false
        }
    }

    pub async fn connect(&mut self) -> BrowserResult<()> {
        if let Some(conn) = &self.connection {
            conn.enable_domains(&[
                "Browser",
                "Target",
                "Page",
                "Network",
                "Runtime",
                "Console",
            ]).await?;
        }
        Ok(())
    }

    pub async fn disconnect(&mut self) -> BrowserResult<()> {
        self.connection = None;
        let mut pages = self.pages.write().await;
        pages.clear();
        Ok(())
    }

    pub async fn list_targets(&self) -> BrowserResult<Vec<TargetInfo>> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let response = conn.send_command(
            &build_method(CdpDomain::Target, "getTargets"),
            None,
        ).await?;

        let targets: Vec<TargetInfo> = serde_json::from_value(
            response.result
                .as_ref()
                .and_then(|r| r.get("targetInfos"))
                .cloned()
                .unwrap_or(serde_json::Value::Array(vec![]))
        ).unwrap_or_default();

        Ok(targets)
    }

    pub async fn create_page(&mut self) -> BrowserResult<Page> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let params = serde_json::json!({
            "url": "about:blank"
        });

        let response = conn.send_command(
            &build_method(CdpDomain::Target, "createTarget"),
            Some(params),
        ).await?;

        let target_id = response.result
            .as_ref()
            .and_then(|r| r.get("targetId"))
            .and_then(|id| id.as_str())
            .ok_or_else(|| BrowserError::TargetNotFound("Failed to create target".to_string()))?
            .to_string();

        let host = self.cdp_url.replace("ws://", "").replace("/DevToolsBrowser", "");
        let page_url = format!("ws://{}/devtools/page/{}", host, target_id);

        let page = Page::new(&page_url, target_id.clone(), Arc::clone(&self.pages)).await?;

        let mut pages = self.pages.write().await;
        pages.insert(target_id, page.clone());

        Ok(page)
    }

    pub async fn get_page(&self, target_id: &str) -> Option<Page> {
        let pages = self.pages.read().await;
        pages.get(target_id).cloned()
    }

    pub async fn close_page(&mut self, target_id: &str) -> BrowserResult<()> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let params = serde_json::json!({
            "targetId": target_id
        });

        conn.send_command(
            &build_method(CdpDomain::Target, "closeTarget"),
            Some(params),
        ).await?;

        let mut pages = self.pages.write().await;
        pages.remove(target_id);

        Ok(())
    }

    pub async fn version(&self) -> BrowserResult<String> {
        let conn = self.connection.as_ref()
            .ok_or_else(|| BrowserError::ConnectionError("Not connected".to_string()))?;

        let response = conn.send_command(
            &build_method(CdpDomain::Browser, "getVersion"),
            None,
        ).await?;

        let version = response.result
            .as_ref()
            .and_then(|r| r.get("protocolVersion"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(version)
    }

    pub fn cdp_url(&self) -> &str {
        &self.cdp_url
    }
}

pub struct BrowserPool {
    browsers: HashMap<String, BrowserManager>,
}

impl BrowserPool {
    pub fn new() -> Self {
        Self {
            browsers: HashMap::new(),
        }
    }

    pub async fn add_browser(&mut self, name: String, browser: BrowserManager) {
        self.browsers.insert(name, browser);
    }

    pub async fn get(&self, name: &str) -> Option<&BrowserManager> {
        self.browsers.get(name)
    }

    pub async fn get_mut(&mut self, name: &str) -> Option<&mut BrowserManager> {
        self.browsers.get_mut(name)
    }

    pub async fn remove(&mut self, name: &str) -> Option<BrowserManager> {
        self.browsers.remove(name)
    }

    pub async fn list(&self) -> Vec<String> {
        self.browsers.keys().cloned().collect()
    }
}

impl Default for BrowserPool {
    fn default() -> Self {
        Self::new()
    }
}
