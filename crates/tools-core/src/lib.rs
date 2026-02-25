pub mod error;
pub mod tool;
pub mod approval;
pub mod groups;
pub mod profiles;
pub mod policy;
pub mod context;
pub mod truncation;

pub use error::{ToolError, ToolResult};
pub use tool::{
    Tool, ToolCall, ToolResponse, ToolRegistry,
    BashTool, WebFetchTool, MemoryTool, BrowseTool, WebSearchTool,
    LinkReaderTool, MediaDescribeTool, CronTool, MessageTool,
    SessionsListTool, SessionsHistoryTool, SessionsSendTool,
    SessionsSpawnTool, SubagentsTool, SessionStatusTool, TtsTool,
    WorkspaceTool,
};
pub use approval::{ApprovalGate, ApprovalPolicy, ApprovalDecision};
pub use policy::{
    ToolPolicy, ToolPolicyDecision, ToolPolicyPipeline,
    PolicyLayer, PolicyContext, LayeredPolicyPipeline,
};
pub use groups::{resolve_tool_group, expand_tool_list, is_group_ref};
pub use profiles::ToolProfile;
pub use context::ToolContext;
pub use truncation::{TruncationConfig, truncate_tool_result, smart_truncate};

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
