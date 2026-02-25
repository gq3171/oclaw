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
    Cron(CronTool),
    Message(MessageTool),
    SessionsList(SessionsListTool),
    SessionsHistory(SessionsHistoryTool),
    SessionsSend(SessionsSendTool),
    SessionsSpawn(SessionsSpawnTool),
    Subagents(SubagentsTool),
    SessionStatus(SessionStatusTool),
    Tts(TtsTool),
    Workspace(WorkspaceTool),
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
            Tool::Cron(_) => "cron",
            Tool::Message(_) => "message",
            Tool::SessionsList(_) => "sessions_list",
            Tool::SessionsHistory(_) => "sessions_history",
            Tool::SessionsSend(_) => "sessions_send",
            Tool::SessionsSpawn(_) => "sessions_spawn",
            Tool::Subagents(_) => "subagents",
            Tool::SessionStatus(_) => "session_status",
            Tool::Tts(_) => "tts",
            Tool::Workspace(_) => "workspace",
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
            Tool::Browse(_) => "Browser automation: navigate, click, type, screenshot, evaluate JS, get DOM snapshot, console logs, and network requests",
            Tool::WebSearch(_) => "Search the web and return a list of results with titles, URLs, and snippets",
            Tool::LinkReader(_) => "Fetch a URL and extract its main text content",
            Tool::MediaDescribe(_) => "Describe an image from a URL using vision capabilities",
            Tool::Cron(_) => "Manage scheduled cron jobs: list, add, update, remove, run, status",
            Tool::Message(_) => "Send a message to a channel target",
            Tool::SessionsList(_) => "List active sessions with optional filters",
            Tool::SessionsHistory(_) => "Retrieve message history for a session",
            Tool::SessionsSend(_) => "Send a message into an existing session",
            Tool::SessionsSpawn(_) => "Spawn a new sub-session with an agent",
            Tool::Subagents(_) => "Manage running subagents: list, kill, steer",
            Tool::SessionStatus(_) => "Get current session status and metadata",
            Tool::Tts(_) => "Convert text to speech audio",
            Tool::Workspace(_) => "Read, write, append, or list files in the agent workspace (SOUL.md, IDENTITY.md, HEARTBEAT.md, memory/). Use this to evolve your personality and store durable memories.",
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
                    "action": { "type": "string", "enum": ["navigate", "click", "type", "screenshot", "evaluate", "snapshot", "console", "network", "back", "forward", "reload"], "description": "Browser action to perform (default: navigate)" },
                    "url": { "type": "string", "description": "URL to navigate to (for navigate action)" },
                    "selector": { "type": "string", "description": "CSS selector (for click/type actions)" },
                    "text": { "type": "string", "description": "Text to type (for type action)" },
                    "expression": { "type": "string", "description": "JavaScript expression (for evaluate action)" },
                    "wait_ms": { "type": "integer", "description": "Wait time in ms after action (default 1000)" }
                },
                "required": ["action"]
            }),
            Tool::WebSearch(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "max_results": { "type": "integer", "description": "Max results (default 5)" },
                    "provider": { "type": "string", "enum": ["auto", "brave", "perplexity", "duckduckgo"], "description": "Search provider (default: auto)" }
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
            Tool::Cron(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "add", "update", "remove", "run", "status"], "description": "Cron action to perform" },
                    "job_id": { "type": "string", "description": "Job ID (for update/remove/run/status)" },
                    "schedule": { "type": "string", "description": "Cron expression (for add/update)" },
                    "command": { "type": "string", "description": "Command to execute (for add/update)" },
                    "label": { "type": "string", "description": "Human-readable label (for add/update)" },
                    "enabled": { "type": "boolean", "description": "Whether job is enabled (for add/update)" }
                },
                "required": ["action"]
            }),
            Tool::Message(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "channel": { "type": "string", "description": "Channel type (telegram, discord, etc.)" },
                    "target": { "type": "string", "description": "Target chat/user ID" },
                    "text": { "type": "string", "description": "Message text to send" },
                    "reply_to": { "type": "string", "description": "Message ID to reply to" }
                },
                "required": ["channel", "target", "text"]
            }),
            Tool::SessionsList(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "channel": { "type": "string", "description": "Filter by channel" },
                    "limit": { "type": "integer", "description": "Max results (default 20)" },
                    "active_only": { "type": "boolean", "description": "Only show active sessions" }
                }
            }),
            Tool::SessionsHistory(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "session_key": { "type": "string", "description": "Session key to get history for" },
                    "limit": { "type": "integer", "description": "Max messages (default 50)" },
                    "before": { "type": "string", "description": "Cursor for pagination" }
                },
                "required": ["session_key"]
            }),
            Tool::SessionsSend(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "session_key": { "type": "string", "description": "Target session key" },
                    "text": { "type": "string", "description": "Message text" },
                    "role": { "type": "string", "enum": ["user", "system"], "description": "Message role (default: user)" }
                },
                "required": ["session_key", "text"]
            }),
            Tool::SessionsSpawn(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Agent to use for the sub-session" },
                    "prompt": { "type": "string", "description": "Initial prompt for the sub-session" },
                    "parent_session_key": { "type": "string", "description": "Parent session key" }
                },
                "required": ["agent_id", "prompt"]
            }),
            Tool::Subagents(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "kill", "steer"], "description": "Subagent action" },
                    "session_key": { "type": "string", "description": "Session key of the subagent (for kill/steer)" },
                    "message": { "type": "string", "description": "Steering message (for steer)" }
                },
                "required": ["action"]
            }),
            Tool::SessionStatus(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "session_key": { "type": "string", "description": "Session key (default: current session)" }
                }
            }),
            Tool::Tts(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to convert to speech" },
                    "provider": { "type": "string", "enum": ["openai", "elevenlabs", "edge"], "description": "TTS provider (default: auto)" },
                    "voice": { "type": "string", "description": "Voice name/ID" },
                    "model": { "type": "string", "description": "Model name (provider-specific)" }
                },
                "required": ["text"]
            }),
            Tool::Workspace(_) => serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["read", "write", "append", "list"], "description": "Action to perform on workspace files" },
                    "path": { "type": "string", "description": "Relative path within workspace (e.g. SOUL.md, IDENTITY.md, memory/2026-02-25.md)" },
                    "content": { "type": "string", "description": "Content to write or append (for write/append actions)" }
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
            Tool::Browse(tool) => tool.execute(arguments).await,
            Tool::WebSearch(tool) => tool.execute(arguments).await,
            Tool::LinkReader(tool) => tool.execute(arguments).await,
            Tool::MediaDescribe(tool) => tool.execute(arguments).await,
            Tool::Cron(tool) => tool.execute(arguments).await,
            Tool::Message(tool) => tool.execute(arguments).await,
            Tool::SessionsList(tool) => tool.execute(arguments).await,
            Tool::SessionsHistory(tool) => tool.execute(arguments).await,
            Tool::SessionsSend(tool) => tool.execute(arguments).await,
            Tool::SessionsSpawn(tool) => tool.execute(arguments).await,
            Tool::Subagents(tool) => tool.execute(arguments).await,
            Tool::SessionStatus(tool) => tool.execute(arguments).await,
            Tool::Tts(tool) => tool.execute(arguments).await,
            Tool::Workspace(tool) => tool.execute(arguments).await,
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
            // Block dangerous environment variables that could hijack execution
            const BLOCKED_ENV: &[&str] = &[
                "LD_PRELOAD", "LD_LIBRARY_PATH", "DYLD_INSERT_LIBRARIES",
                "DYLD_LIBRARY_PATH", "DYLD_FRAMEWORK_PATH",
                "PATH", "HOME", "SHELL", "USER", "LOGNAME",
                "BASH_ENV", "ENV", "CDPATH", "GLOBIGNORE",
                "BASH_FUNC_", "PS4", "PROMPT_COMMAND",
                "PYTHONSTARTUP", "PERL5OPT", "RUBYOPT", "NODE_OPTIONS",
                "JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "CLASSPATH",
                "GIT_SSH_COMMAND", "http_proxy", "https_proxy", "CURL_CA_BUNDLE",
            ];
            for (key, value) in env_vars {
                let key_upper = key.to_uppercase();
                let is_blocked = BLOCKED_ENV.iter().any(|b| {
                    key_upper == *b || key_upper.starts_with("LD_") || key_upper.starts_with("DYLD_")
                });
                if is_blocked {
                    return Err(crate::ToolError::InvalidInput(
                        format!("Environment variable '{}' is blocked for security", key),
                    ));
                }
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
        tools.insert("cron".to_string(), Tool::Cron(CronTool::new()));
        tools.insert("message".to_string(), Tool::Message(MessageTool::new()));
        tools.insert("sessions_list".to_string(), Tool::SessionsList(SessionsListTool::new()));
        tools.insert("sessions_history".to_string(), Tool::SessionsHistory(SessionsHistoryTool::new()));
        tools.insert("sessions_send".to_string(), Tool::SessionsSend(SessionsSendTool::new()));
        tools.insert("sessions_spawn".to_string(), Tool::SessionsSpawn(SessionsSpawnTool::new()));
        tools.insert("subagents".to_string(), Tool::Subagents(SubagentsTool::new()));
        tools.insert("session_status".to_string(), Tool::SessionStatus(SessionStatusTool::new()));
        tools.insert("tts".to_string(), Tool::Tts(TtsTool::new()));
        Self { tools }
    }

    pub fn configure_browser(&mut self, cdp_url: Option<&str>, executable_path: Option<&str>, headless: Option<bool>) {
        let mut browse = BrowseTool::new();
        if let Some(url) = cdp_url { browse.cdp_url = Some(url.to_string()); }
        if let Some(exe) = executable_path { browse.executable_path = Some(exe.to_string()); }
        if let Some(h) = headless { browse.headless = Some(h); }
        self.tools.insert("browse".to_string(), Tool::Browse(browse));
    }

    pub fn configure_workspace(&mut self, workspace_root: &str) {
        self.tools.insert(
            "workspace".to_string(),
            Tool::Workspace(WorkspaceTool::new(workspace_root)),
        );
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
        let essential = [
            "bash", "web_fetch", "web_search", "browse", "memory",
            "link_reader", "media_describe", "cron", "message",
            "sessions_list", "sessions_history", "session_status", "tts",
            "workspace",
        ];
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
        Self { timeout_seconds: 30, max_body_bytes: 2 * 1024 * 1024 }
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

        // SSRF guard: block private IPs unless explicitly allowed
        if !Self::is_url_allowed(&args.url) {
            return Err(crate::ToolError::ExecutionFailed(
                "URL targets a private/localhost address (SSRF blocked)".into(),
            ));
        }

        // Use Firecrawl if API key is available
        if let Ok(fc_key) = std::env::var("FIRECRAWL_API_KEY") {
            return self.fetch_firecrawl(&args.url, &fc_key).await;
        }

        // Fallback: direct HTTP fetch
        self.fetch_direct(&args.url, args.headers).await
    }

    fn is_url_allowed(url: &str) -> bool {
        let parsed = match url::Url::parse(url) {
            Ok(u) => u,
            Err(_) => return false,
        };

        // Only allow http/https schemes
        match parsed.scheme() {
            "http" | "https" => {}
            _ => return false,
        }

        let host = match parsed.host_str() {
            Some(h) => h,
            None => return false,
        };

        let blocked = ["localhost", "127.0.0.1", "0.0.0.0", "[::1]", "::1"];
        if blocked.contains(&host) {
            return std::env::var("OCLAWS_ALLOW_PRIVATE_FETCH").is_ok();
        }

        // Block private IP ranges (RFC 1918 + link-local + IPv6 specials)
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            let is_private = match ip {
                std::net::IpAddr::V4(v4) => {
                    v4.is_loopback()
                        || v4.is_private()
                        || v4.is_link_local()
                        || v4.is_unspecified()
                }
                std::net::IpAddr::V6(v6) => {
                    let seg = v6.segments();
                    v6.is_loopback()
                        || v6.is_unspecified()
                        // Link-local fe80::/10
                        || (seg[0] & 0xffc0) == 0xfe80
                        // Unique-local fc00::/7
                        || (seg[0] & 0xfe00) == 0xfc00
                        // Multicast ff00::/8
                        || (seg[0] & 0xff00) == 0xff00
                        // IPv4-mapped ::ffff:0:0/96 — check the mapped v4 address
                        || matches!(v6.to_ipv4_mapped(), Some(v4) if
                            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified())
                }
            };
            if is_private {
                return std::env::var("OCLAWS_ALLOW_PRIVATE_FETCH").is_ok();
            }
        }

        true
    }

    async fn fetch_direct(
        &self,
        url: &str,
        headers: Option<HashMap<String, String>>,
    ) -> ToolResult<serde_json::Value> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let mut req = client.get(url);
        if let Some(hdrs) = headers {
            for (k, v) in hdrs {
                req = req.header(&k, &v);
            }
        }

        let resp = req.send().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let status = resp.status().as_u16();
        let body = resp.text().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let max_chars = 50_000;
        let truncated = body.len() > max_chars;
        let body = if truncated { &body[..max_chars] } else { &body };

        Ok(serde_json::json!({
            "url": url,
            "status": status,
            "body": body,
            "truncated": truncated,
            "backend": "direct"
        }))
    }

    async fn fetch_firecrawl(
        &self,
        url: &str,
        api_key: &str,
    ) -> ToolResult<serde_json::Value> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let body = serde_json::json!({
            "url": url,
            "formats": ["markdown"],
            "onlyMainContent": true
        });

        let resp = client
            .post("https://api.firecrawl.dev/v1/scrape")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(
                format!("Firecrawl request failed: {}", e),
            ))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(crate::ToolError::ExecutionFailed(
                format!("Firecrawl error ({}): {}", status, text),
            ));
        }

        let json: serde_json::Value = resp.json().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let markdown = json["data"]["markdown"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let max_chars = 50_000;
        let truncated = markdown.len() > max_chars;
        let content = if truncated {
            &markdown[..max_chars]
        } else {
            &markdown
        };

        Ok(serde_json::json!({
            "url": url,
            "body": content,
            "truncated": truncated,
            "backend": "firecrawl"
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
    /// Page state tracking (console, errors, network).
    #[serde(skip)]
    state: std::sync::Arc<std::sync::Mutex<oclaws_browser_core::PageState>>,
}

impl BrowseTool {
    pub fn new() -> Self {
        Self {
            cdp_url: None,
            executable_path: None,
            headless: None,
            timeout_seconds: 30,
            launched_pid: Default::default(),
            state: Default::default(),
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
            #[serde(default = "default_action")]
            action: String,
            #[serde(default)]
            url: Option<String>,
            #[serde(default)]
            selector: Option<String>,
            #[serde(default)]
            text: Option<String>,
            #[serde(default)]
            expression: Option<String>,
            #[serde(default)]
            wait_ms: Option<u64>,
        }
        fn default_action() -> String { "navigate".into() }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let mut manager = self.ensure_browser().await?;
        let mut page = manager.create_page().await.map_err(|e| {
            crate::ToolError::ExecutionFailed(format!("Failed to create page: {}", e))
        })?;

        let wait = args.wait_ms.unwrap_or(1000);
        let mut state = self.state.lock().unwrap().clone();

        let result = match args.action.as_str() {
            "navigate" => {
                let url = args.url.as_deref()
                    .ok_or_else(|| crate::ToolError::InvalidInput("url required for navigate".into()))?;
                page.navigate(url).await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Navigation failed: {}", e))
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                state.url = Some(url.to_string());
                let title = eval_string(&page, "document.title").await;
                state.title = Some(title.clone());
                let text = eval_string(&page, "document.body.innerText").await;
                let max_len = 8000;
                let truncated = text.len() > max_len;
                let content = if truncated { &text[..max_len] } else { &text };
                serde_json::json!({ "action": "navigate", "url": url, "title": title, "content": content, "truncated": truncated })
            }
            "click" => {
                let sel = args.selector.as_deref()
                    .ok_or_else(|| crate::ToolError::InvalidInput("selector required for click".into()))?;
                page.click_element(sel).await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Click failed: {}", e))
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                serde_json::json!({ "action": "click", "selector": sel, "ok": true })
            }
            "type" => {
                let sel = args.selector.as_deref()
                    .ok_or_else(|| crate::ToolError::InvalidInput("selector required for type".into()))?;
                let text = args.text.as_deref()
                    .ok_or_else(|| crate::ToolError::InvalidInput("text required for type".into()))?;
                page.type_text(sel, text).await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Type failed: {}", e))
                })?;
                serde_json::json!({ "action": "type", "selector": sel, "ok": true })
            }
            "screenshot" => {
                let bytes = page.take_screenshot().await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Screenshot failed: {}", e))
                })?;
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                serde_json::json!({ "action": "screenshot", "base64": b64, "size_bytes": bytes.len() })
            }
            "evaluate" => {
                let expr = args.expression.as_deref()
                    .ok_or_else(|| crate::ToolError::InvalidInput("expression required for evaluate".into()))?;
                let result = page.evaluate(expr).await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Evaluate failed: {}", e))
                })?;
                serde_json::json!({ "action": "evaluate", "result": result.value })
            }
            "snapshot" => {
                let html = page.get_html().await.unwrap_or_default();
                let title = eval_string(&page, "document.title").await;
                let url = eval_string(&page, "window.location.href").await;
                let max_len = 12000;
                let truncated = html.len() > max_len;
                let content = if truncated { &html[..max_len] } else { &html };
                serde_json::json!({ "action": "snapshot", "url": url, "title": title, "html": content, "html_length": html.len(), "truncated": truncated })
            }
            "console" => {
                let entries: Vec<_> = state.recent_console(50).into_iter().cloned().collect();
                serde_json::json!({ "action": "console", "entries": entries, "count": entries.len() })
            }
            "network" => {
                let entries: Vec<_> = state.recent_requests(50).into_iter().cloned().collect();
                serde_json::json!({ "action": "network", "entries": entries, "count": entries.len() })
            }
            "back" => {
                page.go_back().await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Back failed: {}", e))
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                serde_json::json!({ "action": "back", "ok": true })
            }
            "forward" => {
                page.go_forward().await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Forward failed: {}", e))
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                serde_json::json!({ "action": "forward", "ok": true })
            }
            "reload" => {
                page.reload().await.map_err(|e| {
                    crate::ToolError::ExecutionFailed(format!("Reload failed: {}", e))
                })?;
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                serde_json::json!({ "action": "reload", "ok": true })
            }
            other => {
                return Err(crate::ToolError::InvalidInput(format!("Unknown action: {}", other)));
            }
        };

        *self.state.lock().unwrap() = state;
        page.close().await.ok();
        manager.disconnect().await.ok();

        Ok(result)
    }
}

