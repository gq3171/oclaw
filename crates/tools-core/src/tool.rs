use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::ToolResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResponse {
    pub id: String,
    pub result: serde_json::Value,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Tool {
    Bash(BashTool),
    ReadFile(ReadFileTool),
    WriteFile(WriteFileTool),
    ListDir(ListDirTool),
    WebFetch(WebFetchTool),
    Memory(MemoryTool),
    Browse(BrowseTool),
    WebSearch(WebSearchTool),
    LinkReader(LinkReaderTool),
    MediaDescribe(MediaDescribeTool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashTool {
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    30
}

impl Tool {
    pub fn name(&self) -> &str {
        match self {
            Tool::Bash(_) => "bash",
            Tool::ReadFile(_) => "read_file",
            Tool::WriteFile(_) => "write_file",
            Tool::ListDir(_) => "list_dir",
            Tool::WebFetch(_) => "web_fetch",
            Tool::Memory(_) => "memory",
            Tool::Browse(_) => "browse",
            Tool::WebSearch(_) => "web_search",
            Tool::LinkReader(_) => "link_reader",
            Tool::MediaDescribe(_) => "media_describe",
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Tool::Bash(_) => "Execute a shell command and return the output",
            Tool::ReadFile(_) => "Read contents of a file",
            Tool::WriteFile(_) => "Write content to a file",
            Tool::ListDir(_) => "List contents of a directory",
            Tool::WebFetch(_) => "Fetch content from a URL via HTTP GET",
            Tool::Memory(_) => "Store and retrieve key-value memory entries",
            Tool::Browse(_) => "Navigate to a URL using a browser, render JavaScript, and return the page text content",
            Tool::WebSearch(_) => "Search the web and return a list of results with titles, URLs, and snippets",
            Tool::LinkReader(_) => "Fetch a URL and extract its main text content",
            Tool::MediaDescribe(_) => "Describe an image from a URL using vision capabilities",
        }
    }

    pub fn parameters(&self) -> serde_json::Value {
        match self {
            Tool::Bash(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
            Tool::ReadFile(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
            Tool::WriteFile(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
            Tool::ListDir(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
            Tool::WebFetch(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"]
            }),
            Tool::Memory(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["get", "set", "delete", "list"] },
                    "key": { "type": "string" },
                    "value": { "type": "string" }
                },
                "required": ["action"]
            }),
            Tool::Browse(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"]
            }),
            Tool::WebSearch(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "max_results": { "type": "integer", "description": "Max results (default 5)" }
                },
                "required": ["query"]
            }),
            Tool::LinkReader(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to read" },
                    "max_chars": { "type": "integer", "description": "Max content chars (default 6000)" }
                },
                "required": ["url"]
            }),
            Tool::MediaDescribe(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Image URL to describe" },
                    "prompt": { "type": "string", "description": "What to describe (default: general description)" }
                },
                "required": ["url"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        match self {
            Tool::Bash(tool) => tool.execute(arguments).await,
            Tool::ReadFile(tool) => tool.execute(arguments).await,
            Tool::WriteFile(tool) => tool.execute(arguments).await,
            Tool::ListDir(tool) => tool.execute(arguments).await,
            Tool::WebFetch(tool) => tool.execute(arguments).await,
            Tool::Memory(tool) => tool.execute(arguments).await,
            Tool::Browse(tool) => tool.execute(arguments).await,
            Tool::WebSearch(tool) => tool.execute(arguments).await,
            Tool::LinkReader(tool) => tool.execute(arguments).await,
            Tool::MediaDescribe(tool) => tool.execute(arguments).await,
        }
    }
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            timeout_seconds: 30,
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        use std::collections::HashMap;
        use std::process::Stdio;
        use tokio::process::Command;
        use tokio::time::{timeout, Duration};

        #[derive(Deserialize)]
        struct BashArguments {
            command: String,
            #[serde(default)]
            timeout_seconds: Option<u64>,
            #[serde(default)]
            working_dir: Option<String>,
            #[serde(default)]
            env: Option<HashMap<String, String>>,
        }

        let args: BashArguments = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let timeout_secs = args.timeout_seconds.unwrap_or(self.timeout_seconds);
        let timeout_dur = Duration::from_secs(timeout_secs);

        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", &args.command]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", &args.command]);
            c
        };

        if let Some(dir) = args.working_dir {
            cmd.current_dir(dir);
        }

        if let Some(env_vars) = args.env {
            for (key, value) in env_vars {
                cmd.env(&key, &value);
            }
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let result = timeout(timeout_dur, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                #[derive(Serialize)]
                struct BashOutput {
                    stdout: String,
                    stderr: String,
                    exit_code: Option<i32>,
                    timed_out: bool,
                }
                let output = BashOutput {
                    stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                    exit_code: output.status.code(),
                    timed_out: false,
                };
                Ok(serde_json::to_value(output)?)
            }
            Ok(Err(e)) => Err(crate::ToolError::ExecutionFailed(e.to_string())),
            Err(_) => {
                #[derive(Serialize)]
                struct BashOutput {
                    stdout: String,
                    stderr: String,
                    exit_code: Option<i32>,
                    timed_out: bool,
                }
                let output = BashOutput {
                    stdout: String::new(),
                    stderr: "Command timed out".to_string(),
                    exit_code: None,
                    timed_out: true,
                };
                Ok(serde_json::to_value(output)?)
            }
        }
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ToolRegistry {
    tools: HashMap<String, Tool>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        tools.insert("bash".to_string(), Tool::Bash(BashTool::new()));
        tools.insert("read_file".to_string(), Tool::ReadFile(ReadFileTool::new()));
        tools.insert("write_file".to_string(), Tool::WriteFile(WriteFileTool::new()));
        tools.insert("list_dir".to_string(), Tool::ListDir(ListDirTool::new()));
        tools.insert("web_fetch".to_string(), Tool::WebFetch(WebFetchTool::new()));
        tools.insert("memory".to_string(), Tool::Memory(MemoryTool::new()));
        tools.insert("browse".to_string(), Tool::Browse(BrowseTool::new()));
        tools.insert("web_search".to_string(), Tool::WebSearch(WebSearchTool::new()));
        tools.insert("link_reader".to_string(), Tool::LinkReader(LinkReaderTool::new()));
        tools.insert("media_describe".to_string(), Tool::MediaDescribe(MediaDescribeTool::new()));
        Self { tools }
    }

    pub fn configure_browser(&mut self, cdp_url: Option<&str>, executable_path: Option<&str>, headless: Option<bool>) {
        let mut browse = BrowseTool::new();
        if let Some(url) = cdp_url { browse.cdp_url = Some(url.to_string()); }
        if let Some(exe) = executable_path { browse.executable_path = Some(exe.to_string()); }
        if let Some(h) = headless { browse.headless = Some(h); }
        self.tools.insert("browse".to_string(), Tool::Browse(browse));
    }

    pub fn register(&mut self, tool: Tool) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.tools.get(name)
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn list(&self) -> Vec<serde_json::Value> {
        self.tools.values().map(|t| {
            serde_json::json!({
                "name": t.name(),
                "description": t.description(),
                "parameters": t.parameters(),
            })
        }).collect()
    }

    /// Return only essential tools for LLM function calling (reduces token usage).
    pub fn list_for_llm(&self) -> Vec<serde_json::Value> {
        let essential = ["bash", "web_fetch", "web_search", "browse", "memory", "link_reader", "media_describe"];
        self.tools.values()
            .filter(|t| essential.contains(&t.name()))
            .map(|t| serde_json::json!({
                "name": t.name(),
                "description": t.description(),
                "parameters": t.parameters(),
            }))
            .collect()
    }

    pub async fn execute_call(&self, call: ToolCall) -> ToolResponse {
        let tool = self.get(&call.name);
        
        match tool {
            Some(t) => {
                let result: ToolResult<serde_json::Value> = t.execute(call.arguments).await;
                match result {
                    Ok(result) => ToolResponse {
                        id: call.id,
                        result,
                        error: None,
                    },
                    Err(e) => ToolResponse {
                        id: call.id,
                        result: serde_json::Value::Null,
                        error: Some(e.to_string()),
                    },
                }
            }
            None => ToolResponse {
                id: call.id,
                result: serde_json::Value::Null,
                error: Some(format!("Tool not found: {}", call.name)),
            },
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileTool {
    pub max_size_bytes: Option<u64>,
}

impl ReadFileTool {
    pub fn new() -> Self {
        Self { max_size_bytes: Some(1024 * 1024) }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct ReadFileArgs {
            path: String,
        }

        let args: ReadFileArgs = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let content = tokio::fs::read_to_string(&args.path).await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Failed to read file: {}", e)))?;

        if let Some(max_size) = self.max_size_bytes
            && content.len() > max_size as usize
        {
            return Err(crate::ToolError::ExecutionFailed("File too large".to_string()));
        }

        Ok(serde_json::json!({
            "path": args.path,
            "content": content,
            "size": content.len()
        }))
    }
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileTool {
    pub create_parents: bool,
}

impl WriteFileTool {
    pub fn new() -> Self {
        Self { create_parents: true }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct WriteFileArgs {
            path: String,
            content: String,
        }

        let args: WriteFileArgs = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        if self.create_parents
            && let Some(parent) = std::path::Path::new(&args.path).parent()
        {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| crate::ToolError::ExecutionFailed(format!("Failed to create directory: {}", e)))?;
        }

        tokio::fs::write(&args.path, &args.content).await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Failed to write file: {}", e)))?;

        Ok(serde_json::json!({
            "path": args.path,
            "bytes_written": args.content.len()
        }))
    }
}

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDirTool {
    pub include_hidden: bool,
}

impl ListDirTool {
    pub fn new() -> Self {
        Self { include_hidden: true }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct ListDirArgs {
            path: String,
        }

        let args: ListDirArgs = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&args.path).await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Failed to read directory: {}", e)))?;

        while let Some(entry) = dir.next_entry().await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Failed to read entry: {}", e)))? {
            
            let file_name = entry.file_name().to_string_lossy().to_string();
            
            if !self.include_hidden && file_name.starts_with('.') {
                continue;
            }

            let metadata = entry.metadata().await
                .map_err(|e| crate::ToolError::ExecutionFailed(format!("Failed to get metadata: {}", e)))?;

            entries.push(serde_json::json!({
                "name": file_name,
                "is_file": metadata.is_file(),
                "is_dir": metadata.is_dir(),
                "size": metadata.len(),
            }));
        }

        Ok(serde_json::json!({
            "path": args.path,
            "entries": entries,
            "count": entries.len()
        }))
    }
}

