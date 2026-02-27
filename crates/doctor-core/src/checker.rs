use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckCategory {
    System,
    Network,
    Configuration,
    Dependencies,
    Storage,
    Security,
    Performance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckStatus {
    Pass,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub code: String,
    pub message: String,
    pub status: CheckStatus,
    pub category: CheckCategory,
    pub suggestion: String,
}

#[async_trait]
pub trait DiagnosticChecker: Send + Sync {
    fn category(&self) -> CheckCategory;
    fn name(&self) -> &str;
    async fn run(&self) -> Vec<CheckResult>;
}

fn result(
    code: &str,
    message: impl Into<String>,
    status: CheckStatus,
    category: CheckCategory,
    suggestion: impl Into<String>,
) -> CheckResult {
    CheckResult {
        code: code.to_string(),
        message: message.into(),
        status,
        category,
        suggestion: suggestion.into(),
    }
}

fn oclaw_home_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oclaw")
}

#[cfg(target_os = "linux")]
fn linux_meminfo() -> Option<(u64, u64)> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = None;
    let mut available_kb = None;
    for line in content.lines() {
        if let Some(raw) = line.strip_prefix("MemTotal:") {
            total_kb = raw
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok());
        } else if let Some(raw) = line.strip_prefix("MemAvailable:") {
            available_kb = raw
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok());
        }
        if total_kb.is_some() && available_kb.is_some() {
            break;
        }
    }
    Some((total_kb?, available_kb?))
}

pub struct SystemChecker;

impl Default for SystemChecker {
    fn default() -> Self {
        Self
    }
}

impl SystemChecker {
    pub fn new() -> Self {
        Self
    }

    async fn check_os(&self) -> CheckResult {
        result(
            "OS_CHECK",
            format!("OS: {} / {}", std::env::consts::OS, std::env::consts::ARCH),
            CheckStatus::Pass,
            CheckCategory::System,
            "",
        )
    }

    async fn check_memory(&self) -> CheckResult {
        #[cfg(target_os = "linux")]
        {
            if let Some((total_kb, available_kb)) = linux_meminfo() {
                if total_kb == 0 {
                    return result(
                        "MEMORY_CHECK",
                        "Unable to read total memory",
                        CheckStatus::Warning,
                        CheckCategory::System,
                        "Check /proc/meminfo permissions.",
                    );
                }
                let ratio = available_kb as f64 / total_kb as f64;
                let msg = format!(
                    "Available memory: {:.1}% ({} MB / {} MB)",
                    ratio * 100.0,
                    available_kb / 1024,
                    total_kb / 1024
                );
                if ratio < 0.10 {
                    return result(
                        "MEMORY_CHECK",
                        msg,
                        CheckStatus::Error,
                        CheckCategory::System,
                        "Free memory is very low; close heavy processes or increase RAM/swap.",
                    );
                }
                if ratio < 0.20 {
                    return result(
                        "MEMORY_CHECK",
                        msg,
                        CheckStatus::Warning,
                        CheckCategory::System,
                        "Memory headroom is limited; monitor OOM risk under load.",
                    );
                }
                return result(
                    "MEMORY_CHECK",
                    msg,
                    CheckStatus::Pass,
                    CheckCategory::System,
                    "",
                );
            }
            result(
                "MEMORY_CHECK",
                "Cannot parse /proc/meminfo",
                CheckStatus::Warning,
                CheckCategory::System,
                "Ensure procfs is mounted and readable.",
            )
        }

        #[cfg(not(target_os = "linux"))]
        {
            result(
                "MEMORY_CHECK",
                "Memory probe is currently Linux-optimized",
                CheckStatus::Warning,
                CheckCategory::System,
                "Run on Linux for detailed memory diagnostics.",
            )
        }
    }