async fn eval_string(page: &oclaws_browser_core::Page, expr: &str) -> String {
    page.evaluate(expr).await.ok()
        .and_then(|r| r.value)
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
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
        struct Args {
            query: String,
            #[serde(default)]
            max_results: Option<usize>,
            #[serde(default)]
            provider: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;
        let max = args.max_results.unwrap_or(5).min(10);

        let provider = args.provider.as_deref().unwrap_or("auto");
        match provider {
            "brave" => self.search_brave(&args.query, max).await,
            "perplexity" => self.search_perplexity(&args.query).await,
            _ => {
                // Auto: try Brave if key exists, else DuckDuckGo
                if std::env::var("BRAVE_API_KEY").is_ok() {
                    self.search_brave(&args.query, max).await
                } else {
                    self.search_ddg(&args.query, max).await
                }
            }
        }
    }

    async fn search_ddg(&self, query: &str, max: usize) -> ToolResult<serde_json::Value> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .build().map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let resp = client.get("https://html.duckduckgo.com/html/")
            .query(&[("q", query)])
            .header("User-Agent", "Mozilla/5.0 (compatible; oclaw/1.0)")
            .send().await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Search failed: {}", e)))?;

        let html = resp.text().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let results = parse_ddg_results(&html, max);
        Ok(serde_json::json!({ "query": query, "provider": "duckduckgo", "results": results }))
    }

    async fn search_brave(&self, query: &str, max: usize) -> ToolResult<serde_json::Value> {
        let api_key = std::env::var("BRAVE_API_KEY")
            .map_err(|_| crate::ToolError::ExecutionFailed("BRAVE_API_KEY not set".into()))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .build().map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let resp = client.get("https://api.search.brave.com/res/v1/web/search")
            .query(&[("q", query), ("count", &max.to_string())])
            .header("X-Subscription-Token", &api_key)
            .header("Accept", "application/json")
            .send().await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Brave search failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::ToolError::ExecutionFailed(
                format!("Brave API error ({}): {}", status, body)
            ));
        }

        let json: serde_json::Value = resp.json().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let results: Vec<serde_json::Value> = json["web"]["results"]
            .as_array()
            .map(|arr| arr.iter().take(max).map(|r| {
                serde_json::json!({
                    "title": r["title"].as_str().unwrap_or(""),
                    "url": r["url"].as_str().unwrap_or(""),
                    "snippet": r["description"].as_str().unwrap_or(""),
                })
            }).collect())
            .unwrap_or_default();

        Ok(serde_json::json!({
            "query": query,
            "provider": "brave",
            "results": results
        }))
    }

    async fn search_perplexity(&self, query: &str) -> ToolResult<serde_json::Value> {
        let api_key = std::env::var("PERPLEXITY_API_KEY")
            .map_err(|_| crate::ToolError::ExecutionFailed("PERPLEXITY_API_KEY not set".into()))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build().map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let body = serde_json::json!({
            "model": "sonar",
            "messages": [{"role": "user", "content": query}]
        });

        let resp = client.post("https://api.perplexity.ai/chat/completions")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send().await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Perplexity failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::ToolError::ExecutionFailed(
                format!("Perplexity API error ({}): {}", status, body)
            ));
        }

        let json: serde_json::Value = resp.json().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let answer = json["choices"][0]["message"]["content"]
            .as_str().unwrap_or("").to_string();

        Ok(serde_json::json!({
            "query": query,
            "provider": "perplexity",
            "answer": format!("[web_content]{answer}[/web_content]"),
        }))
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
    // Phase 1: strip <script> and <style> blocks from the raw HTML
    let stripped = strip_script_style(html);

    // Phase 2: convert remaining HTML to plain text
    let mut out = String::with_capacity(stripped.len() / 3);
    let mut in_tag = false;
    let mut last_was_space = false;

    for c in stripped.chars() {
        if c == '<' { in_tag = true; continue; }
        if c == '>' { in_tag = false; continue; }
        if in_tag { continue; }
        if c.is_whitespace() {
            if !last_was_space { out.push(' '); last_was_space = true; }
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}

/// Remove all <script>...</script> and <style>...</style> blocks from HTML.
fn strip_script_style(html: &str) -> String {
    let mut result = html.to_string();
    for tag in &["script", "style"] {
        loop {
            let lower = result.to_lowercase();
            let open = format!("<{}", tag);
            let close = format!("</{}>", tag);
            let Some(start) = lower.find(&open) else { break };
            let Some(end_rel) = lower[start..].find(&close) else {
                // No closing tag — remove from open tag to end
                result.truncate(start);
                break;
            };
            let end = start + end_rel + close.len();
            result.replace_range(start..end, "");
        }
    }
    result
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

// --- CronTool: manage scheduled cron jobs ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronTool;

impl CronTool {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            #[serde(default)]
            job_id: Option<String>,
            #[serde(default)]
            schedule: Option<String>,
            #[serde(default)]
            command: Option<String>,
            #[serde(default)]
            label: Option<String>,
            #[serde(default)]
            enabled: Option<bool>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        match args.action.as_str() {
            "list" => {
                Ok(serde_json::json!({ "action": "list", "jobs": [], "note": "Cron store not connected — use gateway RPC for persistent jobs" }))
            }
            "add" => {
                let schedule = args.schedule.ok_or_else(|| crate::ToolError::InvalidInput("schedule required".into()))?;
                let command = args.command.ok_or_else(|| crate::ToolError::InvalidInput("command required".into()))?;
                let id = uuid::Uuid::new_v4().to_string();
                Ok(serde_json::json!({
                    "action": "add", "job_id": id,
                    "schedule": schedule, "command": command,
                    "label": args.label, "enabled": args.enabled.unwrap_or(true)
                }))
            }
            "update" => {
                let job_id = args.job_id.ok_or_else(|| crate::ToolError::InvalidInput("job_id required".into()))?;
                Ok(serde_json::json!({
                    "action": "update", "job_id": job_id,
                    "schedule": args.schedule, "command": args.command,
                    "label": args.label, "enabled": args.enabled
                }))
            }
            "remove" => {
                let job_id = args.job_id.ok_or_else(|| crate::ToolError::InvalidInput("job_id required".into()))?;
                Ok(serde_json::json!({ "action": "remove", "job_id": job_id, "removed": true }))
            }
            "run" => {
                let job_id = args.job_id.ok_or_else(|| crate::ToolError::InvalidInput("job_id required".into()))?;
                Ok(serde_json::json!({ "action": "run", "job_id": job_id, "triggered": true }))
            }
            "status" => {
                let job_id = args.job_id.ok_or_else(|| crate::ToolError::InvalidInput("job_id required".into()))?;
                Ok(serde_json::json!({ "action": "status", "job_id": job_id, "status": "unknown" }))
            }
            other => Err(crate::ToolError::InvalidInput(format!("Unknown cron action: {}", other))),
        }
    }
}

impl Default for CronTool { fn default() -> Self { Self::new() } }

// --- MessageTool: send cross-channel messages ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTool;

impl MessageTool {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            channel: String,
            target: String,
            text: String,
            #[serde(default)]
            reply_to: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        // Message delivery is delegated to the channel adapter at runtime.
        // This tool returns a structured intent that the orchestrator fulfills.
        Ok(serde_json::json!({
            "action": "send_message",
            "channel": args.channel,
            "target": args.target,
            "text": args.text,
            "reply_to": args.reply_to,
            "status": "queued"
        }))
    }
}

