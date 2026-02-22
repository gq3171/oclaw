mod connection;
mod stt;
mod stream;
mod tts;

pub use connection::{VoiceConnection, VoiceConnectionManager};
pub use stt::{STTProvider, STTResult};
pub use stream::{AudioStream, AudioFrame, AudioBuffer, AudioProcessor};
pub use tts::{TTSProvider, TTSVoice};

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoiceEvent {
    SpeakingStarted { user_id: String },
    SpeakingStopped { user_id: String },
    VoicePacket { user_id: String, audio: Vec<u8> },
    TextMessage { user_id: String, text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceState {
    Disconnected,
    Connecting,
    Connected,
    Speaking,
    Listening,
}

#[async_trait]
pub trait VoiceChannel: Send + Sync {
    fn channel_id(&self) -> &str;
    async fn connect(&mut self) -> Result<()>;
    async fn disconnect(&mut self) -> Result<()>;
    async fn send_audio(&mut self, audio: &[u8]) -> Result<()>;
    fn state(&self) -> VoiceState;
}

#[async_trait]
pub trait TTSEngine: Send + Sync {
    async fn speak(&self, text: &str, voice: &TTSVoice) -> Result<Vec<u8>>;
    fn list_voices(&self) -> Vec<TTSVoice>;
    fn provider(&self) -> TTSProvider;
}

#[async_trait]
pub trait STTEngine: Send + Sync {
    async fn transcribe(&self, audio: &[u8]) -> Result<STTResult>;
    fn provider(&self) -> STTProvider;
    fn language(&self) -> &str;
    fn set_language(&mut self, lang: &str);
}