    async fn check_disk_space(&self) -> CheckResult {
        #[cfg(not(target_os = "windows"))]
        {
            let target = oclaw_home_dir();
            let output = Command::new("df").arg("-Pk").arg(&target).output();
            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    if let Some(line) = text.lines().nth(1) {
                        let cols: Vec<&str> = line.split_whitespace().collect();
                        if cols.len() >= 4
                            && let Ok(available_kb) = cols[3].parse::<u64>()
                        {
                            let available_mb = available_kb / 1024;
                            if available_kb < 1_048_576 {
                                return result(
                                    "DISK_CHECK",
                                    format!("Low disk space: {} MB free", available_mb),
                                    CheckStatus::Warning,
                                    CheckCategory::System,
                                    "Free at least 1 GB for logs, memory DB and runtime files.",
                                );
                            }
                            return result(
                                "DISK_CHECK",
                                format!("Disk free: {} MB", available_mb),
                                CheckStatus::Pass,
                                CheckCategory::System,
                                "",
                            );
                        }
                    }
                    result(
                        "DISK_CHECK",
                        "Disk check output parse failed",
                        CheckStatus::Warning,
                        CheckCategory::System,
                        "Verify `df` is available and locale output is standard.",
                    )
                }
                _ => result(
                    "DISK_CHECK",
                    "Failed to execute disk space check",
                    CheckStatus::Warning,
                    CheckCategory::System,
                    "Install coreutils or run disk check manually.",
                ),
            }
        }

        #[cfg(target_os = "windows")]
        {
            let script = "$d=Get-PSDrive -PSProvider FileSystem | Sort-Object -Property Free -Descending | Select-Object -First 1; if ($d -ne $null) { \"$($d.Name)|$($d.Free)\" }";
            let output = Command::new("powershell")
                .args(["-NoProfile", "-Command", script])
                .output();

            match output {
                Ok(out) if out.status.success() => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let line = text.lines().find(|l| l.contains('|'));
                    if let Some(line) = line {
                        let cols: Vec<&str> = line.trim().split('|').collect();
                        if cols.len() == 2
                            && let Ok(free_bytes) = cols[1].trim().parse::<u64>()
                        {
                            let free_mb = free_bytes / (1024 * 1024);
                            if free_mb < 1024 {
                                return result(
                                    "DISK_CHECK",
                                    format!("Low disk space on {}: {} MB free", cols[0], free_mb),
                                    CheckStatus::Warning,
                                    CheckCategory::System,
                                    "Free at least 1 GB for logs, memory DB and runtime files.",
                                );
                            }
                            return result(
                                "DISK_CHECK",
                                format!("Disk free on {}: {} MB", cols[0], free_mb),
                                CheckStatus::Pass,
                                CheckCategory::System,
                                "",
                            );
                        }
                    }
                    result(
                        "DISK_CHECK",
                        "Windows disk check output parse failed",
                        CheckStatus::Warning,
                        CheckCategory::System,
                        "Verify PowerShell is available and Get-PSDrive works.",
                    )
                }
                _ => result(
                    "DISK_CHECK",
                    "Failed to execute Windows disk space check",
                    CheckStatus::Warning,
                    CheckCategory::System,
                    "Run `Get-PSDrive -PSProvider FileSystem` manually for diagnostics.",
                ),
            }
        }
    }
}

#[async_trait]
impl DiagnosticChecker for SystemChecker {
    fn category(&self) -> CheckCategory {
        CheckCategory::System
    }

    fn name(&self) -> &str {
        "System Checker"
    }

    async fn run(&self) -> Vec<CheckResult> {
        vec![
            self.check_os().await,
            self.check_memory().await,
            self.check_disk_space().await,
        ]
    }
}

pub struct NetworkChecker;

impl Default for NetworkChecker {
    fn default() -> Self {
        Self
    }
}

impl NetworkChecker {
    pub fn new() -> Self {
        Self
    }

    async fn check_internet(&self) -> CheckResult {
        let fut = tokio::net::TcpStream::connect(("1.1.1.1", 80));
        match tokio::time::timeout(Duration::from_secs(3), fut).await {
            Ok(Ok(_)) => result(
                "INTERNET_CHECK",
                "Outbound network connection is available",
                CheckStatus::Pass,
                CheckCategory::Network,
                "",
            ),
            _ => result(
                "INTERNET_CHECK",
                "Cannot reach public network endpoint (1.1.1.1:80)",
                CheckStatus::Warning,
                CheckCategory::Network,
                "Check firewall/proxy settings and outbound network access.",
            ),
        }
    }