impl Default for ListDirTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchTool {
    pub timeout_seconds: u64,
    pub max_body_bytes: usize,
}

impl WebFetchTool {
    pub fn new() -> Self {
        Self { timeout_seconds: 30, max_body_bytes: 1024 * 1024 }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            url: String,
            #[serde(default)]
            headers: Option<HashMap<String, String>>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let mut req = client.get(&args.url);
        if let Some(hdrs) = args.headers {
            for (k, v) in hdrs {
                req = req.header(&k, &v);
            }
        }

        let resp = req.send().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let status = resp.status().as_u16();
        let body = resp.text().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let truncated = body.len() > self.max_body_bytes;
        let body = if truncated { &body[..self.max_body_bytes] } else { &body };

        Ok(serde_json::json!({
            "url": args.url,
            "status": status,
            "body": body,
            "truncated": truncated
        }))
    }
}

impl Default for WebFetchTool {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryTool {
    #[serde(skip)]
    store: std::sync::Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl MemoryTool {
    pub fn new() -> Self {
        Self { store: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())) }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            key: Option<String>,
            value: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let mut store = self.store.lock().unwrap();
        match args.action.as_str() {
            "get" => {
                let key = args.key.ok_or_else(|| crate::ToolError::InvalidInput("key required".into()))?;
                let val = store.get(&key).cloned();
                Ok(serde_json::json!({ "key": key, "value": val }))
            }
            "set" => {
                let key = args.key.ok_or_else(|| crate::ToolError::InvalidInput("key required".into()))?;
                let val = args.value.unwrap_or_default();
                store.insert(key.clone(), val.clone());
                Ok(serde_json::json!({ "key": key, "value": val }))
            }
            "delete" => {
                let key = args.key.ok_or_else(|| crate::ToolError::InvalidInput("key required".into()))?;
                let removed = store.remove(&key);
                Ok(serde_json::json!({ "key": key, "removed": removed.is_some() }))
            }
            "list" => {
                let keys: Vec<&String> = store.keys().collect();
                Ok(serde_json::json!({ "keys": keys, "count": keys.len() }))
            }
            other => Err(crate::ToolError::InvalidInput(format!("Unknown action: {}", other))),
        }
    }
}

