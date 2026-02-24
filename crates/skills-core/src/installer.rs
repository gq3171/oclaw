/// Skill installer — run install specs (brew, node, go, uv, download).
use crate::manifest::InstallSpec;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct InstallResult {
    pub ok: bool,
    pub message: String,
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

/// Run a single install spec.
pub async fn run_install(spec: &InstallSpec, timeout_secs: u64) -> InstallResult {
    let timeout = std::time::Duration::from_secs(timeout_secs.clamp(1, 900));
    match spec.kind.as_str() {
        "brew" => install_brew(spec, timeout).await,
        "node" => install_node(spec, timeout).await,
        "go" => install_go(spec, timeout).await,
        "uv" => install_uv(spec, timeout).await,
        "download" => install_download(spec, timeout).await,
        other => InstallResult {
            ok: false,
            message: format!("Unknown installer kind: {other}"),
            stdout: String::new(),
            stderr: String::new(),
            code: None,
        },
    }
}

async fn run_cmd(args: &[&str], timeout: std::time::Duration) -> InstallResult {
    let Some((cmd, rest)) = args.split_first() else {
        return InstallResult { ok: false, message: "Empty command".into(), stdout: String::new(), stderr: String::new(), code: None };
    };
    info!("Running: {} {}", cmd, rest.join(" "));
    let child = Command::new(cmd)
        .args(rest)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let Ok(child) = child else {
        return InstallResult { ok: false, message: format!("Failed to spawn {cmd}"), stdout: String::new(), stderr: String::new(), code: None };
    };
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            let code = output.status.code();
            InstallResult {
                ok: output.status.success(),
                message: if output.status.success() { "OK".into() } else { format!("Exit code: {}", code.unwrap_or(-1)) },
                stdout: String::from_utf8_lossy(&output.stdout).into(),
                stderr: String::from_utf8_lossy(&output.stderr).into(),
                code,
            }
        }
        Ok(Err(e)) => InstallResult { ok: false, message: format!("IO error: {e}"), stdout: String::new(), stderr: String::new(), code: None },
        Err(_) => InstallResult { ok: false, message: "Timeout".into(), stdout: String::new(), stderr: String::new(), code: None },
    }
}

async fn install_brew(spec: &InstallSpec, timeout: std::time::Duration) -> InstallResult {
    let Some(formula) = &spec.formula else {
        return InstallResult { ok: false, message: "brew spec missing formula".into(), stdout: String::new(), stderr: String::new(), code: None };
    };
    run_cmd(&["brew", "install", formula], timeout).await
}

async fn install_node(spec: &InstallSpec, timeout: std::time::Duration) -> InstallResult {
    let Some(package) = &spec.package else {
        return InstallResult { ok: false, message: "node spec missing package".into(), stdout: String::new(), stderr: String::new(), code: None };
    };
    // Try npm by default
    run_cmd(&["npm", "install", "-g", "--ignore-scripts", package], timeout).await
}

async fn install_go(spec: &InstallSpec, timeout: std::time::Duration) -> InstallResult {
    let Some(module) = &spec.module else {
        return InstallResult { ok: false, message: "go spec missing module".into(), stdout: String::new(), stderr: String::new(), code: None };
    };
    run_cmd(&["go", "install", module], timeout).await
}

async fn install_uv(spec: &InstallSpec, timeout: std::time::Duration) -> InstallResult {
    let Some(package) = &spec.package else {
        return InstallResult { ok: false, message: "uv spec missing package".into(), stdout: String::new(), stderr: String::new(), code: None };
    };
    run_cmd(&["uv", "tool", "install", package], timeout).await
}

async fn install_download(spec: &InstallSpec, timeout: std::time::Duration) -> InstallResult {
    let Some(url) = &spec.url else {
        return InstallResult { ok: false, message: "download spec missing url".into(), stdout: String::new(), stderr: String::new(), code: None };
    };
    let target = spec.target_dir.as_deref().unwrap_or(".");
    // Use curl for download + extract
    let archive = spec.archive.as_deref().unwrap_or("");
    if archive.ends_with(".zip") {
        let tmp = std::env::temp_dir().join("oclaw_dl.zip");
        let tmp_str = tmp.to_string_lossy().to_string();
        let r = run_cmd(&["curl", "-fsSL", "-o", &tmp_str, url], timeout).await;
        if !r.ok { return r; }
        run_cmd(&["unzip", "-o", &tmp_str, "-d", target], timeout).await
    } else {
        // tar.gz / tar.bz2
        let tar_flag = if archive.ends_with(".bz2") { "-xjf" } else { "-xzf" };
        run_cmd(&["curl", "-fsSL", url, "|", "tar", tar_flag, "-", "-C", target], timeout).await
            .pipe_fallback(url, tar_flag, target, timeout).await
    }
}

impl InstallResult {
    /// Fallback: if piped command fails, try two-step download+extract.
    async fn pipe_fallback(self, url: &str, tar_flag: &str, target: &str, timeout: std::time::Duration) -> InstallResult {
        if self.ok { return self; }
        warn!("Pipe download failed, trying two-step");
        let tmp = std::env::temp_dir().join("oclaw_dl.tar");
        let tmp_str = tmp.to_string_lossy().to_string();
        let r = run_cmd(&["curl", "-fsSL", "-o", &tmp_str, url], timeout).await;
        if !r.ok { return r; }
        run_cmd(&["tar", tar_flag, &tmp_str, "-C", target], timeout).await
    }
}
