#![recursion_limit = "512"]

pub mod approval;
pub mod context;
pub mod error;
pub mod groups;
pub mod policy;
pub mod profiles;
pub mod tool;
pub mod truncation;

pub use approval::{ApprovalDecision, ApprovalGate, ApprovalPolicy};
pub use context::ToolContext;
pub use error::{ToolError, ToolResult};
pub use groups::{expand_tool_list, is_group_ref, resolve_tool_group};
pub use policy::{
    LayeredPolicyPipeline, PolicyContext, PolicyLayer, ToolPolicy, ToolPolicyDecision,
    ToolPolicyPipeline,
};
pub use profiles::ToolProfile;
pub use tool::{
    BashTool, BrowseTool, CronTool, LinkReaderTool, MediaDescribeTool, MemoryTool, MessageTool,
    SessionStatusTool, SessionsHistoryTool, SessionsListTool, SessionsSendTool, SessionsSpawnTool,
    SubagentsTool, Tool, ToolCall, ToolRegistry, ToolResponse, TtsTool, WebFetchTool,
    WebSearchTool, WorkspaceTool,
};
pub use truncation::{TruncationConfig, smart_truncate, truncate_tool_result};

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