impl Default for MemoryTool {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseTool {
    pub cdp_url: Option<String>,
    pub executable_path: Option<String>,
    pub headless: Option<bool>,
    pub timeout_seconds: u64,
    /// Track a child browser process we launched (PID).
    #[serde(skip)]
    launched_pid: std::sync::Arc<std::sync::Mutex<Option<u32>>>,
}

impl BrowseTool {
    pub fn new() -> Self {
        Self {
            cdp_url: None,
            executable_path: None,
            headless: None,
            timeout_seconds: 30,
            launched_pid: Default::default(),
        }
    }

    pub fn with_cdp_url(mut self, url: &str) -> Self {
        self.cdp_url = Some(url.to_string());
        self
    }

    pub fn with_executable(mut self, path: &str) -> Self {
        self.executable_path = Some(path.to_string());
        self
    }

    pub fn with_headless(mut self, headless: bool) -> Self {
        self.headless = Some(headless);
        self
    }

    /// Detect browser executable: config > Edge > Chrome
    fn detect_browser(&self) -> Option<String> {
        if let Some(ref p) = self.executable_path
            && std::path::Path::new(p).exists()
        {
            return Some(p.clone());
        }

        let candidates = if cfg!(windows) {
            vec![
                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            ]
        } else if cfg!(target_os = "macos") {
            vec![
                "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ]
        } else {
            vec![
                "/usr/bin/microsoft-edge",
                "/usr/bin/microsoft-edge-stable",
                "/usr/bin/google-chrome",
                "/usr/bin/google-chrome-stable",
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
            ]
        };

        candidates.into_iter()
            .find(|p| std::path::Path::new(p).exists())
            .map(|s| s.to_string())
    }

    /// Try connecting to CDP; if fails, auto-launch browser then retry.
    async fn ensure_browser(&self) -> Result<oclaws_browser_core::BrowserManager, crate::ToolError> {
        let cdp_url = self.cdp_url.as_deref().unwrap_or("http://127.0.0.1:9222");

        // Try connecting first
        if let Ok(mgr) = oclaws_browser_core::BrowserManager::new(cdp_url).await {
            return Ok(mgr);
        }

        // Auto-launch browser
        let exe = self.detect_browser().ok_or_else(|| {
            crate::ToolError::ExecutionFailed(
                "No browser found. Install Edge or Chrome, or set browser.executablePath in config.".into()
            )
        })?;

        tracing::info!("Auto-launching browser: {}", exe);

        let port = cdp_url.split(':').next_back()
            .and_then(|s| s.trim_matches('/').parse::<u16>().ok())
            .unwrap_or(9222);

        let mut cmd = tokio::process::Command::new(&exe);
        cmd.arg(format!("--remote-debugging-port={}", port));
        if self.headless.unwrap_or(false) {
            cmd.arg("--headless=new");
        }
        cmd.arg("--no-first-run");
        cmd.arg("--no-default-browser-check");
        cmd.arg("--disable-gpu");
        cmd.arg("--disable-sync");
        cmd.arg("--disable-background-networking");
        cmd.arg("--disable-component-update");
        cmd.arg("--disable-session-crashed-bubble");
        cmd.arg("--hide-crash-restore-bubble");
        let user_data_dir = std::env::temp_dir().join("oclaw-browser-profile");
        cmd.arg(format!("--user-data-dir={}", user_data_dir.display()));
        cmd.arg("about:blank");

        #[cfg(windows)]
        cmd.creation_flags(0x00000008); // DETACHED_PROCESS
        let child = cmd
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Failed to launch browser: {}", e)))?;

        if let Some(pid) = child.id() {
            *self.launched_pid.lock().unwrap() = Some(pid);
        }

        // Wait for CDP to become available
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            if let Ok(mgr) = oclaws_browser_core::BrowserManager::new(cdp_url).await {
                return Ok(mgr);
            }
        }

        Err(crate::ToolError::ExecutionFailed(format!(
            "Browser launched but CDP not available at {} after 6s", cdp_url
        )))
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            url: String,
            #[serde(default)]
            wait_ms: Option<u64>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let mut manager = self.ensure_browser().await?;

        let mut page = manager.create_page().await.map_err(|e| {
            crate::ToolError::ExecutionFailed(format!("Failed to create page: {}", e))
        })?;

        page.navigate(&args.url).await.map_err(|e| {
            crate::ToolError::ExecutionFailed(format!("Navigation failed: {}", e))
        })?;

        let wait = args.wait_ms.unwrap_or(2000);
        tokio::time::sleep(std::time::Duration::from_millis(wait)).await;

        let html_len = page.get_html().await.map(|h| h.len()).unwrap_or(0);

        let text_obj = page.evaluate("document.body.innerText").await;
        let text_str = text_obj.as_ref().ok()
            .and_then(|r| r.value.as_ref())
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let title_obj = page.evaluate("document.title").await;
        let title_str = title_obj.as_ref().ok()
            .and_then(|r| r.value.as_ref())
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let max_len = 8000;
        let truncated = text_str.len() > max_len;
        let content = if truncated { &text_str[..max_len] } else { text_str };

        let result = serde_json::json!({
            "url": args.url,
            "title": title_str,
            "content": content,
            "html_length": html_len,
            "truncated": truncated
        });

        page.close().await.ok();
        manager.disconnect().await.ok();

        Ok(result)
    }
}

impl Default for BrowseTool {
    fn default() -> Self { Self::new() }
}

// --- WebSearchTool: DuckDuckGo HTML scraping (no API key needed) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchTool {
    pub timeout_seconds: u64,
}