impl Default for MessageTool { fn default() -> Self { Self::new() } }

// --- SessionsListTool: list active sessions ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsListTool;

impl SessionsListTool {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            channel: Option<String>,
            #[serde(default)]
            limit: Option<usize>,
            #[serde(default)]
            active_only: Option<bool>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        Ok(serde_json::json!({
            "action": "sessions_list",
            "channel": args.channel,
            "limit": args.limit.unwrap_or(20),
            "active_only": args.active_only.unwrap_or(false),
            "sessions": [],
            "note": "Session store not connected — results populated by orchestrator"
        }))
    }
}

impl Default for SessionsListTool { fn default() -> Self { Self::new() } }

// --- SessionsHistoryTool: retrieve session message history ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsHistoryTool;

impl SessionsHistoryTool {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            session_key: String,
            #[serde(default)]
            limit: Option<usize>,
            #[serde(default)]
            before: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        Ok(serde_json::json!({
            "action": "sessions_history",
            "session_key": args.session_key,
            "limit": args.limit.unwrap_or(50),
            "before": args.before,
            "messages": [],
            "note": "Session store not connected — results populated by orchestrator"
        }))
    }
}

impl Default for SessionsHistoryTool { fn default() -> Self { Self::new() } }

// --- SessionsSendTool: send message into a session ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsSendTool;

