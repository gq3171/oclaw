/// Requirement gates — check bins, env vars, config paths, OS filter.
use crate::manifest::{RequiresSpec, SkillManifest};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct GateResult {
    pub passed: bool,
    pub missing_bins: Vec<String>,
    pub missing_env: Vec<String>,
    pub missing_config: Vec<String>,
    pub os_mismatch: bool,
}

/// Check all requirement gates for a skill manifest.
pub fn check_gates(
    manifest: &SkillManifest,
    config_lookup: &dyn Fn(&str) -> bool,
) -> GateResult {
    let mut result = GateResult {
        passed: true,
        missing_bins: Vec::new(),
        missing_env: Vec::new(),
        missing_config: Vec::new(),
        os_mismatch: false,
    };

    // OS gate
    if let Some(meta) = &manifest.metadata
        && let Some(oc) = &meta.openclaw
        && !oc.os.is_empty()
    {
        let current = std::env::consts::OS; // "windows", "linux", "macos"
        let matches = oc.os.iter().any(|o| {
            let o = o.to_lowercase();
            o == current || (o == "macos" && current == "macos")
                || (o == "darwin" && current == "macos")
        });
        if !matches {
            result.os_mismatch = true;
            result.passed = false;
        }
    }

    let requires = manifest.metadata.as_ref()
        .and_then(|m| m.openclaw.as_ref())
        .and_then(|oc| oc.requires.as_ref());

    let Some(req) = requires else { return result };

    check_bins(req, &mut result);
    check_env(req, &mut result);
    check_config(req, config_lookup, &mut result);

    result
}

fn check_bins(req: &RequiresSpec, result: &mut GateResult) {
    for bin in &req.bins {
        if which(bin).is_none() {
            result.missing_bins.push(bin.clone());
            result.passed = false;
        }
    }
}

fn check_env(req: &RequiresSpec, result: &mut GateResult) {
    for var in &req.env {
        if std::env::var(var).unwrap_or_default().is_empty() {
            result.missing_env.push(var.clone());
            result.passed = false;
        }
    }
}

fn check_config(req: &RequiresSpec, lookup: &dyn Fn(&str) -> bool, result: &mut GateResult) {
    for path in &req.config {
        if !lookup(path) {
            result.missing_config.push(path.clone());
            result.passed = false;
        }
    }
}

/// Simple `which` — search PATH for a binary.
pub fn which(bin: &str) -> Option<PathBuf> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let sep = if cfg!(windows) { ';' } else { ':' };
    let extensions: Vec<&str> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".to_string())
            .leak() // static lifetime for split
            .split(';')
            .collect()
    } else {
        vec![""]
    };

    for dir in path_var.split(sep) {
        for ext in &extensions {
            let candidate = PathBuf::from(dir).join(format!("{bin}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