impl WebSearchTool {
    pub fn new() -> Self { Self { timeout_seconds: 15 } }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args { query: String, #[serde(default)] max_results: Option<usize> }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;
        let max = args.max_results.unwrap_or(5).min(10);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .build().map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let resp = client.get("https://html.duckduckgo.com/html/")
            .query(&[("q", &args.query)])
            .header("User-Agent", "Mozilla/5.0 (compatible; oclaw/1.0)")
            .send().await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Search request failed: {}", e)))?;

        let html = resp.text().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let results = parse_ddg_results(&html, max);
        Ok(serde_json::json!({ "query": args.query, "results": results }))
    }
}

impl Default for WebSearchTool { fn default() -> Self { Self::new() } }

fn parse_ddg_results(html: &str, max: usize) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    // Parse DuckDuckGo HTML result blocks: <a class="result__a" href="...">title</a>
    // and <a class="result__snippet">snippet</a>
    let mut pos = 0;
    while results.len() < max {
        // Find result link
        let link_marker = "class=\"result__a\"";
        let Some(link_start) = html[pos..].find(link_marker) else { break };
        let link_start = pos + link_start;

        // Extract href
        let before = &html[link_start.saturating_sub(200)..link_start];
        let href = extract_attr(before, "href").unwrap_or_default();

        // Extract title text
        let after_tag = link_start + link_marker.len();
        let title = extract_tag_text(&html[after_tag..]).unwrap_or_default();

        // Find snippet
        let snippet_marker = "class=\"result__snippet\"";
        let snippet = if let Some(spos) = html[after_tag..].find(snippet_marker) {
            let s = after_tag + spos + snippet_marker.len();
            extract_tag_text(&html[s..]).unwrap_or_default()
        } else { String::new() };

        // Decode DuckDuckGo redirect URL
        let url = if href.contains("uddg=") {
            href.split("uddg=").nth(1)
                .and_then(|u| urlencoding::decode(u.split('&').next().unwrap_or(u)).ok())
                .map(|s| s.into_owned())
                .unwrap_or(href)
        } else { href };

        if !url.is_empty() && !title.is_empty() {
            results.push(serde_json::json!({
                "title": strip_html_tags(&title),
                "url": url,
                "snippet": strip_html_tags(&snippet),
            }));
        }
        pos = after_tag + 1;
    }
    results
}

