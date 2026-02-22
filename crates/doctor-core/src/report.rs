use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportEntry {
    pub timestamp: DateTime<Utc>,
    pub category: String,
    pub status: String,
    pub message: String,
    pub details: Option<HashMap<String, String>>,
}

impl ReportEntry {
    pub fn new(category: &str, status: &str, message: &str) -> Self {
        Self {
            timestamp: Utc::now(),
            category: category.to_string(),
            status: status.to_string(),
            message: message.to_string(),
            details: None,
        }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub total_checks: usize,
    pub passed_checks: usize,
    pub warning_checks: usize,
    pub error_checks: usize,
    pub critical_checks: usize,
    pub entries: Vec<ReportEntry>,
    pub summary: String,
}

impl DiagnosticReport {
    pub fn new(entries: Vec<ReportEntry>) -> Self {
        let total_checks = entries.len();
        let passed_checks = entries.iter().filter(|e| e.status == "pass").count();
        let warning_checks = entries.iter().filter(|e| e.status == "warning").count();
        let error_checks = entries.iter().filter(|e| e.status == "error").count();
        let critical_checks = entries.iter().filter(|e| e.status == "critical").count();

        let summary = if critical_checks > 0 {
            "Critical issues found - immediate attention required".to_string()
        } else if error_checks > 0 {
            "Errors found - attention required".to_string()
        } else if warning_checks > 0 {
            "Warnings found - may require attention".to_string()
        } else {
            "All checks passed".to_string()
        };

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            total_checks,
            passed_checks,
            warning_checks,
            error_checks,
            critical_checks,
            entries,
            summary,
        }
    }

    pub fn has_errors(&self) -> bool {
        self.error_checks > 0 || self.critical_checks > 0
    }

    pub fn has_warnings(&self) -> bool {
        self.warning_checks > 0
    }

    pub fn is_healthy(&self) -> bool {
        !self.has_errors() && !self.has_warnings()
    }

    pub fn get_entries_by_category(&self, category: &str) -> Vec<&ReportEntry> {
        self.entries
            .iter()
            .filter(|e| e.category == category)
            .collect()
    }

    pub fn get_entries_by_status(&self, status: &str) -> Vec<&ReportEntry> {
        self.entries.iter().filter(|e| e.status == status).collect()
    }
}

pub struct ReportBuilder {
    entries: Vec<ReportEntry>,
}

impl ReportBuilder {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add_entry(mut self, entry: ReportEntry) -> Self {
        self.entries.push(entry);
        self
    }

    pub fn add_entries(mut self, entries: Vec<ReportEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    pub fn build(self) -> DiagnosticReport {
        DiagnosticReport::new(self.entries)
    }
}

impl Default for ReportBuilder {
    fn default() -> Self {
        Self::new()
    }
}
