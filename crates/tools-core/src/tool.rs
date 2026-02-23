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
        }
    }

    pub fn parameters(&self) -> serde_json::Value {
        match self {
            Tool::Bash(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command to execute"
                    },
                    "timeout_seconds": {
                        "type": "number",
                        "description": "Maximum time to wait for completion (default: 30)"
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Working directory for the command"
                    },
                    "env": {
                        "type": "object",
                        "description": "Environment variables to set"
                    }
                },
                "required": ["command"]
            }),
            Tool::ReadFile(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read"
                    }
                },
                "required": ["path"]
            }),
            Tool::WriteFile(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
            Tool::ListDir(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the directory to list"
                    }
                },
                "required": ["path"]
            }),
            Tool::WebFetch(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL to fetch" },
                    "headers": { "type": "object", "description": "Optional HTTP headers" }
                },
                "required": ["url"]
            }),
            Tool::Memory(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["get", "set", "delete", "list"], "description": "Memory operation" },
                    "key": { "type": "string", "description": "Memory key" },
                    "value": { "type": "string", "description": "Value to store (for set)" }
                },
                "required": ["action"]
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
        Self { tools }
    }

    pub fn register(&mut self, tool: Tool) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.tools.get(name)
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
