//! System Monitoring - Real implementation using sysinfo

use serde::{Deserialize, Serialize};
use sysinfo::{System, Disks};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStats {
    pub cpu_usage: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub disks: Vec<DiskInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub mount_point: String,
    pub total: u64,
    pub used: u64,
}

pub struct SystemMonitor {
    sys: System,
}

impl SystemMonitor {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self { sys }
    }

    pub fn refresh(&mut self) {
        self.sys.refresh_all();
    }

    pub fn stats(&self) -> SystemStats {
        let disks = Disks::new_with_refreshed_list();
        SystemStats {
            cpu_usage: self.sys.global_cpu_usage(),
            memory_used: self.sys.used_memory(),
            memory_total: self.sys.total_memory(),
            disks: disks.iter().map(|d| DiskInfo {
                mount_point: d.mount_point().to_string_lossy().into_owned(),
                total: d.total_space(),
                used: d.total_space() - d.available_space(),
            }).collect(),
        }
    }

    pub fn memory_percent(&self) -> f32 {
        let total = self.sys.total_memory();
        if total == 0 { return 0.0; }
        (self.sys.used_memory() as f32 / total as f32) * 100.0
    }

    pub fn system_name(&self) -> String {
        System::name().unwrap_or_else(|| "Unknown".into())
    }
}

impl Default for SystemMonitor {
    fn default() -> Self { Self::new() }
}
