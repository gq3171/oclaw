use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct Session {
    pub key: String,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
    pub metadata: HashMap<String, String>,
}

impl Session {
    pub fn new(key: &str, agent_id: &str) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            key: key.to_string(),
            agent_id: agent_id.to_string(),
            created_at: now,
            updated_at: now,
            message_count: 0,
            metadata: HashMap::new(),
        }
    }
}

pub type SessionStore = Arc<RwLock<HashMap<String, Session>>>;

pub fn create_session_store() -> SessionStore {
    Arc::new(RwLock::new(HashMap::new()))
}

/// SQLite-backed session persistence.
pub struct SessionDb {
    conn: Connection,
}

impl SessionDb {
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                key TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS session_metadata (
                session_key TEXT NOT NULL,
                k TEXT NOT NULL,
                v TEXT NOT NULL,
                PRIMARY KEY (session_key, k),
                FOREIGN KEY (session_key) REFERENCES sessions(key) ON DELETE CASCADE
            );",
        )?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        Ok(Self { conn })
    }

    pub fn save(&self, session: &Session) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sessions (key, agent_id, created_at, updated_at, message_count)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                session.key,
                session.agent_id,
                session.created_at,
                session.updated_at,
                session.message_count
            ],
        )?;
        self.conn.execute(
            "DELETE FROM session_metadata WHERE session_key = ?1",
            [&session.key],
        )?;
        for (k, v) in &session.metadata {
            self.conn.execute(
                "INSERT INTO session_metadata (session_key, k, v) VALUES (?1, ?2, ?3)",
                rusqlite::params![session.key, k, v],
            )?;
        }
        Ok(())
    }

    pub fn load(&self, key: &str) -> Result<Option<Session>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT agent_id, created_at, updated_at, message_count FROM sessions WHERE key = ?1",
        )?;
        let mut rows = stmt.query([key])?;
        let row = match rows.next()? {
            Some(r) => r,
            None => return Ok(None),
        };
        let mut session = Session {
            key: key.to_string(),
            agent_id: row.get(0)?,
            created_at: row.get(1)?,
            updated_at: row.get(2)?,
            message_count: row.get(3)?,
            metadata: HashMap::new(),
        };
        let mut meta_stmt = self
            .conn
            .prepare("SELECT k, v FROM session_metadata WHERE session_key = ?1")?;
        let meta_rows = meta_stmt.query_map([key], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for pair in meta_rows {
            let (k, v) = pair?;
            session.metadata.insert(k, v);
        }
        Ok(Some(session))
    }

    pub fn list(&self) -> Result<Vec<Session>, rusqlite::Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT key FROM sessions ORDER BY updated_at DESC")?;
        let keys: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        let mut sessions = Vec::new();
        for key in keys {
            if let Some(s) = self.load(&key)? {
                sessions.push(s);
            }
        }
        Ok(sessions)
    }

    pub fn delete(&self, key: &str) -> Result<bool, rusqlite::Error> {
        let count = self
            .conn
            .execute("DELETE FROM sessions WHERE key = ?1", [key])?;
        Ok(count > 0)
    }
}
