//! Media Storage

use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Media reference
#[derive(Debug, Clone)]
pub struct MediaRef {
    pub id: String,
    pub path: PathBuf,
    pub metadata: MediaMetadata,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Media metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub mime_type: String,
    pub size_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_secs: Option<f64>,
    pub format: Option<String>,
    pub checksum: Option<String>,
}

impl MediaMetadata {
    pub fn new(mime_type: &str, size_bytes: u64) -> Self {
        Self {
            mime_type: mime_type.to_string(),
            size_bytes,
            width: None,
            height: None,
            duration_secs: None,
            format: None,
            checksum: None,
        }
    }
    
    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }
    
    pub fn with_duration(mut self, duration: f64) -> Self {
        self.duration_secs = Some(duration);
        self
    }
    
    pub fn with_format(mut self, format: &str) -> Self {
        self.format = Some(format.to_string());
        self
    }
    
    pub fn with_checksum(mut self, checksum: &str) -> Self {
        self.checksum = Some(checksum.to_string());
        self
    }
}

/// Media store for managing media files
pub struct MediaStore {
    _root: PathBuf,
    refs: HashMap<String, MediaRef>,
}

impl MediaStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            _root: root,
            refs: HashMap::new(),
        }
    }
    
    /// Add a media file to the store
    pub fn add(&mut self, id: &str, path: PathBuf, metadata: MediaMetadata) -> MediaRef {
        let reference = MediaRef {
            id: id.to_string(),
            path,
            metadata,
            created_at: chrono::Utc::now(),
        };
        
        self.refs.insert(id.to_string(), reference.clone());
        reference
    }
    
    /// Get a media reference by ID
    pub fn get(&self, id: &str) -> Option<&MediaRef> {
        self.refs.get(id)
    }
    
    /// Remove a media reference
    pub fn remove(&mut self, id: &str) -> Option<MediaRef> {
        self.refs.remove(id)
    }
    
    /// List all media references
    pub fn list(&self) -> Vec<&MediaRef> {
        self.refs.values().collect()
    }
    
    /// Get total size of all media
    pub fn total_size(&self) -> u64 {
        self.refs.values()
            .map(|r| r.metadata.size_bytes)
            .sum()
    }
    
    /// Check if media exists
    pub fn contains(&self, id: &str) -> bool {
        self.refs.contains_key(id)
    }
    
    /// Clear all references
    pub fn clear(&mut self) {
        self.refs.clear();
    }
}