impl SessionsSendTool {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            session_key: String,
            text: String,
            #[serde(default)]
            role: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let role = args.role.unwrap_or_else(|| "user".to_string());
        Ok(serde_json::json!({
            "action": "sessions_send",
            "session_key": args.session_key,
            "text": args.text,
            "role": role,
            "status": "queued"
        }))
    }
}

impl Default for SessionsSendTool { fn default() -> Self { Self::new() } }

// --- SessionsSpawnTool: spawn a sub-session ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsSpawnTool;

impl SessionsSpawnTool {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            agent_id: String,
            prompt: String,
            #[serde(default)]
            parent_session_key: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        let session_id = uuid::Uuid::new_v4().to_string();
        Ok(serde_json::json!({
            "action": "sessions_spawn",
            "session_id": session_id,
            "agent_id": args.agent_id,
            "prompt": args.prompt,
            "parent_session_key": args.parent_session_key,
            "status": "spawned"
        }))
    }
}

impl Default for SessionsSpawnTool { fn default() -> Self { Self::new() } }

// --- SubagentsTool: manage running subagents ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentsTool;

impl SubagentsTool {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            #[serde(default)]
            session_key: Option<String>,
            #[serde(default)]
            message: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        match args.action.as_str() {
            "list" => {
                Ok(serde_json::json!({
                    "action": "subagents_list",
                    "subagents": [],
                    "note": "Populated by orchestrator at runtime"
                }))
            }
            "kill" => {
                let key = args.session_key.ok_or_else(|| {
                    crate::ToolError::InvalidInput("session_key required for kill".into())
                })?;
                Ok(serde_json::json!({
                    "action": "subagents_kill",
                    "session_key": key,
                    "status": "killed"
                }))
            }
            "steer" => {
                let key = args.session_key.ok_or_else(|| {
                    crate::ToolError::InvalidInput("session_key required for steer".into())
                })?;
                let msg = args.message.ok_or_else(|| {
                    crate::ToolError::InvalidInput("message required for steer".into())
                })?;
                Ok(serde_json::json!({
                    "action": "subagents_steer",
                    "session_key": key,
                    "message": msg,
                    "status": "steered"
                }))
            }
            other => Err(crate::ToolError::InvalidInput(
                format!("Unknown subagents action: {}", other),
            )),
        }
    }
}

