use crate::{GatewayError, GatewayResult};
use oclaws_protocol::frames::{EventFrame, GatewayFrame, RequestFrame, ResponseFrame};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

pub struct MessageHandler;

impl MessageHandler {
    pub fn new_request(method: &str, params: Option<serde_json::Value>) -> RequestFrame {
        RequestFrame {
            frame_type: oclaws_protocol::frames::RequestFrameType::Req,
            id: Uuid::new_v4().to_string(),
            method: method.to_string(),
            params,
        }
    }

    pub fn new_response(
        id: &str,
        ok: bool,
        payload: Option<serde_json::Value>,
        error: Option<oclaws_protocol::frames::ErrorDetails>,
    ) -> ResponseFrame {
        ResponseFrame {
            frame_type: oclaws_protocol::frames::ResponseFrameType::Res,
            id: id.to_string(),
            ok,
            payload,
            error,
        }
    }

    pub fn new_event(event: &str, payload: Option<serde_json::Value>) -> EventFrame {
        EventFrame {
            frame_type: oclaws_protocol::frames::EventFrameType::Event,
            event: event.to_string(),
            payload,
            seq: None,
            state_version: None,
        }
    }

    pub fn parse_frame(data: &[u8]) -> GatewayResult<GatewayFrame> {
        serde_json::from_slice(data).map_err(|e| GatewayError::InvalidFrame(e.to_string()))
    }

    pub fn serialize_frame(frame: &GatewayFrame) -> GatewayResult<Vec<u8>> {
        serde_json::to_vec(frame).map_err(|e| GatewayError::InvalidFrame(e.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub key: String,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub model: String,
    pub system_prompt: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

/// SQLite-backed session manager. Sessions survive restarts.
/// The inner Connection is wrapped in a Mutex so SessionManager is Send + Sync.
pub struct SessionManager {
    db: std::sync::Mutex<rusqlite::Connection>,
}

impl SessionManager {
    pub fn new() -> Self {
        let db = rusqlite::Connection::open_in_memory()
            .expect("Failed to open in-memory SQLite");
        Self::init_db(&db);
        Self { db: std::sync::Mutex::new(db) }
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let db = rusqlite::Connection::open(path).map_err(|e| e.to_string())?;
        Self::init_db(&db);
        Ok(Self { db: std::sync::Mutex::new(db) })
    }

    fn init_db(db: &rusqlite::Connection) {
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);"
        ).expect("Failed to create schema_version table");

        let current: i64 = db.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| r.get(0)
        ).unwrap_or(0);

        let migrations: &[&str] = &[
            // v1: initial sessions table
            "CREATE TABLE IF NOT EXISTS sessions (
                key TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0
            );",
            // v2: agents table + session messages table
            "CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                model TEXT NOT NULL DEFAULT 'default',
                system_prompt TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS session_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_key TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                FOREIGN KEY (session_key) REFERENCES sessions(key) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_session_messages_key ON session_messages(session_key, id);",
        ];

        for (i, sql) in migrations.iter().enumerate() {
            let ver = (i + 1) as i64;
            if ver > current {
                db.execute_batch(sql).unwrap_or_else(|e| panic!("Migration v{} failed: {}", ver, e));
                db.execute("INSERT INTO schema_version (version) VALUES (?1)", rusqlite::params![ver]).ok();
            }
        }
    }

    pub fn create_session(&self, key: &str, agent_id: &str) -> Result<SessionInfo, String> {
        let now = chrono::Utc::now().timestamp_millis();
        {
            let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;

            // Use INSERT OR IGNORE to avoid overwriting existing sessions,
            // then UPDATE to refresh the agent_id and updated_at timestamp.
            db.execute(
                "INSERT OR IGNORE INTO sessions (key, agent_id, created_at, updated_at, message_count) VALUES (?1, ?2, ?3, ?4, 0)",
                rusqlite::params![key, agent_id, now, now],
            ).map_err(|e| format!("Failed to create session: {}", e))?;

            db.execute(
                "UPDATE sessions SET agent_id = ?1, updated_at = ?2 WHERE key = ?3",
                rusqlite::params![agent_id, now, key],
            ).map_err(|e| format!("Failed to update session: {}", e))?;
        } // drop lock before calling get_session

        // Return the actual session state (preserving message_count)
        self.get_session(key)?
            .ok_or_else(|| "Session created but not found".to_string())
    }