fn extract_attr(before: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = before.rfind(&pattern)? + pattern.len();
    let end = before[start..].find('"')? + start;
    Some(before[start..end].to_string())
}

fn extract_tag_text(html: &str) -> Option<String> {
    let start = html.find('>')? + 1;
    let end = html[start..].find('<')? + start;
    Some(html[start..end].trim().to_string())
}

fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

// --- LinkReaderTool: fetch URL, strip HTML, return text ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkReaderTool {
    pub timeout_seconds: u64,
}

impl LinkReaderTool {
    pub fn new() -> Self { Self { timeout_seconds: 20 } }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args { url: String, #[serde(default)] max_chars: Option<usize> }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;
        let max_chars = args.max_chars.unwrap_or(6000);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build().map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let resp = client.get(&args.url)
            .header("User-Agent", "Mozilla/5.0 (compatible; oclaw/1.0)")
            .send().await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Fetch failed: {}", e)))?;

        let status = resp.status().as_u16();
        let content_type = resp.headers().get("content-type")
            .and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let body = resp.text().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let text = if content_type.contains("html") {
            html_to_text(&body)
        } else {
            body
        };

        let truncated = text.len() > max_chars;
        let content = if truncated { &text[..max_chars] } else { &text };

        Ok(serde_json::json!({
            "url": args.url, "status": status,
            "content": content, "truncated": truncated,
            "content_type": content_type,
        }))
    }
}