impl Default for SubagentsTool { fn default() -> Self { Self::new() } }

// --- SessionStatusTool: get session status ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusTool;

impl SessionStatusTool {
    pub fn new() -> Self { Self }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            session_key: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        Ok(serde_json::json!({
            "action": "session_status",
            "session_key": args.session_key,
            "status": "unknown",
            "note": "Populated by orchestrator at runtime"
        }))
    }
}

impl Default for SessionStatusTool { fn default() -> Self { Self::new() } }

// --- TtsTool: text-to-speech conversion ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsTool {
    pub default_provider: String,
}

impl TtsTool {
    pub fn new() -> Self {
        Self { default_provider: "openai".to_string() }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            text: String,
            #[serde(default)]
            provider: Option<String>,
            #[serde(default)]
            voice: Option<String>,
            #[serde(default)]
            model: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        if args.text.is_empty() {
            return Err(crate::ToolError::InvalidInput(
                "text must not be empty".into(),
            ));
        }

        let provider = args.provider.unwrap_or_else(|| self.default_provider.clone());

        match provider.as_str() {
            "openai" => self.tts_openai(&args.text, args.voice.as_deref(), args.model.as_deref()).await,
            "elevenlabs" => self.tts_elevenlabs(&args.text, args.voice.as_deref(), args.model.as_deref()).await,
            "edge" => self.tts_edge(&args.text, args.voice.as_deref()).await,
            other => Err(crate::ToolError::InvalidInput(
                format!("Unknown TTS provider: {}", other),
            )),
        }
    }

