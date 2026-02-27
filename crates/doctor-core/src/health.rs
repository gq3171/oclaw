use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

impl HealthStatus {
    pub fn is_healthy(&self) -> bool {
        matches!(self, HealthStatus::Healthy)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Degraded => "degraded",
            HealthStatus::Unhealthy => "unhealthy",
            HealthStatus::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthComponent {
    pub name: String,
    pub status: HealthStatus,
    pub message: Option<String>,
    pub details: Option<HashMap<String, String>>,
}

impl HealthComponent {
    pub fn new(name: &str, status: HealthStatus) -> Self {
        Self {
            name: name.to_string(),
            status,
            message: None,
            details: None,
        }
    }

    pub fn with_message(mut self, message: &str) -> Self {
        self.message = Some(message.to_string());
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub overall_status: HealthStatus,
    pub components: Vec<HealthComponent>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl HealthReport {
    pub fn new(components: Vec<HealthComponent>) -> Self {
        let overall_status = if components.iter().all(|c| c.status == HealthStatus::Healthy) {
            HealthStatus::Healthy
        } else if components
            .iter()
            .any(|c| c.status == HealthStatus::Unhealthy)
        {
            HealthStatus::Unhealthy
        } else {
            HealthStatus::Degraded
        };

        Self {
            overall_status,
            components,
            timestamp: chrono::Utc::now(),
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.overall_status.is_healthy()
    }
}

pub trait HealthCheck: Send + Sync {
    fn check(&self) -> HealthComponent;
}

pub struct HealthChecker {
    checks: Vec<Box<dyn HealthCheck>>,
}

impl HealthChecker {
    pub fn new() -> Self {
        Self { checks: Vec::new() }
    }

    pub fn register(&mut self, check: Box<dyn HealthCheck>) {
        self.checks.push(check);
    }

    pub fn check_all(&self) -> HealthReport {
        let mut components = Vec::new();

        for check in &self.checks {
            let component = check.check();
            components.push(component);
        }

        HealthReport::new(components)
    }

    pub fn check_one(&self, name: &str) -> Option<HealthComponent> {
        for check in &self.checks {
            let component = check.check();
            if component.name == name {
                return Some(component);
            }
        }
        None
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SystemHealthCheck {
    started: std::time::Instant,
}

impl SystemHealthCheck {
    pub fn new() -> Self {
        Self {
            started: std::time::Instant::now(),
        }
    }
}

impl Default for SystemHealthCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthCheck for SystemHealthCheck {
    fn check(&self) -> HealthComponent {
        let uptime = self.started.elapsed().as_secs();
        HealthComponent::new("system", HealthStatus::Healthy)
            .with_message("System is operational")
            .with_detail("uptime_secs", &uptime.to_string())
    }
}

pub struct FlagHealthCheck {
    name: String,
    flag: Arc<AtomicBool>,
}

impl FlagHealthCheck {
    pub fn new(name: &str, flag: Arc<AtomicBool>) -> Self {
        Self {
            name: name.to_string(),
            flag,
        }
    }
}

impl HealthCheck for FlagHealthCheck {
    fn check(&self) -> HealthComponent {
        if self.flag.load(Ordering::Relaxed) {
            HealthComponent::new(&self.name, HealthStatus::Healthy)
        } else {
            HealthComponent::new(&self.name, HealthStatus::Unhealthy)
                .with_message("Component is down")
        }
    }
}

pub struct CallbackHealthCheck {
    cb: Box<dyn Fn() -> HealthComponent + Send + Sync>,
}

impl CallbackHealthCheck {
    pub fn new(cb: Box<dyn Fn() -> HealthComponent + Send + Sync>) -> Self {
        Self { cb }
    }
}

impl HealthCheck for CallbackHealthCheck {
    fn check(&self) -> HealthComponent {
        (self.cb)()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_healthy() {
        let status = HealthStatus::Healthy;
        assert!(status.is_healthy());
        assert_eq!(status.as_str(), "healthy");
    }

    #[test]
    fn test_health_status_unhealthy() {
        let status = HealthStatus::Unhealthy;
        assert!(!status.is_healthy());
        assert_eq!(status.as_str(), "unhealthy");
    }

    #[test]
    fn test_health_component_new() {
        let component = HealthComponent::new("test", HealthStatus::Healthy);
        assert_eq!(component.name, "test");
        assert_eq!(component.status, HealthStatus::Healthy);
    }

    #[test]
    fn test_health_component_with_message() {
        let component =
            HealthComponent::new("test", HealthStatus::Healthy).with_message("All good");
        assert_eq!(component.message, Some("All good".to_string()));
    }

    #[test]
    fn test_health_component_with_detail() {
        let component =
            HealthComponent::new("test", HealthStatus::Healthy).with_detail("key", "value");
        assert!(component.details.is_some());
        assert_eq!(
            component.details.as_ref().unwrap().get("key"),
            Some(&"value".to_string())
        );
    }

    #[test]
    fn test_health_report_all_healthy() {
        let components = vec![
            HealthComponent::new("comp1", HealthStatus::Healthy),
            HealthComponent::new("comp2", HealthStatus::Healthy),
        ];
        let report = HealthReport::new(components);
        assert!(report.is_healthy());
        assert_eq!(report.overall_status, HealthStatus::Healthy);
    }

    #[test]
    fn test_health_report_with_unhealthy() {
        let components = vec![
            HealthComponent::new("comp1", HealthStatus::Healthy),
            HealthComponent::new("comp2", HealthStatus::Unhealthy),
        ];
        let report = HealthReport::new(components);
        assert!(!report.is_healthy());
        assert_eq!(report.overall_status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_health_report_with_degraded() {
        let components = vec![
            HealthComponent::new("comp1", HealthStatus::Healthy),
            HealthComponent::new("comp2", HealthStatus::Degraded),
        ];
        let report = HealthReport::new(components);
        assert!(!report.is_healthy());
        assert_eq!(report.overall_status, HealthStatus::Degraded);
    }

    #[test]
    fn test_health_checker() {
        let mut checker = HealthChecker::new();
        checker.register(Box::new(SystemHealthCheck::new()));

        let report = checker.check_all();
        assert!(report.is_healthy());
    }

    #[test]
    fn test_health_checker_check_one() {
        let mut checker = HealthChecker::new();
        checker.register(Box::new(SystemHealthCheck::new()));

        let component = checker.check_one("system");
        assert!(component.is_some());
        assert_eq!(component.unwrap().name, "system");
    }

    #[test]
    fn test_system_health_check_reports_uptime() {
        let check = SystemHealthCheck::new();
        let comp = check.check();
        assert_eq!(comp.status, HealthStatus::Healthy);
        assert!(comp.details.as_ref().unwrap().contains_key("uptime_secs"));
    }

    #[test]
    fn test_flag_health_check_toggle() {
        let flag = Arc::new(AtomicBool::new(true));
        let check = FlagHealthCheck::new("db", flag.clone());
        assert_eq!(check.check().status, HealthStatus::Healthy);

        flag.store(false, Ordering::Relaxed);
        assert_eq!(check.check().status, HealthStatus::Unhealthy);
    }

    #[test]
    fn test_callback_health_check() {
        let check = CallbackHealthCheck::new(Box::new(|| {
            HealthComponent::new("custom", HealthStatus::Degraded).with_message("high latency")
        }));
        let comp = check.check();
        assert_eq!(comp.name, "custom");
        assert_eq!(comp.status, HealthStatus::Degraded);
    }

    #[test]
    fn test_checker_mixed_statuses() {
        let up = Arc::new(AtomicBool::new(true));
        let down = Arc::new(AtomicBool::new(false));
        let mut checker = HealthChecker::new();
        checker.register(Box::new(FlagHealthCheck::new("a", up)));
        checker.register(Box::new(FlagHealthCheck::new("b", down)));
        let report = checker.check_all();
        assert_eq!(report.overall_status, HealthStatus::Unhealthy);
        assert_eq!(report.components.len(), 2);
    }
}
