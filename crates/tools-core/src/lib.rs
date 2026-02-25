pub mod error;
pub mod tool;
pub mod approval;

pub use error::{ToolError, ToolResult};
pub use tool::{Tool, ToolCall, ToolResponse, ToolRegistry, BashTool, WebFetchTool, MemoryTool};
pub use approval::{ApprovalGate, ApprovalPolicy, ApprovalDecision};

#[cfg(test)]
mod tests {
    use crate::error::ToolError;

    #[test]
    fn test_tool_error_display() {
        let err = ToolError::NotFound("tool".to_string());
        assert_eq!(err.to_string(), "Tool not found: tool");

        let err = ToolError::ExecutionFailed("failed".to_string());
        assert_eq!(err.to_string(), "Tool execution failed: failed");

        let err = ToolError::InvalidInput("invalid".to_string());
        assert_eq!(err.to_string(), "Invalid input: invalid");
    }
}