    async fn tts_openai(
        &self, text: &str, voice: Option<&str>, model: Option<&str>,
    ) -> ToolResult<serde_json::Value> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| crate::ToolError::ExecutionFailed("OPENAI_API_KEY not set".into()))?;

        let voice = voice.unwrap_or("alloy");
        let model = model.unwrap_or("tts-1");

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let body = serde_json::json!({
            "model": model,
            "input": text,
            "voice": voice,
            "response_format": "mp3"
        });

        let resp = client
            .post("https://api.openai.com/v1/audio/speech")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(
                format!("OpenAI TTS request failed: {}", e),
            ))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(crate::ToolError::ExecutionFailed(
                format!("OpenAI TTS error ({}): {}", status, text),
            ));
        }

        let bytes = resp.bytes().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let tmp = std::env::temp_dir().join(format!("oclaw-tts-{}.mp3", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp, &bytes).await
            .map_err(|e| crate::ToolError::ExecutionFailed(
                format!("Failed to write audio: {}", e),
            ))?;

        Ok(serde_json::json!({
            "provider": "openai",
            "voice": voice,
            "model": model,
            "audio_path": tmp.to_string_lossy(),
            "size_bytes": bytes.len(),
            "format": "mp3"
        }))
    }

    async fn tts_elevenlabs(
        &self, text: &str, voice: Option<&str>, model: Option<&str>,
    ) -> ToolResult<serde_json::Value> {
        let api_key = std::env::var("ELEVENLABS_API_KEY")
            .map_err(|_| crate::ToolError::ExecutionFailed("ELEVENLABS_API_KEY not set".into()))?;

        let voice_id = voice.unwrap_or("21m00Tcm4TlvDq8ikWAM");
        let model_id = model.unwrap_or("eleven_monolingual_v1");

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let body = serde_json::json!({
            "text": text,
            "model_id": model_id,
            "voice_settings": { "stability": 0.5, "similarity_boost": 0.75 }
        });

        let url = format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{}",
            voice_id
        );

        let resp = client
            .post(&url)
            .header("xi-api-key", &api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(
                format!("ElevenLabs TTS request failed: {}", e),
            ))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(crate::ToolError::ExecutionFailed(
                format!("ElevenLabs TTS error ({}): {}", status, text),
            ));
        }

        let bytes = resp.bytes().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))?;

        let tmp = std::env::temp_dir().join(format!("oclaw-tts-{}.mp3", uuid::Uuid::new_v4()));
        tokio::fs::write(&tmp, &bytes).await
            .map_err(|e| crate::ToolError::ExecutionFailed(
                format!("Failed to write audio: {}", e),
            ))?;

        Ok(serde_json::json!({
            "provider": "elevenlabs",
            "voice_id": voice_id,
            "model_id": model_id,
            "audio_path": tmp.to_string_lossy(),
            "size_bytes": bytes.len(),
            "format": "mp3"
        }))
    }

    async fn tts_edge(
        &self, text: &str, voice: Option<&str>,
    ) -> ToolResult<serde_json::Value> {
        let voice = voice.unwrap_or("en-US-AriaNeural");
        // Edge TTS uses a local CLI tool (edge-tts) if available
        let tmp = std::env::temp_dir().join(format!("oclaw-tts-{}.mp3", uuid::Uuid::new_v4()));

        let output = tokio::process::Command::new("edge-tts")
            .args(["--voice", voice, "--text", text, "--write-media", &tmp.to_string_lossy()])
            .output()
            .await
            .map_err(|e| crate::ToolError::ExecutionFailed(
                format!("edge-tts not found or failed: {}. Install with: pip install edge-tts", e),
            ))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(crate::ToolError::ExecutionFailed(
                format!("edge-tts failed: {}", stderr),
            ));
        }

        let size = tokio::fs::metadata(&tmp).await
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(serde_json::json!({
            "provider": "edge",
            "voice": voice,
            "audio_path": tmp.to_string_lossy(),
            "size_bytes": size,
            "format": "mp3"
        }))
    }
}

