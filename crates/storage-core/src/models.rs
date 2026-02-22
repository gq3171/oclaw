use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RecordKind {
    Message,
    Session,
    Conversation,
    ToolCall,
    User,
    Custom(String),
}

impl RecordKind {
    pub fn as_str(&self) -> &str {
        match self {
            RecordKind::Message => "message",
            RecordKind::Session => "session",
            RecordKind::Conversation => "conversation",
            RecordKind::ToolCall => "toolcall",
            RecordKind::User => "user",
            RecordKind::Custom(s) => s,
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "message" => RecordKind::Message,
            "session" => RecordKind::Session,
            "conversation" => RecordKind::Conversation,
            "toolcall" => RecordKind::ToolCall,
            "user" => RecordKind::User,
            other => RecordKind::Custom(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub kind: RecordKind,
    pub key: String,
    pub value: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

impl Record {
    pub fn new(kind: RecordKind, key: String, value: serde_json::Value) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            kind,
            key,
            value,
            created_at: now,
            updated_at: now,
            metadata: serde_json::json!({}),
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryFilter {
    pub kind: Option<RecordKind>,
    pub key_prefix: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self {
            kind: None,
            key_prefix: None,
            limit: Some(100),
            offset: Some(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_creation() {
        let record = Record::new(
            RecordKind::Message,
            "test_key".to_string(),
            serde_json::json!({"content": "hello"}),
        );

        assert_eq!(record.kind, RecordKind::Message);
        assert_eq!(record.key, "test_key");
        assert_eq!(record.value["content"], "hello");
        assert!(!record.id.is_empty());
    }

    #[test]
    fn test_record_with_metadata() {
        let record = Record::new(
            RecordKind::Session,
            "session_1".to_string(),
            serde_json::json!({"user": "test"}),
        )
        .with_metadata(serde_json::json!({"source": "api"}));

        assert_eq!(record.metadata["source"], "api");
    }

    #[test]
    fn test_record_kind_serialization() {
        let kinds = vec![
            RecordKind::Message,
            RecordKind::Session,
            RecordKind::Conversation,
            RecordKind::ToolCall,
            RecordKind::User,
        ];

        for kind in kinds {
            let serialized = kind.as_str();
            let deserialized = RecordKind::parse(serialized);
            assert_eq!(deserialized.as_str(), serialized);
        }
    }

    #[test]
    fn test_record_kind_custom() {
        let custom = RecordKind::Custom("my_type".to_string());
        assert_eq!(custom.as_str(), "my_type");
    }
}
