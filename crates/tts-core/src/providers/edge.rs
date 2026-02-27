//! Edge TTS provider (Microsoft Edge free TTS API).

use async_trait::async_trait;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

use super::{SynthesizeResult, TtsError, TtsProviderBackend};

#[derive(Debug, Clone, Default)]
pub struct EdgeTtsOptions {
    pub lang: Option<String>,
    pub output_format: Option<String>,
    pub save_subtitles: bool,
    pub proxy: Option<String>,
    pub rate: Option<String>,
    pub pitch: Option<String>,
    pub volume: Option<String>,
    pub timeout_ms: Option<u64>,
}

pub struct EdgeTts {
    options: EdgeTtsOptions,
}

impl EdgeTts {
    pub fn new() -> Self {
        Self {
            options: EdgeTtsOptions::default(),
        }
    }

    pub fn with_options(mut self, options: EdgeTtsOptions) -> Self {
        self.options = options;
        self
    }

    fn resolve_voice(&self, explicit_voice: Option<&str>) -> String {
        if let Some(v) = explicit_voice.map(str::trim).filter(|v| !v.is_empty()) {
            return v.to_string();
        }

        if let Some(lang) = self
            .options
            .lang
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            for candidate in self.available_voices() {
                if candidate
                    .to_ascii_lowercase()
                    .starts_with(&lang.to_ascii_lowercase())
                {
                    return candidate.to_string();
                }
            }
        }

        self.default_voice().to_string()
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
        output_path: &Path,
    ) -> Result<SynthesizeResult, TtsError> {
        let voice = self.resolve_voice(voice);
        debug!(
            provider = "edge",
            voice,
            lang = ?self.options.lang,
            output_format = ?self.options.output_format,
            "Synthesizing speech"
        );

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let output = output_path
            .to_str()
            .ok_or_else(|| TtsError::Api("Invalid output path".to_string()))?;
        let subtitle_output = if self.options.save_subtitles {
            Some(output_path.with_extension("vtt"))
        } else {
            None
        };
        let subtitle_output_str = subtitle_output.as_ref().and_then(|p| p.to_str());

        if let Err(primary_err) =
            run_edge_tts_binary(text, &voice, output, subtitle_output_str, &self.options).await
        {
            let py3 = run_edge_tts_python_module(
                "python3",
                text,
                &voice,
                output,
                subtitle_output_str,
                &self.options,
            )
            .await;
            let py = if py3.is_err() {
                run_edge_tts_python_module(
                    "python",
                    text,
                    &voice,
                    output,
                    subtitle_output_str,
                    &self.options,
                )
                .await
            } else {
                Ok(())
            };

            if let Err(py_err) = py {
                return Err(TtsError::Api(format!(
                    "Edge TTS failed (binary + python fallback): {}; {}",
                    primary_err, py_err
                )));
            }
        }

        let bytes_written = tokio::fs::metadata(output_path).await.map(|m| m.len())?;
        Ok(SynthesizeResult {
            output_path: output_path.to_path_buf(),
            duration_ms: None,
            bytes_written,
        })
    }
}

fn build_edge_tts_args(
    text: &str,
    voice: &str,
    output_path: &str,
    subtitle_output_path: Option<&str>,
    options: &EdgeTtsOptions,
) -> Vec<String> {
    let mut args = vec![
        "--voice".to_string(),
        voice.to_string(),
        "--text".to_string(),
        text.to_string(),
        "--write-media".to_string(),
        output_path.to_string(),
    ];

    if let Some(fmt) = options
        .output_format
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        args.push("--format".to_string());
        args.push(fmt.to_string());
    }
    if let Some(rate) = options
        .rate
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        args.push("--rate".to_string());
        args.push(rate.to_string());
    }
    if let Some(volume) = options
        .volume
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        args.push("--volume".to_string());
        args.push(volume.to_string());
    }
    if let Some(pitch) = options
        .pitch
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        args.push("--pitch".to_string());
        args.push(pitch.to_string());
    }
    if let Some(proxy) = options
        .proxy
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        args.push("--proxy".to_string());
        args.push(proxy.to_string());
    }
    if let Some(sub) = subtitle_output_path {
        args.push("--write-subtitles".to_string());
        args.push(sub.to_string());
    }

    args
}

async fn run_edge_tts_binary(
    text: &str,
    voice: &str,
    output_path: &str,
    subtitle_output_path: Option<&str>,
    options: &EdgeTtsOptions,
) -> Result<(), String> {
    let args = build_edge_tts_args(text, voice, output_path, subtitle_output_path, options);
    run_command("edge-tts", &[], &args, options.timeout_ms).await
}

async fn run_edge_tts_python_module(
    interpreter: &str,
    text: &str,
    voice: &str,
    output_path: &str,
    subtitle_output_path: Option<&str>,
    options: &EdgeTtsOptions,
) -> Result<(), String> {
    let args = build_edge_tts_args(text, voice, output_path, subtitle_output_path, options);
    run_command(interpreter, &["-m", "edge_tts"], &args, options.timeout_ms).await
}

async fn run_command(
    binary: &str,
    prefix_args: &[&str],
    args: &[String],
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    let mut cmd = Command::new(binary);
    cmd.kill_on_drop(true);
    for arg in prefix_args {
        cmd.arg(arg);
    }
    for arg in args {
        cmd.arg(arg);
    }

    let child = cmd
        .spawn()
        .map_err(|e| format!("{} spawn failed: {}", binary, e))?;

    let wait_fut = child.wait_with_output();
    let out = if let Some(ms) = timeout_ms.filter(|ms| *ms > 0) {
        tokio::time::timeout(Duration::from_millis(ms), wait_fut)
            .await
            .map_err(|_| format!("{} timeout after {}ms", binary, ms))?
            .map_err(|e| format!("{} wait failed: {}", binary, e))?
    } else {
        wait_fut
            .await
            .map_err(|e| format!("{} wait failed: {}", binary, e))?
    };

    if out.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(format!(
        "{} exit={} stdout='{}' stderr='{}'",
        binary, out.status, stdout, stderr
    ))
}
