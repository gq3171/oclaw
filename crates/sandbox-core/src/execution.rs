use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::docker::{DockerSandbox, DockerConfig};

#[derive(Debug, Clone)]
pub enum ExecutionResult {
    Success {
        stdout: String,
        stderr: String,
        exit_code: i32,
        duration_ms: u64,
    },
    Failure {
        error: String,
        exit_code: i32,
        duration_ms: u64,
    },
    Timeout {
        partial_output: String,
        duration_ms: u64,
    },
}

impl ExecutionResult {
    pub fn is_success(&self) -> bool {
        matches!(self, ExecutionResult::Success { exit_code: 0, .. })
    }

    pub fn exit_code(&self) -> Option<i32> {
        match self {
            ExecutionResult::Success { exit_code, .. } => Some(*exit_code),
            ExecutionResult::Failure { exit_code, .. } => Some(*exit_code),
            ExecutionResult::Timeout { .. } => None,
        }
    }

    pub fn output(&self) -> String {
        match self {
            ExecutionResult::Success { stdout, stderr, .. } => {
                if stderr.is_empty() {
                    stdout.clone()
                } else {
                    format!("{}\n{}", stdout, stderr)
                }
            }
            ExecutionResult::Failure { error, .. } => error.clone(),
            ExecutionResult::Timeout { partial_output, .. } => partial_output.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub command: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_dir: Option<PathBuf>,
    pub timeout_ms: Option<u64>,
    pub stdin: Option<String>,
}

impl ExecutionRequest {
    pub fn new(command: Vec<String>) -> Self {
        Self {
            command,
            env: HashMap::new(),
            working_dir: None,
            timeout_ms: Some(60000),
            stdin: None,
        }
    }

    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    pub fn with_stdin(mut self, stdin: String) -> Self {
        self.stdin = Some(stdin);
        self
    }
}

pub struct ExecutionContext {
    pub sandbox_id: String,
    pub working_dir: PathBuf,
    pub env: HashMap<String, String>,
    pub timeout_ms: u64,
}

#[async_trait]
pub trait ExecutionEngine: Send + Sync {
    async fn execute(&self, ctx: &ExecutionContext, request: &ExecutionRequest) -> ExecutionResult;
    async fn execute_stream(
        &self,
        ctx: &ExecutionContext,
        request: &ExecutionRequest,
    ) -> Result<()>;
}

pub struct SandboxExecutor {
    sandbox: Arc<DockerSandbox>,
}

impl SandboxExecutor {
    pub fn new(sandbox: Arc<DockerSandbox>) -> Self {
        Self {
            sandbox,
        }
    }

    pub fn with_docker_config(config: DockerConfig) -> Self {
        Self::new(Arc::new(DockerSandbox::new(config)))
    }
}

#[async_trait]
impl ExecutionEngine for SandboxExecutor {
    async fn execute(&self, ctx: &ExecutionContext, request: &ExecutionRequest) -> ExecutionResult {
        let start = std::time::Instant::now();
        
        let cmd: Vec<&str> = request.command.iter().map(|s| s.as_str()).collect();
        
        let result = self.sandbox.exec_in_sandbox(&ctx.sandbox_id, cmd).await;
        
        let duration_ms = start.elapsed().as_millis() as u64;
        
        match result {
            Ok(output) => {
                ExecutionResult::Success {
                    stdout: output,
                    stderr: String::new(),
                    exit_code: 0,
                    duration_ms,
                }
            }
            Err(e) => {
                ExecutionResult::Failure {
                    error: e.to_string(),
                    exit_code: 1,
                    duration_ms,
                }
            }
        }
    }

    async fn execute_stream(
        &self,
        ctx: &ExecutionContext,
        request: &ExecutionRequest,
    ) -> Result<()> {
        let cmd: Vec<&str> = request.command.iter().map(|s| s.as_str()).collect();
        self.sandbox.exec_in_sandbox(&ctx.sandbox_id, cmd).await?;
        Ok(())
    }
}

pub struct ExecutionManager {
    sandboxes: Arc<RwLock<HashMap<String, Arc<DockerSandbox>>>>,
    default_config: DockerConfig,
}

impl ExecutionManager {
    pub fn new(default_config: DockerConfig) -> Self {
        Self {
            sandboxes: Arc::new(RwLock::new(HashMap::new())),
            default_config,
        }
    }

    pub async fn create_sandbox(&self, name: &str, image: &str) -> Result<String> {
        let sandbox = DockerSandbox::new(self.default_config.clone());
        
        if !sandbox.health_check().await? {
            anyhow::bail!("Docker is not available");
        }
        
        let id = sandbox.create_sandbox(name, image).await?;
        sandbox.start_sandbox(&id).await?;
        
        let mut sandboxes = self.sandboxes.write().await;
        sandboxes.insert(id.clone(), Arc::new(sandbox));
        
        Ok(id)
    }

    pub async fn get_sandbox(&self, id: &str) -> Option<Arc<DockerSandbox>> {
        let sandboxes = self.sandboxes.read().await;
        sandboxes.get(id).cloned()
    }

    pub async fn remove_sandbox(&self, id: &str) -> Result<()> {
        let sandbox = {
            let mut sandboxes = self.sandboxes.write().await;
            sandboxes.remove(id)
        };
        
        if let Some(sandbox) = sandbox {
            sandbox.stop_sandbox(id).await?;
            sandbox.remove_sandbox(id).await?;
        }
        
        Ok(())
    }

    pub async fn execute(&self, sandbox_id: &str, request: &ExecutionRequest) -> Result<ExecutionResult> {
        let sandbox = self.get_sandbox(sandbox_id).await
            .ok_or_else(|| anyhow::anyhow!("Sandbox not found: {}", sandbox_id))?;
        
        let executor = SandboxExecutor::new(sandbox);
        
        let ctx = ExecutionContext {
            sandbox_id: sandbox_id.to_string(),
            working_dir: request.working_dir.clone().unwrap_or_else(|| PathBuf::from("/")),
            env: request.env.clone(),
            timeout_ms: request.timeout_ms.unwrap_or(60000),
        };
        
        Ok(executor.execute(&ctx, request).await)
    }

    pub async fn list_sandboxes(&self) -> Vec<String> {
        let sandboxes = self.sandboxes.read().await;
        sandboxes.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_request_new() {
        let request = ExecutionRequest::new(vec!["echo".to_string(), "hello".to_string()]);
        assert_eq!(request.command, vec!["echo", "hello"]);
        assert!(request.timeout_ms.is_some());
    }

    #[test]
    fn test_execution_request_with_env() {
        let request = ExecutionRequest::new(vec!["echo".to_string()])
            .with_env("KEY", "value")
            .with_env("FOO", "bar");
        
        assert_eq!(request.env.get("KEY"), Some(&"value".to_string()));
        assert_eq!(request.env.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn test_execution_request_with_timeout() {
        let request = ExecutionRequest::new(vec!["sleep".to_string()])
            .with_timeout(5000);
        
        assert_eq!(request.timeout_ms, Some(5000));
    }

    #[test]
    fn test_execution_result_success() {
        let result = ExecutionResult::Success {
            stdout: "hello".to_string(),
            stderr: "".to_string(),
            exit_code: 0,
            duration_ms: 100,
        };
        
        assert!(result.is_success());
        assert_eq!(result.exit_code(), Some(0));
        assert_eq!(result.output(), "hello");
    }

    #[test]
    fn test_execution_result_failure() {
        let result = ExecutionResult::Failure {
            error: "command failed".to_string(),
            exit_code: 1,
            duration_ms: 50,
        };
        
        assert!(!result.is_success());
        assert_eq!(result.exit_code(), Some(1));
    }

    #[test]
    fn test_execution_result_timeout() {
        let result = ExecutionResult::Timeout {
            partial_output: "partial".to_string(),
            duration_ms: 60000,
        };
        
        assert!(!result.is_success());
        assert_eq!(result.exit_code(), None);
        assert_eq!(result.output(), "partial");
    }

    #[tokio::test]
    async fn test_execution_manager_new() {
        let config = DockerConfig::default();
        let manager = ExecutionManager::new(config);
        assert!(manager.list_sandboxes().await.is_empty());
    }
}
