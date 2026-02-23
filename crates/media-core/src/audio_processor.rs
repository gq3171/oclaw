//! Audio Processing

use std::io::Cursor;

/// Supported audio formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Mp3,
    Ogg,
    Flac,
    Aac,
}

impl AudioFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "wav" => Some(AudioFormat::Wav),
            "mp3" => Some(AudioFormat::Mp3),
            "ogg" => Some(AudioFormat::Ogg),
            "flac" => Some(AudioFormat::Flac),
            "aac" => Some(AudioFormat::Aac),
            _ => None,
        }
    }
    
    pub fn mime_type(&self) -> &str {
        match self {
            AudioFormat::Wav => "audio/wav",
            AudioFormat::Mp3 => "audio/mpeg",
            AudioFormat::Ogg => "audio/ogg",
            AudioFormat::Flac => "audio/flac",
            AudioFormat::Aac => "audio/aac",
        }
    }
}

/// Audio codec
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Pcm,
    Mp3,
    Vorbis,
    Flac,
    Aac,
}

/// Audio configuration
#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub codec: AudioCodec,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 44100,
            channels: 2,
            bits_per_sample: 16,
            codec: AudioCodec::Pcm,
        }
    }
}

/// Audio metadata
#[derive(Debug, Clone)]
pub struct AudioMetadata {
    pub duration_secs: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub format: AudioFormat,
    pub codec: AudioCodec,
}

/// Audio processor
pub struct AudioProcessor {
    _config: AudioConfig,
}

impl AudioProcessor {
    pub fn new(config: AudioConfig) -> Self {
        Self { _config: config }
    }
    
    /// Load audio from WAV bytes
    pub fn load_wav(&self, data: &[u8]) -> MediaResult<(Vec<i16>, AudioMetadata)> {
        let cursor = Cursor::new(data);
        let reader = hound::WavReader::new(cursor)
            .map_err(|e| MediaError::AudioError(e.to_string()))?;
        
        let spec = reader.spec();
        let samples: Vec<i16> = reader.into_samples()
            .filter_map(|s| s.ok())
            .collect();
        
        let duration = samples.len() as f64 / (spec.sample_rate as f64 * spec.channels as f64);
        
        let metadata = AudioMetadata {
            duration_secs: duration,
            sample_rate: spec.sample_rate,
            channels: spec.channels,
            bits_per_sample: spec.bits_per_sample,
            format: AudioFormat::Wav,
            codec: AudioCodec::Pcm,
        };
        
        Ok((samples, metadata))
    }
    
    /// Save audio to WAV
    pub fn save_wav(&self, samples: &[i16], config: &AudioConfig) -> MediaResult<Vec<u8>> {
        let spec = hound::WavSpec {
            channels: config.channels,
            sample_rate: config.sample_rate,
            bits_per_sample: config.bits_per_sample,
            sample_format: hound::SampleFormat::Int,
        };
        
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut buffer, spec)
                .map_err(|e| MediaError::AudioError(e.to_string()))?;
            
            for sample in samples {
                writer.write_sample(*sample)
                    .map_err(|e| MediaError::AudioError(e.to_string()))?;
            }
            
            writer.finalize()
                .map_err(|e| MediaError::AudioError(e.to_string()))?;
        }
        
        Ok(buffer.into_inner())
    }
    
    /// Convert sample rate
    pub fn resample(&self, samples: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
        if from_rate == to_rate {
            return samples.to_vec();
        }
        
        let ratio = to_rate as f64 / from_rate as f64;
        let new_len = (samples.len() as f64 * ratio) as usize;
        let mut result = Vec::with_capacity(new_len);
        
        for i in 0..new_len {
            let src_idx = i as f64 / ratio;
            let idx = src_idx as usize;
            
            if idx + 1 < samples.len() {
                let frac = src_idx - idx as f64;
                let sample = ((samples[idx] as f64 * (1.0 - frac)) + (samples[idx + 1] as f64 * frac)) as i16;
                result.push(sample);
            } else if idx < samples.len() {
                result.push(samples[idx]);
            }
        }
        
        result
    }
    
    /// Convert stereo to mono
    pub fn stereo_to_mono(&self, samples: &[i16], channels: u16) -> Vec<i16> {
        if channels == 1 {
            return samples.to_vec();
        }
        
        let mono_len = samples.len() / channels as usize;
        let mut mono = Vec::with_capacity(mono_len);
        
        for i in 0..mono_len {
            let mut sum = 0i32;
            for c in 0..channels as usize {
                sum += samples[i * channels as usize + c] as i32;
            }
            mono.push((sum / channels as i32) as i16);
        }
        
        mono
    }
    
    /// Convert mono to stereo
    pub fn mono_to_stereo(&self, samples: &[i16]) -> Vec<i16> {
        let mut stereo = Vec::with_capacity(samples.len() * 2);
        
        for &sample in samples {
            stereo.push(sample);
            stereo.push(sample);
        }
        
        stereo
    }
    
    /// Apply gain
    pub fn apply_gain(&self, samples: &[i16], gain_db: f32) -> Vec<i16> {
        let gain = 10f32.powf(gain_db / 20.0);
        let max_val = i16::MAX as f32;
        
        samples.iter()
            .map(|&s| {
                let scaled = s as f32 * gain;
                scaled.clamp(-max_val, max_val) as i16
            })
            .collect()
    }
    
    /// Normalize audio
    pub fn normalize(&self, samples: &[i16]) -> Vec<i16> {
        let max_sample = samples.iter()
            .map(|s| s.abs() as i32)
            .max()
            .unwrap_or(1);
        
        let target_max = i16::MAX as i32 / 2;
        let scale = target_max as f32 / max_sample as f32;
        
        samples.iter()
            .map(|&s| (s as f32 * scale) as i16)
            .collect()
    }
    
    /// Fade in
    pub fn fade_in(&self, samples: &[i16], duration_secs: f64, sample_rate: u32) -> Vec<i16> {
        let fade_len = (duration_secs * sample_rate as f64) as usize;
        let fade_len = fade_len.min(samples.len());
        
        let mut result = samples.to_vec();
        
        for (i, sample) in result.iter_mut().enumerate().take(fade_len) {
            let gain = i as f32 / fade_len as f32;
            *sample = (*sample as f32 * gain) as i16;
        }
        
        result
    }
    
    /// Fade out
    pub fn fade_out(&self, samples: &[i16], duration_secs: f64, sample_rate: u32) -> Vec<i16> {
        let fade_len = (duration_secs * sample_rate as f64) as usize;
        let fade_len = fade_len.min(samples.len());
        
        let mut result = samples.to_vec();
        let start = samples.len() - fade_len;
        
        for (i, sample) in result.iter_mut().enumerate().skip(start) {
            let gain = (fade_len - (i - start)) as f32 / fade_len as f32;
            *sample = (*sample as f32 * gain) as i16;
        }
        
        result
    }
    
    /// Get audio duration
    pub fn duration(&self, samples: &[i16], sample_rate: u32, channels: u16) -> f64 {
        samples.len() as f64 / (sample_rate as f64 * channels as f64)
    }
}

use crate::{MediaError, MediaResult};
