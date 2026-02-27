use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::traits::{Channel, ChannelStatus};

struct ChannelHealth {
    last_check: Instant,
    consecutive_failures: u32,
    cooldown_until: Option<Instant>,
}

pub struct HealthMonitor {
    channels: Arc<RwLock<HashMap<String, Arc<RwLock<dyn Channel>>>>>,
    state: Arc<RwLock<HashMap<String, ChannelHealth>>>,
    check_interval: Duration,
    max_failures: u32,
    cooldown: Duration,
}

impl HealthMonitor {
    pub fn new(
        channels: Arc<RwLock<HashMap<String, Arc<RwLock<dyn Channel>>>>>,
    ) -> Self {
        Self {
            channels,
            state: Arc::new(RwLock::new(HashMap::new())),
            check_interval: Duration::from_secs(30),
            max_failures: 3,
            cooldown: Duration::from_secs(60),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.check_interval = interval;
        self
    }

    pub async fn start(&self) {
        let channels = self.channels.clone();
        let state = self.state.clone();
        let interval = self.check_interval;
        let max_failures = self.max_failures;
        let cooldown = self.cooldown;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                check_all(&channels, &state, max_failures, cooldown).await;
            }
        });

        info!("Channel health monitor started (interval: {:?})", self.check_interval);
    }
}

async fn check_all(
    channels: &Arc<RwLock<HashMap<String, Arc<RwLock<dyn Channel>>>>>,
    state: &Arc<RwLock<HashMap<String, ChannelHealth>>>,
    max_failures: u32,
    cooldown: Duration,
) {
    let chans = channels.read().await;
    for (name, channel) in chans.iter() {
        let ch = channel.read().await;
        let status = ch.status();
        drop(ch);

        let mut st = state.write().await;
        let health = st.entry(name.clone()).or_insert(ChannelHealth {
            last_check: Instant::now(),
            consecutive_failures: 0,
            cooldown_until: None,
        });

        if let Some(until) = health.cooldown_until {
            if Instant::now() < until {
                continue;
            }
            health.cooldown_until = None;
        }

        health.last_check = Instant::now();

        match status {
            ChannelStatus::Connected => {
                health.consecutive_failures = 0;
            }
            ChannelStatus::Connecting | ChannelStatus::Reconnecting { .. } => {
                // Connection lifecycle in progress; don't count as failure.
            }
            ChannelStatus::Error | ChannelStatus::Disconnected => {
                health.consecutive_failures += 1;
                warn!("Channel '{}' unhealthy (failures: {})", name, health.consecutive_failures);

                if health.consecutive_failures >= max_failures {
                    health.cooldown_until = Some(Instant::now() + cooldown);
                    info!("Channel '{}' entering cooldown ({:?})", name, cooldown);

                    // Attempt reconnect
                    drop(st);
                    let mut ch = channel.write().await;
                    let _ = ch.disconnect().await;
                    if let Err(e) = ch.connect().await {
                        warn!("Channel '{}' reconnect failed: {}", name, e);
                    } else {
                        info!("Channel '{}' reconnected", name);
                    }
                    continue;
                }
            }
        }
    }
}