    async fn check_dns(&self) -> CheckResult {
        match tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::lookup_host("api.anthropic.com:443"),
        )
        .await
        {
            Ok(Ok(addrs)) => {
                if addrs.into_iter().next().is_some() {
                    result(
                        "DNS_CHECK",
                        "DNS resolution works for provider endpoints",
                        CheckStatus::Pass,
                        CheckCategory::Network,
                        "",
                    )
                } else {
                    result(
                        "DNS_CHECK",
                        "DNS lookup returned no address records",
                        CheckStatus::Warning,
                        CheckCategory::Network,
                        "Check DNS server configuration.",
                    )
                }
            }
            _ => result(
                "DNS_CHECK",
                "DNS lookup failed for api.anthropic.com",
                CheckStatus::Warning,
                CheckCategory::Network,
                "Check DNS server configuration.",
            ),
        }
    }

    async fn check_ports(&self) -> CheckResult {
        match tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(_listener) => result(
                "PORT_CHECK",
                "Local TCP bind check passed",
                CheckStatus::Pass,
                CheckCategory::Network,
                "",
            ),
            Err(e) => result(
                "PORT_CHECK",
                format!("Cannot bind local TCP socket: {}", e),
                CheckStatus::Error,
                CheckCategory::Network,
                "Verify local networking stack and permissions.",
            ),
        }
    }
}

#[async_trait]
impl DiagnosticChecker for NetworkChecker {
    fn category(&self) -> CheckCategory {
        CheckCategory::Network
    }

    fn name(&self) -> &str {
        "Network Checker"
    }

    async fn run(&self) -> Vec<CheckResult> {
        vec![
            self.check_internet().await,
            self.check_dns().await,
            self.check_ports().await,
        ]
    }
}

pub struct ConfigChecker;

impl Default for ConfigChecker {
    fn default() -> Self {
        Self
    }
}

impl ConfigChecker {
    pub fn new() -> Self {
        Self
    }

    async fn check_config_files(&self) -> CheckResult {
        let path = oclaw_home_dir().join("config.json");
        if path.exists() {
            result(
                "CONFIG_FILES",
                format!("Config file found: {}", path.display()),
                CheckStatus::Pass,
                CheckCategory::Configuration,
                "",
            )
        } else {
            result(
                "CONFIG_FILES",
                format!("Config file missing: {}", path.display()),
                CheckStatus::Warning,
                CheckCategory::Configuration,
                "Run `oclaw config init` to create a default config.",
            )
        }
    }

    async fn check_env_vars(&self) -> CheckResult {
        let keys = [
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "OCLAWS_PROVIDER_ANTHROPIC_API_KEY",
            "OCLAWS_PROVIDER_OPENAI_API_KEY",
        ];
        let found = keys
            .iter()
            .any(|k| std::env::var(k).ok().filter(|v| !v.is_empty()).is_some());
        if found {
            result(
                "ENV_VARS",
                "API key environment variables detected",
                CheckStatus::Pass,
                CheckCategory::Configuration,
                "",
            )
        } else {
            result(
                "ENV_VARS",
                "No common LLM API key environment variables found",
                CheckStatus::Warning,
                CheckCategory::Configuration,
                "Set at least one provider API key (for example OPENAI_API_KEY).",
            )
        }
    }

    async fn check_secrets(&self) -> CheckResult {
        let path = oclaw_home_dir().join("config.json");
        let content = std::fs::read_to_string(&path);
        match content {
            Ok(text) => {
                if text.contains("${") {
                    result(
                        "SECRETS",
                        "Config contains unresolved ${...} placeholders",
                        CheckStatus::Warning,
                        CheckCategory::Configuration,
                        "Ensure environment-variable placeholders are resolvable at runtime.",
                    )
                } else {
                    result(
                        "SECRETS",
                        "No unresolved secret placeholders detected",
                        CheckStatus::Pass,
                        CheckCategory::Configuration,
                        "",
                    )
                }
            }
            Err(_) => result(
                "SECRETS",
                "Skipped secret placeholder check (config not readable)",
                CheckStatus::Warning,
                CheckCategory::Configuration,
                "Check config file readability.",
            ),
        }
    }
}

