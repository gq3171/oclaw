pub mod error;
pub mod scheduler;
pub mod tool;

pub use error::{ToolError, ToolResult};
pub use tool::{Tool, ToolCall, ToolResponse, ToolRegistry, BashTool};

#[cfg(test)]
mod tests {
    use crate::error::ToolError;
    use crate::scheduler::{CronJob, SchedulerStats};
    use chrono::Utc;

    #[test]
    fn test_tool_error_display() {
        let err = ToolError::NotFound("tool".to_string());
        assert_eq!(err.to_string(), "Tool not found: tool");
        
        let err = ToolError::ExecutionFailed("failed".to_string());
        assert_eq!(err.to_string(), "Tool execution failed: failed");
        
        let err = ToolError::InvalidInput("invalid".to_string());
        assert_eq!(err.to_string(), "Invalid input: invalid");
    }

    #[test]
    fn test_cron_job_creation() {
        let job = CronJob {
            id: "test_id".to_string(),
            name: "test_job".to_string(),
            cron_expression: "0 * * * *".to_string(),
            command: "echo test".to_string(),
            enabled: true,
            last_run: None,
            next_run: None,
            created_at: Utc::now(),
        };
        
        assert_eq!(job.name, "test_job");
        assert_eq!(job.cron_expression, "0 * * * *");
        assert!(job.enabled);
    }

    #[test]
    fn test_scheduler_stats() {
        let stats = SchedulerStats {
            total_jobs: 10,
            enabled_jobs: 5,
            running_jobs: 2,
        };
        
        assert_eq!(stats.total_jobs, 10);
        assert_eq!(stats.enabled_jobs, 5);
        assert_eq!(stats.running_jobs, 2);
    }
}
