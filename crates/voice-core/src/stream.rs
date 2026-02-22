use bytes::Bytes;
use std::sync::Arc;

#[allow(dead_code)]
pub struct AudioStream {
    sample_rate: u32,
    channels: u16,
    bits_per_sample: u16,
    codec: AudioCodec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Pcm,
    Opus,
    Mp3,
    Webm,
}

impl AudioStream {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
            bits_per_sample: 16,
            codec: AudioCodec::Pcm,
        }
    }

    pub fn with_codec(mut self, codec: AudioCodec) -> Self {
        self.codec = codec;
        self
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn bits_per_sample(&self) -> u16 {
        self.bits_per_sample
    }

    pub fn codec(&self) -> AudioCodec {
        self.codec
    }

    pub fn bytes_per_sample(&self) -> u32 {
        (self.bits_per_sample / 8) as u32 * self.channels as u32
    }

    pub fn bytes_per_second(&self) -> u32 {
        self.bytes_per_sample() * self.sample_rate
    }
}

impl Default for AudioStream {
    fn default() -> Self {
        Self::new(48000, 2)
    }
}

pub struct AudioFrame {
    pub data: Bytes,
    pub timestamp: u64,
    pub sequence: u32,
}

impl AudioFrame {
    pub fn new(data: Vec<u8>, timestamp: u64, sequence: u32) -> Self {
        Self {
            data: Bytes::from(data),
            timestamp,
            sequence,
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

pub struct AudioBuffer {
    frames: Vec<AudioFrame>,
    max_frames: usize,
}

impl AudioBuffer {
    pub fn new(max_frames: usize) -> Self {
        Self {
            frames: Vec::new(),
            max_frames,
        }
    }

    pub fn push(&mut self, frame: AudioFrame) {
        if self.frames.len() >= self.max_frames {
            self.frames.remove(0);
        }
        self.frames.push(frame);
    }

    pub fn clear(&mut self) {
        self.frames.clear();
    }

    pub fn frames(&self) -> &[AudioFrame] {
        &self.frames
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn concatenate(&self) -> Bytes {
        let mut combined = Vec::new();
        for frame in &self.frames {
            combined.extend_from_slice(&frame.data);
        }
        Bytes::from(combined)
    }
}

impl Default for AudioBuffer {
    fn default() -> Self {
        Self::new(100)
    }
}

pub struct AudioProcessor {
    stream: AudioStream,
    buffer: Arc<parking_lot::Mutex<AudioBuffer>>,
}

impl AudioProcessor {
    pub fn new(stream: AudioStream) -> Self {
        Self {
            stream,
            buffer: Arc::new(parking_lot::Mutex::new(AudioBuffer::default())),
        }
    }

    pub fn process_frame(&self, frame: AudioFrame) {
        let mut buffer = self.buffer.lock();
        buffer.push(frame);
    }

    pub fn get_buffer(&self) -> Bytes {
        let buffer = self.buffer.lock();
        buffer.concatenate()
    }

    pub fn clear_buffer(&self) {
        let mut buffer = self.buffer.lock();
        buffer.clear();
    }

    pub fn stream(&self) -> &AudioStream {
        &self.stream
    }
}

#[allow(dead_code)]
pub struct AudioStreamGenerator {
    sample_rate: u32,
    channels: u16,
    sequence: u32,
    timestamp: u64,
}

impl AudioStreamGenerator {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
            sequence: 0,
            timestamp: 0,
        }
    }

    pub fn next_frame(&mut self) -> AudioFrame {
        let frame = AudioFrame::new(
            vec![0u8; (self.sample_rate * self.channels as u32 * 2 / 50) as usize],
            self.timestamp,
            self.sequence,
        );
        self.sequence += 1;
        self.timestamp += 20;
        frame
    }
}

#[allow(dead_code)]
pub fn create_audio_stream(sample_rate: u32, channels: u16) -> AudioStreamGenerator {
    AudioStreamGenerator::new(sample_rate, channels)
}

#[allow(dead_code)]
pub fn resample_audio(audio: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
    if from_rate == to_rate {
        return audio.to_vec();
    }

    let ratio = to_rate as f64 / from_rate as f64;
    let new_len = (audio.len() as f64 * ratio) as usize;
    let mut resampled = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_idx = (i as f64 / ratio) as usize;
        if src_idx < audio.len() {
            resampled.push(audio[src_idx]);
        }
    }

    resampled
}

#[allow(dead_code)]
pub fn mix_audio(channels: &[&[i16]]) -> Vec<i16> {
    if channels.is_empty() {
        return Vec::new();
    }

    let max_len = channels.iter().map(|c| c.len()).max().unwrap_or(0);
    let mut mixed = vec![0i16; max_len];

    for channel in channels {
        for (i, &sample) in channel.iter().enumerate() {
            let sum = mixed[i] as i32 + sample as i32;
            mixed[i] = sum.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
    }

    mixed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_stream_new() {
        let stream = AudioStream::new(48000, 2);
        assert_eq!(stream.sample_rate(), 48000);
        assert_eq!(stream.channels(), 2);
        assert_eq!(stream.codec(), AudioCodec::Pcm);
    }

    #[test]
    fn test_audio_stream_with_codec() {
        let stream = AudioStream::new(48000, 2).with_codec(AudioCodec::Opus);
        assert_eq!(stream.codec(), AudioCodec::Opus);
    }

    #[test]
    fn test_audio_stream_bytes_per_sample() {
        let stream = AudioStream::new(48000, 2);
        assert_eq!(stream.bytes_per_sample(), 4);
        assert_eq!(stream.bytes_per_second(), 192000);
    }

    #[test]
    fn test_audio_frame_new() {
        let frame = AudioFrame::new(vec![1, 2, 3, 4], 100, 1);
        assert_eq!(frame.len(), 4);
        assert!(!frame.is_empty());
        assert_eq!(frame.timestamp, 100);
        assert_eq!(frame.sequence, 1);
    }

    #[test]
    fn test_audio_buffer() {
        let mut buffer = AudioBuffer::new(3);

        buffer.push(AudioFrame::new(vec![1], 0, 0));
        buffer.push(AudioFrame::new(vec![2], 1, 1));
        buffer.push(AudioFrame::new(vec![3], 2, 2));

        assert_eq!(buffer.len(), 3);

        buffer.push(AudioFrame::new(vec![4], 3, 3));

        assert_eq!(buffer.len(), 3);

        buffer.clear();
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_audio_buffer_concatenate() {
        let mut buffer = AudioBuffer::new(10);
        buffer.push(AudioFrame::new(vec![1, 2], 0, 0));
        buffer.push(AudioFrame::new(vec![3, 4], 1, 1));

        let combined = buffer.concatenate();
        assert_eq!(combined, bytes::Bytes::from(vec![1, 2, 3, 4]));
    }

    #[test]
    fn test_audio_stream_generator() {
        let mut generator = AudioStreamGenerator::new(48000, 2);

        let frame1 = generator.next_frame();
        let frame2 = generator.next_frame();

        assert_eq!(frame1.sequence, 0);
        assert_eq!(frame2.sequence, 1);
        assert_eq!(frame2.timestamp, 20);
    }

    #[test]
    fn test_resample_audio_same_rate() {
        let audio = vec![1i16, 2, 3, 4, 5];
        let resampled = resample_audio(&audio, 48000, 48000);
        assert_eq!(resampled, audio);
    }

    #[test]
    fn test_resample_audio_upscale() {
        let audio = vec![1i16, 2, 3, 4];
        let resampled = resample_audio(&audio, 16000, 32000);
        assert!(resampled.len() >= audio.len());
    }

    #[test]
    fn test_mix_audio_empty() {
        let mixed = mix_audio(&[]);
        assert!(mixed.is_empty());
    }

    #[test]
    fn test_mix_audio_single_channel() {
        let audio = vec![1i16, 2, 3, 4];
        let mixed = mix_audio(&[&audio]);
        assert_eq!(mixed, audio);
    }

    #[test]
    fn test_mix_audio_multiple_channels() {
        let ch1 = vec![1000i16, 2000];
        let ch2 = vec![1000i16, 2000];
        let mixed = mix_audio(&[&ch1, &ch2]);
        assert_eq!(mixed, vec![2000i16, 4000]);
    }
}
