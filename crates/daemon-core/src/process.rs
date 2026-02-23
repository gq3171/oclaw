//! Process Management - Real implementation using sysinfo

use serde::{Deserialize, Serialize};
use sysinfo::{System, Pid};

use crate::DaemonResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessStatus {
    Running,
    Sleeping,
    Stopped,
    Zombie,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub status: ProcessStatus,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
}

pub struct ProcessManager {
    sys: System,
}

impl ProcessManager {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        Self { sys }
    }

    pub fn refresh(&mut self) {
        self.sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    }

    pub fn get(&self, pid: u32) -> Option<ProcessInfo> {
        self.sys.process(Pid::from_u32(pid)).map(|p| ProcessInfo {
            pid,
            name: p.name().to_string_lossy().into_owned(),
            status: match p.status() {
                sysinfo::ProcessStatus::Run => ProcessStatus::Running,
                sysinfo::ProcessStatus::Sleep => ProcessStatus::Sleeping,
                sysinfo::ProcessStatus::Stop => ProcessStatus::Stopped,
                sysinfo::ProcessStatus::Zombie => ProcessStatus::Zombie,
                _ => ProcessStatus::Unknown,
            },
            cpu_usage: p.cpu_usage(),
            memory_bytes: p.memory(),
        })
    }

    pub fn is_running(&self, pid: u32) -> bool {
        self.sys.process(Pid::from_u32(pid)).is_some()
    }

    pub fn list(&self) -> Vec<ProcessInfo> {
        self.sys.processes().iter().map(|(pid, p)| ProcessInfo {
            pid: pid.as_u32(),
            name: p.name().to_string_lossy().into_owned(),
            status: match p.status() {
                sysinfo::ProcessStatus::Run => ProcessStatus::Running,
                sysinfo::ProcessStatus::Sleep => ProcessStatus::Sleeping,
                sysinfo::ProcessStatus::Stop => ProcessStatus::Stopped,
                sysinfo::ProcessStatus::Zombie => ProcessStatus::Zombie,
                _ => ProcessStatus::Unknown,
            },
            cpu_usage: p.cpu_usage(),
            memory_bytes: p.memory(),
        }).collect()
    }

    pub fn kill(&self, pid: u32) -> DaemonResult<bool> {
        match self.sys.process(Pid::from_u32(pid)) {
            Some(p) => Ok(p.kill()),
            None => Err(crate::DaemonError::NotFound(format!("pid {pid}"))),
        }
    }
}

impl Default for ProcessManager {
    fn default() -> Self { Self::new() }
}