impl Default for TtsTool { fn default() -> Self { Self::new() } }

// --- WorkspaceTool: agent self-modification via workspace files ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceTool {
    /// Root directory of the agent workspace.
    pub workspace_root: String,
}

impl WorkspaceTool {
    pub fn new(workspace_root: impl Into<String>) -> Self {
        Self { workspace_root: workspace_root.into() }
    }

    /// Resolve and validate a relative path within the workspace.
    fn resolve_path(&self, rel: &str) -> Result<std::path::PathBuf, crate::ToolError> {
        let root = std::path::Path::new(&self.workspace_root);
        let target = root.join(rel);

        // Canonicalize what exists, then check prefix
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        // For new files, check the parent
        let check_path = if target.exists() {
            target.canonicalize().unwrap_or_else(|_| target.clone())
        } else if let Some(parent) = target.parent() {
            let p = parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf());
            p.join(target.file_name().unwrap_or_default())
        } else {
            target.clone()
        };

        if !check_path.starts_with(&canonical_root) {
            return Err(crate::ToolError::InvalidInput(
                "Path escapes workspace directory".into(),
            ));
        }
        Ok(target)
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> crate::error::ToolResult<serde_json::Value> {
        #[derive(Deserialize)]
        struct Args {
            action: String,
            #[serde(default)]
            path: Option<String>,
            #[serde(default)]
            content: Option<String>,
        }

        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| crate::ToolError::InvalidInput(e.to_string()))?;