#[async_trait]
impl DiagnosticChecker for ConfigChecker {
    fn category(&self) -> CheckCategory {
        CheckCategory::Configuration
    }

    fn name(&self) -> &str {
        "Configuration Checker"
    }

    async fn run(&self) -> Vec<CheckResult> {
        vec![
            self.check_config_files().await,
            self.check_env_vars().await,
            self.check_secrets().await,
        ]
    }
}

pub struct DependencyChecker;

impl Default for DependencyChecker {
    fn default() -> Self {
        Self
    }
}

impl DependencyChecker {
    pub fn new() -> Self {
        Self
    }

    fn check_binary(&self, bin: &str, arg: &str, required: bool) -> CheckResult {
        let output = Command::new(bin).arg(arg).output();
        match output {
            Ok(out) if out.status.success() => result(
                &format!("{}_CHECK", bin.to_uppercase()),
                format!("{} available", bin),
                CheckStatus::Pass,
                CheckCategory::Dependencies,
                "",
            ),
            _ if required => result(
                &format!("{}_CHECK", bin.to_uppercase()),
                format!("{} not available", bin),
                CheckStatus::Error,
                CheckCategory::Dependencies,
                format!("Install {} and ensure it is in PATH.", bin),
            ),
            _ => result(
                &format!("{}_CHECK", bin.to_uppercase()),
                format!("{} not available", bin),
                CheckStatus::Warning,
                CheckCategory::Dependencies,
                format!("Install {} if this feature is required.", bin),
            ),
        }
    }

    async fn check_docker(&self) -> CheckResult {
        self.check_binary("docker", "--version", false)
    }

    async fn check_rust(&self) -> CheckResult {
        self.check_binary("rustc", "--version", true)
    }

    async fn check_node(&self) -> CheckResult {
        self.check_binary("node", "--version", false)
    }
}

#[async_trait]
impl DiagnosticChecker for DependencyChecker {
    fn category(&self) -> CheckCategory {
        CheckCategory::Dependencies
    }

    fn name(&self) -> &str {
        "Dependency Checker"
    }

    async fn run(&self) -> Vec<CheckResult> {
        vec![
            self.check_docker().await,
            self.check_rust().await,
            self.check_node().await,
        ]
    }
}

pub struct StorageChecker;

impl Default for StorageChecker {
    fn default() -> Self {
        Self
    }
}

impl StorageChecker {
    pub fn new() -> Self {
        Self
    }

    async fn check_database(&self) -> CheckResult {
        let path = oclaw_home_dir().join("memory.db");
        if !path.exists() {
            return result(
                "DATABASE",
                format!("Memory database not found: {}", path.display()),
                CheckStatus::Warning,
                CheckCategory::Storage,
                "Database will be created on first memory write.",
            );
        }
        match rusqlite::Connection::open(&path)
            .and_then(|conn| conn.query_row("SELECT 1", [], |row| row.get::<_, i32>(0)))
        {
            Ok(_) => result(
                "DATABASE",
                format!("SQLite check passed: {}", path.display()),
                CheckStatus::Pass,
                CheckCategory::Storage,
                "",
            ),
            Err(e) => result(
                "DATABASE",
                format!("SQLite check failed: {}", e),
                CheckStatus::Error,
                CheckCategory::Storage,
                "Verify DB permissions and integrity.",
            ),
        }
    }

    async fn check_cache(&self) -> CheckResult {
        let dir = oclaw_home_dir().join("cache");
        match std::fs::create_dir_all(&dir) {
            Ok(_) => result(
                "CACHE",
                format!("Cache directory writable: {}", dir.display()),
                CheckStatus::Pass,
                CheckCategory::Storage,
                "",
            ),
            Err(e) => result(
                "CACHE",
                format!("Cache directory check failed: {}", e),
                CheckStatus::Error,
                CheckCategory::Storage,
                "Check filesystem permissions for ~/.oclaw/cache.",
            ),
        }
    }