    pub fn get_session(&self, key: &str) -> Result<Option<SessionInfo>, String> {
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        Ok(db.query_row(
            "SELECT key, agent_id, created_at, updated_at, message_count FROM sessions WHERE key = ?1",
            rusqlite::params![key],
            |row| Ok(SessionInfo {
                key: row.get(0)?,
                agent_id: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                message_count: row.get(4)?,
            }),
        ).ok())
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionInfo>, String> {
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT key, agent_id, created_at, updated_at, message_count FROM sessions ORDER BY updated_at DESC"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| Ok(SessionInfo {
            key: row.get(0)?,
            agent_id: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            message_count: row.get(4)?,
        })).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn remove_session(&self, key: &str) -> Result<Option<SessionInfo>, String> {
        let session = self.get_session(key)?;
        if session.is_some() {
            let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
            db.execute("DELETE FROM sessions WHERE key = ?1", rusqlite::params![key]).ok();
        }
        Ok(session)
    }

    pub fn update_message_count(&self, key: &str, count: i64) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp_millis();
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        db.execute(
            "UPDATE sessions SET message_count = ?1, updated_at = ?2 WHERE key = ?3",
            rusqlite::params![count, now, key],
        ).ok();
        Ok(())
    }

    pub fn update_agent_id(&self, key: &str, agent_id: &str) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp_millis();
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        db.execute(
            "UPDATE sessions SET agent_id = ?1, updated_at = ?2 WHERE key = ?3",
            rusqlite::params![agent_id, now, key],
        ).map_err(|e| format!("Failed to update agent_id: {}", e))?;
        Ok(())
    }

    pub fn touch_session(&self, key: &str) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp_millis();
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        db.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE key = ?2",
            rusqlite::params![now, key],
        ).map_err(|e| format!("Failed to touch session: {}", e))?;
        Ok(())
    }

    // ── Agent CRUD ──────────────────────────────────────────────────

    pub fn create_agent(&self, id: &str, name: &str, model: &str, system_prompt: &str) -> Result<AgentInfo, String> {
        let now = chrono::Utc::now().timestamp_millis();
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        db.execute(
            "INSERT INTO agents (id, name, model, system_prompt, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, name, model, system_prompt, now, now],
        ).map_err(|e| format!("Failed to create agent: {}", e))?;
        Ok(AgentInfo { id: id.to_string(), name: name.to_string(), model: model.to_string(), system_prompt: system_prompt.to_string(), created_at: now, updated_at: now })
    }

    pub fn list_agents(&self) -> Result<Vec<AgentInfo>, String> {
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT id, name, model, system_prompt, created_at, updated_at FROM agents ORDER BY created_at"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| Ok(AgentInfo {
            id: row.get(0)?, name: row.get(1)?, model: row.get(2)?,
            system_prompt: row.get(3)?, created_at: row.get(4)?, updated_at: row.get(5)?,
        })).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn update_agent(&self, id: &str, patch: &serde_json::Value) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp_millis();
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        if let Some(name) = patch["name"].as_str() {
            db.execute("UPDATE agents SET name = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![name, now, id]).map_err(|e| e.to_string())?;
        }
        if let Some(model) = patch["model"].as_str() {
            db.execute("UPDATE agents SET model = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![model, now, id]).map_err(|e| e.to_string())?;
        }
        if let Some(sp) = patch["systemPrompt"].as_str() {
            db.execute("UPDATE agents SET system_prompt = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![sp, now, id]).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn delete_agent(&self, id: &str) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        db.execute("DELETE FROM agents WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("Failed to delete agent: {}", e))?;
        Ok(())
    }

    // ── Session Messages ────────────────────────────────────────────

    pub fn add_message(&self, session_key: &str, role: &str, content: &str) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp_millis();
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        db.execute(
            "INSERT INTO session_messages (session_key, role, content, timestamp) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![session_key, role, content, now],
        ).map_err(|e| format!("Failed to add message: {}", e))?;
        db.execute(
            "UPDATE sessions SET message_count = (SELECT COUNT(*) FROM session_messages WHERE session_key = ?1), updated_at = ?2 WHERE key = ?1",
            rusqlite::params![session_key, now],
        ).map_err(|e| format!("Failed to update message count: {}", e))?;
        Ok(())
    }

    pub fn get_messages(&self, session_key: &str, limit: usize) -> Result<Vec<SessionMessage>, String> {
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT role, content, timestamp FROM session_messages WHERE session_key = ?1 ORDER BY id DESC LIMIT ?2"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![session_key, limit as i64], |row| {
            Ok(SessionMessage { role: row.get(0)?, content: row.get(1)?, timestamp: row.get(2)? })
        }).map_err(|e| e.to_string())?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn clear_messages(&self, session_key: &str) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp_millis();
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        db.execute("DELETE FROM session_messages WHERE session_key = ?1", rusqlite::params![session_key])
            .map_err(|e| format!("Failed to clear messages: {}", e))?;
        db.execute(
            "UPDATE sessions SET message_count = 0, updated_at = ?1 WHERE key = ?2",
            rusqlite::params![now, session_key],
        ).map_err(|e| format!("Failed to update session: {}", e))?;
        Ok(())
    }

    pub fn compact_messages(&self, session_key: &str, max_messages: usize) -> Result<(i64, i64), String> {
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        let total: i64 = db.query_row(
            "SELECT COUNT(*) FROM session_messages WHERE session_key = ?1",
            rusqlite::params![session_key], |r| r.get(0),
        ).map_err(|e| e.to_string())?;
        if total as usize > max_messages {
            let to_delete = total - max_messages as i64;
            db.execute(
                "DELETE FROM session_messages WHERE id IN (SELECT id FROM session_messages WHERE session_key = ?1 ORDER BY id ASC LIMIT ?2)",
                rusqlite::params![session_key, to_delete],
            ).map_err(|e| e.to_string())?;
            let now = chrono::Utc::now().timestamp_millis();
            let new_count = total - to_delete;
            db.execute(
                "UPDATE sessions SET message_count = ?1, updated_at = ?2 WHERE key = ?3",
                rusqlite::params![new_count, now, session_key],
            ).map_err(|e| e.to_string())?;
            Ok((total, new_count))
        } else {
            Ok((total, total))
        }
    }

    /// Remove sessions not updated within `max_age_ms` milliseconds.
    pub fn cleanup_stale(&self, max_age_ms: i64) -> Result<usize, String> {
        let cutoff = chrono::Utc::now().timestamp_millis() - max_age_ms;
        let db = self.db.lock().map_err(|e| format!("DB lock poisoned: {}", e))?;
        Ok(db.execute("DELETE FROM sessions WHERE updated_at < ?1", rusqlite::params![cutoff])
            .unwrap_or(0))
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_create_and_get() {
        let mgr = SessionManager::new();
        let s = mgr.create_session("k1", "agent1").unwrap();
        assert_eq!(s.key, "k1");
        assert_eq!(s.agent_id, "agent1");
        assert_eq!(s.message_count, 0);

        let got = mgr.get_session("k1").unwrap().unwrap();
        assert_eq!(got.key, "k1");
    }

    #[test]
    fn test_session_get_missing() {
        let mgr = SessionManager::new();
        assert!(mgr.get_session("nope").unwrap().is_none());
    }

    #[test]
    fn test_session_list() {
        let mgr = SessionManager::new();
        mgr.create_session("a", "ag").unwrap();
        mgr.create_session("b", "ag").unwrap();
        let list = mgr.list_sessions().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_session_remove() {
        let mgr = SessionManager::new();
        mgr.create_session("r1", "ag").unwrap();
        let removed = mgr.remove_session("r1").unwrap();
        assert!(removed.is_some());
        assert!(mgr.get_session("r1").unwrap().is_none());
    }

    #[test]
    fn test_session_remove_missing() {
        let mgr = SessionManager::new();
        assert!(mgr.remove_session("nope").unwrap().is_none());
    }

    #[test]
    fn test_session_update_message_count() {
        let mgr = SessionManager::new();
        mgr.create_session("mc", "ag").unwrap();
        mgr.update_message_count("mc", 42).unwrap();
        let s = mgr.get_session("mc").unwrap().unwrap();
        assert_eq!(s.message_count, 42);
    }

    #[test]
    fn test_session_cleanup_stale() {
        let mgr = SessionManager::new();
        mgr.create_session("old", "ag").unwrap();
        // Sleep so the session's updated_at is strictly before cutoff
        std::thread::sleep(std::time::Duration::from_millis(2));
        let removed = mgr.cleanup_stale(0).unwrap();
        assert_eq!(removed, 1);
        assert!(mgr.get_session("old").unwrap().is_none());
    }

    #[test]
    fn test_session_cleanup_keeps_fresh() {
        let mgr = SessionManager::new();
        mgr.create_session("fresh", "ag").unwrap();
        // 1 hour max age — session just created should survive
        let removed = mgr.cleanup_stale(3_600_000).unwrap();
        assert_eq!(removed, 0);
        assert!(mgr.get_session("fresh").unwrap().is_some());
    }
}