        match args.action.as_str() {
            "read" => self.action_read(args.path).await,
            "write" => self.action_write(args.path, args.content).await,
            "append" => self.action_append(args.path, args.content).await,
            "list" => self.action_list(args.path).await,
            other => Err(crate::ToolError::InvalidInput(
                format!("Unknown workspace action: {}", other),
            )),
        }
    }

    async fn action_read(&self, path: Option<String>) -> crate::error::ToolResult<serde_json::Value> {
        let rel = path.ok_or_else(|| crate::ToolError::InvalidInput("path required for read".into()))?;
        let full = self.resolve_path(&rel)?;
        let content = tokio::fs::read_to_string(&full).await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Read failed: {}", e)))?;
        Ok(serde_json::json!({
            "action": "read", "path": rel,
            "content": content, "size": content.len(),
        }))
    }

    async fn action_write(&self, path: Option<String>, content: Option<String>) -> crate::error::ToolResult<serde_json::Value> {
        let rel = path.ok_or_else(|| crate::ToolError::InvalidInput("path required for write".into()))?;
        let content = content.ok_or_else(|| crate::ToolError::InvalidInput("content required for write".into()))?;
        let full = self.resolve_path(&rel)?;
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| crate::ToolError::ExecutionFailed(format!("mkdir failed: {}", e)))?;
        }
        tokio::fs::write(&full, &content).await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Write failed: {}", e)))?;
        Ok(serde_json::json!({
            "action": "write", "path": rel,
            "bytes_written": content.len(),
        }))
    }

    async fn action_append(&self, path: Option<String>, content: Option<String>) -> crate::error::ToolResult<serde_json::Value> {
        let rel = path.ok_or_else(|| crate::ToolError::InvalidInput("path required for append".into()))?;
        let content = content.ok_or_else(|| crate::ToolError::InvalidInput("content required for append".into()))?;
        let full = self.resolve_path(&rel)?;
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| crate::ToolError::ExecutionFailed(format!("mkdir failed: {}", e)))?;
        }
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true).append(true).open(&full).await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Open failed: {}", e)))?;
        file.write_all(content.as_bytes()).await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("Append failed: {}", e)))?;
        Ok(serde_json::json!({
            "action": "append", "path": rel,
            "bytes_appended": content.len(),
        }))
    }

    async fn action_list(&self, path: Option<String>) -> crate::error::ToolResult<serde_json::Value> {
        let rel = path.unwrap_or_default();
        let full = if rel.is_empty() {
            std::path::PathBuf::from(&self.workspace_root)
        } else {
            self.resolve_path(&rel)?
        };
        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(&full).await
            .map_err(|e| crate::ToolError::ExecutionFailed(format!("List failed: {}", e)))?;
        while let Some(entry) = dir.next_entry().await
            .map_err(|e| crate::ToolError::ExecutionFailed(e.to_string()))? {
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().await.ok();
            entries.push(serde_json::json!({
                "name": name,
                "is_dir": meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
            }));
        }
        Ok(serde_json::json!({
            "action": "list",
            "path": if rel.is_empty() { "." } else { &rel },
            "entries": entries, "count": entries.len(),
        }))
    }
}
