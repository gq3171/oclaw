mod checker;
mod health;
mod report;

pub use checker::{
    CheckCategory, CheckResult, CheckStatus, ConfigChecker, DependencyChecker, DiagnosticChecker,
    NetworkChecker, PerformanceChecker, SecurityChecker, StorageChecker, SystemChecker,
};
pub use health::{
    CallbackHealthCheck, FlagHealthCheck, HealthCheck, HealthChecker, HealthComponent,
    HealthReport, HealthStatus, SystemHealthCheck,
};
pub use report::{DiagnosticReport, ReportBuilder, ReportEntry};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticIssue {
    pub code: String,
    pub message: String,
    pub severity: Severity,
    pub category: CheckCategory,
    pub suggestion: Option<String>,
    pub details: Option<HashMap<String, String>>,
}

impl DiagnosticIssue {
    pub fn new(code: &str, message: &str, severity: Severity, category: CheckCategory) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            severity,
            category,
            suggestion: None,
            details: None,
        }
    }

    pub fn with_suggestion(mut self, suggestion: &str) -> Self {
        self.suggestion = Some(suggestion.to_string());
        self
    }

    pub fn with_detail(mut self, key: &str, value: &str) -> Self {
        if self.details.is_none() {
            self.details = Some(HashMap::new());
        }
        self.details
            .as_mut()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        self
    }
}

pub struct DiagnosticManager {
    checkers: Arc<RwLock<Vec<Box<dyn DiagnosticChecker>>>>,
    issues: Arc<RwLock<Vec<DiagnosticIssue>>>,
}

impl DiagnosticManager {
    pub fn new() -> Self {
        let default_checkers: Vec<Box<dyn DiagnosticChecker>> = vec![
            Box::new(SystemChecker::new()),
            Box::new(NetworkChecker::new()),
            Box::new(ConfigChecker::new()),
            Box::new(DependencyChecker::new()),
            Box::new(StorageChecker::new()),
            Box::new(SecurityChecker::new()),
            Box::new(PerformanceChecker::new()),
        ];
        Self {
            checkers: Arc::new(RwLock::new(default_checkers)),
            issues: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn register_checker(&self, checker: Box<dyn DiagnosticChecker>) {
        let mut checkers = self.checkers.write().await;
        checkers.push(checker);
    }

    pub async fn run_all_checks(&self) -> Vec<DiagnosticIssue> {
        let checkers = self.checkers.read().await;
        let mut all_issues = Vec::new();

        for checker in checkers.iter() {
            let results = checker.run().await;
            for result in results {
                if result.status != CheckStatus::Pass {
                    let issue = DiagnosticIssue::new(
                        &result.code,
                        &result.message,
                        match result.status {
                            CheckStatus::Pass => Severity::Info,
                            CheckStatus::Warning => Severity::Warning,
                            CheckStatus::Error => Severity::Error,
                            CheckStatus::Critical => Severity::Critical,
                        },
                        result.category,
                    )
                    .with_suggestion(&result.suggestion);
                    all_issues.push(issue);
                }
            }
        }

        let mut issues = self.issues.write().await;
        *issues = all_issues.clone();

        all_issues
    }

    pub async fn run_category_check(&self, category: CheckCategory) -> Vec<DiagnosticIssue> {
        let checkers = self.checkers.read().await;
        let mut issues = Vec::new();

        for checker in checkers.iter() {
            if checker.category() == category {
                let results = checker.run().await;
                for result in results {
                    if result.status != CheckStatus::Pass {
                        let issue = DiagnosticIssue::new(
                            &result.code,
                            &result.message,
                            match result.status {
                                CheckStatus::Pass => Severity::Info,
                                CheckStatus::Warning => Severity::Warning,
                                CheckStatus::Error => Severity::Error,
                                CheckStatus::Critical => Severity::Critical,
                            },
                            result.category,
                        )
                        .with_suggestion(&result.suggestion);
                        issues.push(issue);
                    }
                }
            }
        }

        issues
    }

    pub async fn get_issues(&self) -> Vec<DiagnosticIssue> {
        let issues = self.issues.read().await;
        issues.clone()
    }

    pub async fn clear_issues(&self) {
        let mut issues = self.issues.write().await;
        issues.clear();
    }

    pub async fn get_issues_by_severity(&self, severity: Severity) -> Vec<DiagnosticIssue> {
        let issues = self.issues.read().await;
        issues
            .iter()
            .filter(|i| i.severity == severity)
            .cloned()
            .collect()
    }

    pub async fn get_issues_by_category(&self, category: CheckCategory) -> Vec<DiagnosticIssue> {
        let issues = self.issues.read().await;
        issues
            .iter()
            .filter(|i| i.category == category)
            .cloned()
            .collect()
    }
}

impl Default for DiagnosticManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_issue_new() {
        let issue = DiagnosticIssue::new(
            "TEST001",
            "Test issue",
            Severity::Warning,
            CheckCategory::System,
        );
        assert_eq!(issue.code, "TEST001");
        assert_eq!(issue.severity, Severity::Warning);
    }

    #[test]
    fn test_diagnostic_issue_with_suggestion() {
        let issue = DiagnosticIssue::new(
            "TEST001",
            "Test issue",
            Severity::Error,
            CheckCategory::System,
        )
        .with_suggestion("Fix this issue");
        assert_eq!(issue.suggestion, Some("Fix this issue".to_string()));
    }

    #[test]
    fn test_diagnostic_issue_with_detail() {
        let issue = DiagnosticIssue::new(
            "TEST001",
            "Test issue",
            Severity::Warning,
            CheckCategory::Network,
        )
        .with_detail("key", "value");
        assert!(issue.details.is_some());
        assert_eq!(
            issue.details.as_ref().unwrap().get("key"),
            Some(&"value".to_string())
        );
    }

    #[tokio::test]
    async fn test_diagnostic_manager_new() {
        let manager = DiagnosticManager::new();
        let issues = manager.get_issues().await;
        assert!(issues.is_empty());
    }

    #[tokio::test]
    async fn test_diagnostic_manager_run_all_checks() {
        let manager = DiagnosticManager::new();
        let issues = manager.run_all_checks().await;
        let stored = manager.get_issues().await;
        assert_eq!(issues.len(), stored.len());
    }
}