    async fn check_logs(&self) -> CheckResult {
        let dir = oclaw_home_dir().join("logs");
        match std::fs::create_dir_all(&dir) {
            Ok(_) => result(
                "LOGS",
                format!("Log directory writable: {}", dir.display()),
                CheckStatus::Pass,
                CheckCategory::Storage,
                "",
            ),
            Err(e) => result(
                "LOGS",
                format!("Log directory check failed: {}", e),
                CheckStatus::Error,
                CheckCategory::Storage,
                "Check filesystem permissions for ~/.oclaw/logs.",
            ),
        }
    }
}

#[async_trait]
impl DiagnosticChecker for StorageChecker {
    fn category(&self) -> CheckCategory {
        CheckCategory::Storage
    }

    fn name(&self) -> &str {
        "Storage Checker"
    }

    async fn run(&self) -> Vec<CheckResult> {
        vec![
            self.check_database().await,
            self.check_cache().await,
            self.check_logs().await,
        ]
    }
}

pub struct SecurityChecker;

impl Default for SecurityChecker {
    fn default() -> Self {
        Self
    }
}

impl SecurityChecker {
    pub fn new() -> Self {
        Self
    }

    async fn check_permissions(&self) -> CheckResult {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = oclaw_home_dir();
            match std::fs::metadata(&path) {
                Ok(meta) => {
                    let mode = meta.permissions().mode() & 0o777;
                    if mode & 0o077 != 0 {
                        result(
                            "PERMISSIONS",
                            format!("{} is too permissive ({:o})", path.display(), mode),
                            CheckStatus::Warning,
                            CheckCategory::Security,
                            "Run `chmod 700 ~/.oclaw` to restrict access.",
                        )
                    } else {
                        result(
                            "PERMISSIONS",
                            format!("{} permission looks safe ({:o})", path.display(), mode),
                            CheckStatus::Pass,
                            CheckCategory::Security,
                            "",
                        )
                    }
                }
                Err(_) => result(
                    "PERMISSIONS",
                    format!("{} not found yet", path.display()),
                    CheckStatus::Warning,
                    CheckCategory::Security,
                    "Create config first, then re-run doctor.",
                ),
            }
        }
        #[cfg(not(unix))]
        {
            result(
                "PERMISSIONS",
                "Permission check is currently Unix-optimized",
                CheckStatus::Warning,
                CheckCategory::Security,
                "Run on Unix/Linux for detailed permission checks.",
            )
        }
    }

    async fn check_encryption(&self) -> CheckResult {
        let has_auth = std::env::var("OCLAWS_GATEWAY_AUTH_TOKEN")
            .ok()
            .filter(|v| !v.is_empty())
            .is_some()
            || std::env::var("OCLAWS_GATEWAY_AUTH_PASSWORD")
                .ok()
                .filter(|v| !v.is_empty())
                .is_some();
        if has_auth {
            result(
                "ENCRYPTION",
                "Gateway auth secret environment variables found",
                CheckStatus::Pass,
                CheckCategory::Security,
                "",
            )
        } else {
            result(
                "ENCRYPTION",
                "No gateway auth secret detected in environment",
                CheckStatus::Warning,
                CheckCategory::Security,
                "Set OCLAWS_GATEWAY_AUTH_TOKEN or OCLAWS_GATEWAY_AUTH_PASSWORD.",
            )
        }
    }

    async fn check_tls(&self) -> CheckResult {
        let cert = std::env::var("OCLAWS_TLS_CERT").ok();
        let key = std::env::var("OCLAWS_TLS_KEY").ok();
        match (cert, key) {
            (Some(c), Some(k)) if PathBuf::from(&c).exists() && PathBuf::from(&k).exists() => {
                result(
                    "TLS",
                    "TLS cert/key files detected",
                    CheckStatus::Pass,
                    CheckCategory::Security,
                    "",
                )
            }
            _ => result(
                "TLS",
                "TLS cert/key not configured via OCLAWS_TLS_CERT/OCLAWS_TLS_KEY",
                CheckStatus::Warning,
                CheckCategory::Security,
                "Configure TLS env vars for production deployments.",
            ),
        }
    }
}

#[async_trait]
impl DiagnosticChecker for SecurityChecker {
    fn category(&self) -> CheckCategory {
        CheckCategory::Security
    }

