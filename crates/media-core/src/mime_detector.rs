//! MIME Type Detection

use std::path::Path;
use base64::{Engine as _, engine::general_purpose};

/// MIME type representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeType {
    pub mime: String,
    pub category: MimeCategory,
    pub extensions: Vec<String>,
}

/// MIME category
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeCategory {
    Image,
    Audio,
    Video,
    Application,
    Text,
    Other,
}

/// MIME detector
pub struct MimeDetector;

impl MimeDetector {
    pub fn new() -> Self {
        Self
    }
    
    /// Detect MIME type from bytes
    pub fn detect(&self, data: &[u8]) -> Option<MimeType> {
        // Check magic bytes
        if data.len() < 12 {
            return None;
        }
        
        // PNG: 89 50 4E 47 0D 0A 1A 0A
        if data.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
            return Some(MimeType {
                mime: "image/png".to_string(),
                category: MimeCategory::Image,
                extensions: vec!["png".to_string()],
            });
        }
        
        // JPEG: FF D8 FF
        if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
            return Some(MimeType {
                mime: "image/jpeg".to_string(),
                category: MimeCategory::Image,
                extensions: vec!["jpg".to_string(), "jpeg".to_string()],
            });
        }
        
        // GIF: 47 49 46 38
        if data.starts_with(b"GIF89a") || data.starts_with(b"GIF87a") {
            return Some(MimeType {
                mime: "image/gif".to_string(),
                category: MimeCategory::Image,
                extensions: vec!["gif".to_string()],
            });
        }
        
        // WebP: 52 49 46 46 .... 57 45 42 50
        if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
            return Some(MimeType {
                mime: "image/webp".to_string(),
                category: MimeCategory::Image,
                extensions: vec!["webp".to_string()],
            });
        }
        
        // BMP: 42 4D
        if data.starts_with(b"BM") {
            return Some(MimeType {
                mime: "image/bmp".to_string(),
                category: MimeCategory::Image,
                extensions: vec!["bmp".to_string()],
            });
        }
        
        // TIFF: 49 49 2A 00 or 4D 4D 00 2A
        if data.len() >= 4
            && (data.starts_with(&[0x49, 0x49, 0x2A, 0x00]) ||
               data.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])) {
                return Some(MimeType {
                    mime: "image/tiff".to_string(),
                    category: MimeCategory::Image,
                    extensions: vec!["tiff".to_string(), "tif".to_string()],
                });
        }
        
        // ICO: 00 00 01 00
        if data.len() >= 4 && data[0] == 0x00 && data[1] == 0x00 && 
           data[2] == 0x01 && data[3] == 0x00 {
            return Some(MimeType {
                mime: "image/x-icon".to_string(),
                category: MimeCategory::Image,
                extensions: vec!["ico".to_string()],
            });
        }
        
        // AVIF: ftypavif or ftypeavis
        if data.len() >= 12 && &data[4..8] == b"avif" || &data[4..8] == b"avis" {
            return Some(MimeType {
                mime: "image/avif".to_string(),
                category: MimeCategory::Image,
                extensions: vec!["avif".to_string()],
            });
        }
        
        // WAV: 52 49 46 46 .... 57 41 56 45
        if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WAVE" {
            return Some(MimeType {
                mime: "audio/wav".to_string(),
                category: MimeCategory::Audio,
                extensions: vec!["wav".to_string()],
            });
        }
        
        // MP3: FF FB or FF F3 or FF F2 or ID3
        if data.len() >= 2 {
            if data[0] == 0xFF && (data[1] & 0xE0) == 0xE0 {
                return Some(MimeType {
                    mime: "audio/mpeg".to_string(),
                    category: MimeCategory::Audio,
                    extensions: vec!["mp3".to_string()],
                });
            }
            // ID3 tag
            if data[0] == 0x49 && data[1] == 0x44 && data[2] == 0x33 {
                return Some(MimeType {
                    mime: "audio/mpeg".to_string(),
                    category: MimeCategory::Audio,
                    extensions: vec!["mp3".to_string()],
                });
            }
        }
        
        // OGG: 4F 67 67 53
        if data.starts_with(b"OggS") {
            return Some(MimeType {
                mime: "audio/ogg".to_string(),
                category: MimeCategory::Audio,
                extensions: vec!["ogg".to_string()],
            });
        }
        
        // FLAC: 66 4C 61 43
        if data.starts_with(b"fLaC") {
            return Some(MimeType {
                mime: "audio/flac".to_string(),
                category: MimeCategory::Audio,
                extensions: vec!["flac".to_string()],
            });
        }
        
        // PDF: 25 50 44 46
        if data.starts_with(b"%PDF") {
            return Some(MimeType {
                mime: "application/pdf".to_string(),
                category: MimeCategory::Application,
                extensions: vec!["pdf".to_string()],
            });
        }
        
        // ZIP: 50 4B 03 04
        if data.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
            return Some(MimeType {
                mime: "application/zip".to_string(),
                category: MimeCategory::Application,
                extensions: vec!["zip".to_string()],
            });
        }
        
        // JSON: starts with { or [
        if !data.is_empty() {
            let first_char = data[0] as char;
            if first_char == '{' || first_char == '[' {
                return Some(MimeType {
                    mime: "application/json".to_string(),
                    category: MimeCategory::Application,
                    extensions: vec!["json".to_string()],
                });
            }
        }
        
        // Plain text
        if data.iter().all(|&b| b < 128 || b == b'\n' || b == b'\r' || b == b'\t') {
            return Some(MimeType {
                mime: "text/plain".to_string(),
                category: MimeCategory::Text,
                extensions: vec!["txt".to_string()],
            });
        }
        
        None
    }
    
    /// Detect from file path
    pub fn detect_path(&self, path: &Path) -> Option<MimeType> {
        let extension = path.extension()?.to_str()?;
        
        let mime = match extension.to_lowercase().as_str() {
            // Images
            "png" => ("image/png", MimeCategory::Image),
            "jpg" | "jpeg" => ("image/jpeg", MimeCategory::Image),
            "gif" => ("image/gif", MimeCategory::Image),
            "webp" => ("image/webp", MimeCategory::Image),
            "bmp" => ("image/bmp", MimeCategory::Image),
            "ico" => ("image/x-icon", MimeCategory::Image),
            "svg" => ("image/svg+xml", MimeCategory::Image),
            "tiff" | "tif" => ("image/tiff", MimeCategory::Image),
            "avif" => ("image/avif", MimeCategory::Image),
            
            // Audio
            "wav" => ("audio/wav", MimeCategory::Audio),
            "mp3" => ("audio/mpeg", MimeCategory::Audio),
            "ogg" => ("audio/ogg", MimeCategory::Audio),
            "flac" => ("audio/flac", MimeCategory::Audio),
            "aac" => ("audio/aac", MimeCategory::Audio),
            "m4a" => ("audio/mp4", MimeCategory::Audio),
            
            // Video
            "mp4" => ("video/mp4", MimeCategory::Video),
            "webm" => ("video/webm", MimeCategory::Video),
            "avi" => ("video/x-msvideo", MimeCategory::Video),
            "mkv" => ("video/x-matroska", MimeCategory::Video),
            
            // Application
            "pdf" => ("application/pdf", MimeCategory::Application),
            "zip" => ("application/zip", MimeCategory::Application),
            "json" => ("application/json", MimeCategory::Application),
            "xml" => ("application/xml", MimeCategory::Application),
            "html" | "htm" => ("text/html", MimeCategory::Text),
            "css" => ("text/css", MimeCategory::Text),
            "js" => ("application/javascript", MimeCategory::Application),
            
            // Text
            "txt" | "md" => ("text/plain", MimeCategory::Text),
            
            _ => return None,
        };
        
        Some(MimeType {
            mime: mime.0.to_string(),
            category: mime.1,
            extensions: vec![extension.to_string()],
        })
    }
    
    /// Detect from base64 encoded data
    pub fn detect_base64(&self, data: &str) -> Option<MimeType> {
        // Remove data URL prefix if present
        let data = data.strip_prefix("data:")
            .and_then(|s| s.split(',').next())
            .unwrap_or(data);
        
        // Try to decode and detect
        if let Ok(bytes) = general_purpose::STANDARD.decode(data) {
            self.detect(&bytes)
        } else {
            None
        }
    }
}

impl Default for MimeDetector {
    fn default() -> Self {
        Self::new()
    }
}
