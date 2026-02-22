use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{VoiceChannel, VoiceState};

#[derive(Clone)]
pub struct VoiceConnection {
    id: String,
    channel_id: String,
    state: VoiceState,
    gateway_url: String,
}

impl VoiceConnection {
    pub fn new(id: String, channel_id: String) -> Self {
        Self {
            id,
            channel_id,
            state: VoiceState::Disconnected,
            gateway_url: String::new(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn channel_id(&self) -> &str {
        &self.channel_id
    }

    pub fn state(&self) -> VoiceState {
        self.state
    }

    pub async fn connect(&mut self, gateway_url: &str) -> Result<()> {
        self.gateway_url = gateway_url.to_string();
        self.state = VoiceState::Connecting;
        
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        self.state = VoiceState::Connected;
        Ok(())
    }

    pub async fn send_audio(&mut self, _audio: &[u8]) -> Result<()> {
        if self.state != VoiceState::Connected {
            anyhow::bail!("Not connected");
        }
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        self.state = VoiceState::Disconnected;
        Ok(())
    }
}

#[async_trait]
impl VoiceChannel for VoiceConnection {
    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    async fn connect(&mut self) -> Result<()> {
        self.connect("wss://voice.example.com").await
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.disconnect().await
    }

    async fn send_audio(&mut self, audio: &[u8]) -> Result<()> {
        self.send_audio(audio).await
    }

    fn state(&self) -> VoiceState {
        self.state
    }
}

pub struct VoiceConnectionManager {
    connections: Arc<RwLock<Vec<VoiceConnection>>>,
}

impl VoiceConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn create_connection(&self, channel_id: &str) -> Result<VoiceConnection> {
        let id = uuid::Uuid::new_v4().to_string();
        let connection = VoiceConnection::new(id, channel_id.to_string());
        
        let mut connections = self.connections.write().await;
        connections.push(connection.clone());
        
        Ok(connection)
    }

    pub async fn get_connection(&self, id: &str) -> Option<VoiceConnection> {
        let connections = self.connections.read().await;
        connections.iter().find(|c| c.id() == id).cloned()
    }

    pub async fn remove_connection(&self, id: &str) -> Result<()> {
        let mut connections = self.connections.write().await;
        connections.retain(|c| c.id() != id);
        Ok(())
    }

    pub async fn list_connections(&self) -> Vec<VoiceConnection> {
        let connections = self.connections.read().await;
        connections.clone()
    }
}

impl Default for VoiceConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_voice_connection_new() {
        let conn = VoiceConnection::new("test-id".to_string(), "channel-1".to_string());
        assert_eq!(conn.id(), "test-id");
        assert_eq!(conn.channel_id(), "channel-1");
        assert_eq!(conn.state(), VoiceState::Disconnected);
    }

    #[tokio::test]
    async fn test_voice_connection_connect() {
        let mut conn = VoiceConnection::new("test-id".to_string(), "channel-1".to_string());
        conn.connect("wss://example.com/voice").await.unwrap();
        assert_eq!(conn.state(), VoiceState::Connected);
    }

    #[tokio::test]
    async fn test_voice_connection_disconnect() {
        let mut conn = VoiceConnection::new("test-id".to_string(), "channel-1".to_string());
        conn.connect("wss://example.com/voice").await.unwrap();
        assert_eq!(conn.state(), VoiceState::Connected);
        
        conn.disconnect().await.unwrap();
        assert_eq!(conn.state(), VoiceState::Disconnected);
    }

    #[tokio::test]
    async fn test_voice_connection_manager() {
        let manager = VoiceConnectionManager::new();
        
        let conn = manager.create_connection("channel-1").await.unwrap();
        assert_eq!(conn.channel_id(), "channel-1");
        
        let retrieved = manager.get_connection(conn.id()).await;
        assert!(retrieved.is_some());
        
        manager.remove_connection(conn.id()).await.unwrap();
        
        let retrieved = manager.get_connection(conn.id()).await;
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_voice_connection_clone() {
        let conn1 = VoiceConnection::new("test-id".to_string(), "channel-1".to_string());
        let conn2 = conn1.clone();
        assert_eq!(conn1.id(), conn2.id());
        assert_eq!(conn1.channel_id(), conn2.channel_id());
    }
}