    fn name(&self) -> &str {
        "Security Checker"
    }

    async fn run(&self) -> Vec<CheckResult> {
        vec![
            self.check_permissions().await,
            self.check_encryption().await,
            self.check_tls().await,
        ]
    }
}

pub struct PerformanceChecker;

impl Default for PerformanceChecker {
    fn default() -> Self {
        Self
    }
}

impl PerformanceChecker {
    pub fn new() -> Self {
        Self
    }

    async fn check_cpu(&self) -> CheckResult {
        #[cfg(target_os = "linux")]
        {
            match std::fs::read_to_string("/proc/loadavg") {
                Ok(content) => {
                    let load = content
                        .split_whitespace()
                        .next()
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    if load > 8.0 {
                        result(
                            "CPU",
                            format!("High system load detected: {:.2}", load),
                            CheckStatus::Warning,
                            CheckCategory::Performance,
                            "Investigate CPU-intensive processes.",
                        )
                    } else {
                        result(
                            "CPU",
                            format!("System load is normal: {:.2}", load),
                            CheckStatus::Pass,
                            CheckCategory::Performance,
                            "",
                        )
                    }
                }
                Err(_) => result(
                    "CPU",
                    "Cannot read /proc/loadavg",
                    CheckStatus::Warning,
                    CheckCategory::Performance,
                    "Check procfs availability.",
                ),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            result(
                "CPU",
                "CPU load check is currently Linux-optimized",
                CheckStatus::Warning,
                CheckCategory::Performance,
                "Run on Linux for detailed load diagnostics.",
            )
        }
    }

    async fn check_memory_usage(&self) -> CheckResult {
        #[cfg(target_os = "linux")]
        {
            if let Some((total_kb, available_kb)) = linux_meminfo() {
                let used_ratio = 1.0 - (available_kb as f64 / total_kb as f64);
                if used_ratio > 0.90 {
                    return result(
                        "MEMORY_USAGE",
                        format!("Very high memory usage: {:.1}%", used_ratio * 100.0),
                        CheckStatus::Warning,
                        CheckCategory::Performance,
                        "Reduce memory pressure before heavy workloads.",
                    );
                }
                return result(
                    "MEMORY_USAGE",
                    format!("Memory usage: {:.1}%", used_ratio * 100.0),
                    CheckStatus::Pass,
                    CheckCategory::Performance,
                    "",
                );
            }
            result(
                "MEMORY_USAGE",
                "Cannot read memory usage",
                CheckStatus::Warning,
                CheckCategory::Performance,
                "Check /proc/meminfo readability.",
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            result(
                "MEMORY_USAGE",
                "Memory usage check is currently Linux-optimized",
                CheckStatus::Warning,
                CheckCategory::Performance,
                "Run on Linux for detailed memory usage checks.",
            )
        }
    }

    async fn check_response_time(&self) -> CheckResult {
        let start = Instant::now();
        let probe = tokio::net::lookup_host("localhost:80").await;
        let elapsed = start.elapsed().as_millis();
        match probe {
            Ok(_) if elapsed <= 200 => result(
                "RESPONSE_TIME",
                format!("Local resolver latency: {} ms", elapsed),
                CheckStatus::Pass,
                CheckCategory::Performance,
                "",
            ),
            Ok(_) => result(
                "RESPONSE_TIME",
                format!("Local resolver latency is high: {} ms", elapsed),
                CheckStatus::Warning,
                CheckCategory::Performance,
                "Check system DNS and local network stack.",
            ),
            Err(e) => result(
                "RESPONSE_TIME",
                format!("Local resolver probe failed: {}", e),
                CheckStatus::Warning,
                CheckCategory::Performance,
                "Verify local DNS resolver health.",
            ),
        }
    }
}

#[async_trait]
impl DiagnosticChecker for PerformanceChecker {
    fn category(&self) -> CheckCategory {
        CheckCategory::Performance
    }

    fn name(&self) -> &str {
        "Performance Checker"
    }

    async fn run(&self) -> Vec<CheckResult> {
        vec![
            self.check_cpu().await,
            self.check_memory_usage().await,
            self.check_response_time().await,
        ]
    }
}
