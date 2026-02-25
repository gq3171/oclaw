//! Edge TTS provider (Microsoft Edge free TTS API).

use async_trait::async_trait;
use reqwest::Client;
use std::path::Path;
use tracing::debug;

use super::{SynthesizeResult, TtsError, TtsProviderBackend};

pub struct EdgeTts {
    client: Client,
}

impl EdgeTts {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

impl Default for EdgeTts {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TtsProviderBackend for EdgeTts {
    fn id(&self) -> &str {
        "edge"
    }

    fn default_voice(&self) -> &str {
        "en-US-AriaNeural"
    }

    fn available_voices(&self) -> Vec<&str> {
        vec![
            "en-US-AriaNeural",
            "en-US-GuyNeural",
            "en-US-JennyNeural",
            "zh-CN-XiaoxiaoNeural",
            "zh-CN-YunxiNeural",
            "ja-JP-NanamiNeural",
        ]
    }

    async fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        _output_path: &Path,
    ) -> Result<SynthesizeResult, TtsError> {
        let voice = voice.unwrap_or(self.default_voice());
        debug!(provider = "edge", voice, "Synthesizing speech");

        // Edge TTS uses a WebSocket-based protocol.
        // This is a simplified HTTP fallback placeholder — real implementation
        // would use the edge-tts WebSocket protocol.
        let ssml = format!(
            r#"<speak version="1.0" xmlns="http://www.w3.org/2001/10/synthesis" xml:lang="en-US">
  <voice name="{}">{}</voice>
</speak>"#,
            voice,
            xml_escape(text)
        );

        // For now, write an empty placeholder and return an error
        // indicating the full WebSocket implementation is needed.
        let _ = ssml;
        let _ = &self.client;

        Err(TtsError::Api(
            "Edge TTS requires WebSocket protocol — use edge-tts CLI as fallback".to_string(),
        ))
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
