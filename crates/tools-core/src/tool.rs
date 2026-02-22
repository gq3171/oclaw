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
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Tool::Bash(_) => "Execute a shell command and return the output",
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
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> ToolResult<serde_json::Value> {
        match self {
            Tool::Bash(tool) => tool.execute(arguments).await,
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