impl Default for LinkReaderTool { fn default() -> Self { Self::new() } }

fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 3);
    let mut in_tag = false;
    let mut in_script = false;
    let mut last_was_space = false;
    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            // Check for script/style start
            let rest = &html[html.len().min(out.len())..];
            if rest.len() > 7 {
                let lower: String = rest.chars().take(7).collect();
                if lower.starts_with("script") || lower.starts_with("style") {
                    in_script = true;
                }
            }
            continue;
        }
        if c == '>' { in_tag = false; continue; }
        if in_tag { continue; }
        if in_script {
            // Look for closing tag
            if c == '/' { in_script = false; }
            continue;
        }
        if c.is_whitespace() {
            if !last_was_space { out.push(' '); last_was_space = true; }
        } else {
            out.push(c); last_was_space = false;
        }
    }
    out.trim().to_string()
}

// --- MediaDescribeTool: describe image via HTTP download + base64 for vision API ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaDescribeTool {
    pub timeout_seconds: u64,
}

impl MediaDescribeTool {
    pub fn new() -> Self { Self { timeout_seconds: 30 } }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            url: String,
            #[serde(default)]
            prompt: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .build().map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let resp = client.get(&args.url)
            .send().await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Image fetch failed: {}", e)))?;

        let content_type = resp.headers().get("content-type")
            .and_then(|v| v.to_str().ok()).unwrap_or("image/jpeg").to_string();
        let bytes = resp.bytes().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let prompt = args.prompt.unwrap_or_else(|| "Describe this image in detail.".into());

        Ok(serde_json::json!({
            "url": args.url,
            "content_type": content_type,
            "size_bytes": bytes.len(),
            "base64": b64,
            "prompt": prompt,
            "note": "Image downloaded. Use the base64 data with a vision-capable model to get a description."
        }))
    }
}

impl Default for MediaDescribeTool { fn default() -> Self { Self::new() } }
