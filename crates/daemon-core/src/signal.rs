//! Signal Handling - Real implementation using tokio signals

use crate::DaemonResult;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Interrupt,
    Terminate,
    HangUp,
}

#[derive(Debug, Clone)]
pub enum SignalEvent {
    Signal(Signal),
    Shutdown,
}

pub struct SignalHandler {
    tx: broadcast::Sender<SignalEvent>,
}

impl SignalHandler {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SignalEvent> {
        self.tx.subscribe()
    }

    pub fn register(&self) -> DaemonResult<()> {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut sigint = match signal(SignalKind::interrupt()) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to register SIGINT: {}", e);
                        return;
                    }
                };
                let mut sigterm = match signal(SignalKind::terminate()) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to register SIGTERM: {}", e);
                        return;
                    }
                };
                let mut sighup = match signal(SignalKind::hangup()) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to register SIGHUP: {}", e);
                        return;
                    }
                };
                loop {
                    tokio::select! {
                        _ = sigint.recv() => { let _ = tx.send(SignalEvent::Signal(Signal::Interrupt)); }
                        _ = sigterm.recv() => { let _ = tx.send(SignalEvent::Signal(Signal::Terminate)); }
                        _ = sighup.recv() => { let _ = tx.send(SignalEvent::Signal(Signal::HangUp)); }
                    }
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
                let _ = tx.send(SignalEvent::Signal(Signal::Interrupt));
            }
        });
        Ok(())
    }
}

impl Default for SignalHandler {
    fn default() -> Self {
        Self::new()
    }
}
