//! TTS (Text-to-Speech) — multi-provider synthesis pipeline.

pub mod directive;
pub mod prepare;
pub mod providers;
pub mod types;

pub use directive::{TtsDirective, parse_tts_directives};
pub use prepare::prepare_for_tts;
pub use types::{TtsAutoMode, TtsConfig, TtsProvider, TtsRequest, TtsResult};
