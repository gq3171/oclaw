use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::error::{StorageError, StorageResult};
use crate::models::{QueryFilter, Record, RecordKind};

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub async fn new(path: Option<PathBuf>) -> StorageResult<Self> {
        let db_path = path.unwrap_or_else(|| {
            let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
            path.push("oclaws");
            std::fs::create_dir_all(&path).ok();
            path.push("data.db");
            path
        });

        let conn = Connection::open(&db_path)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS records (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                metadata TEXT NOT NULL
            )",
            [],
        ).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_kind ON records(kind)",
            [],
        ).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_key ON records(key)",
            [],
        ).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn insert(&self, record: Record) -> StorageResult<()> {
        let conn = self.conn.lock().await;
        
        conn.execute(
            "INSERT INTO records (id, kind, key, value, created_at, updated_at, metadata) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.id,
                record.kind.as_str(),
                record.key,
                serde_json::to_string(&record.value)?,
                record.created_at.to_rfc3339(),
                record.updated_at.to_rfc3339(),
                serde_json::to_string(&record.metadata)?,
            ],
        ).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn update(&self, record: Record) -> StorageResult<()> {
        let conn = self.conn.lock().await;
        
        let rows = conn.execute(
            "UPDATE records SET key = ?1, value = ?2, updated_at = ?3, metadata = ?4 WHERE id = ?5",
            params![
                record.key,
                serde_json::to_string(&record.value)?,
                record.updated_at.to_rfc3339(),
                serde_json::to_string(&record.metadata)?,
                record.id,
            ],
        ).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        if rows == 0 {
            return Err(StorageError::NotFound(record.id));
        }

        Ok(())
    }

    pub async fn delete(&self, id: &str) -> StorageResult<()> {
        let conn = self.conn.lock().await;
        
        let rows = conn.execute(
            "DELETE FROM records WHERE id = ?1",
            params![id],
        ).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        if rows == 0 {
            return Err(StorageError::NotFound(id.to_string()));
        }

        Ok(())
    }

    pub async fn get(&self, id: &str) -> StorageResult<Record> {
        let conn = self.conn.lock().await;
        
        let mut stmt = conn.prepare(
            "SELECT id, kind, key, value, created_at, updated_at, metadata FROM records WHERE id = ?1"
        ).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        let record = stmt.query_row(params![id], |row| {
            Ok(Record {
                id: row.get(0)?,
                kind: RecordKind::parse(&row.get::<_, String>(1)?),
                key: row.get(2)?,
                value: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or(serde_json::Value::Null),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                metadata: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or(serde_json::Value::Null),
            })
        }).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StorageError::NotFound(id.to_string()),
            _ => StorageError::DatabaseError(e.to_string()),
        })?;

        Ok(record)
    }

    pub async fn get_by_key(&self, kind: &RecordKind, key: &str) -> StorageResult<Option<Record>> {
        let conn = self.conn.lock().await;
        
        let mut stmt = conn.prepare(
            "SELECT id, kind, key, value, created_at, updated_at, metadata FROM records WHERE kind = ?1 AND key = ?2"
        ).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        let result = stmt.query_row(params![kind.as_str(), key], |row| {
            Ok(Record {
                id: row.get(0)?,
                kind: RecordKind::parse(&row.get::<_, String>(1)?),
                key: row.get(2)?,
                value: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or(serde_json::Value::Null),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                metadata: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or(serde_json::Value::Null),
            })
        });

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::DatabaseError(e.to_string())),
        }
    }

    pub async fn query(&self, filter: QueryFilter) -> StorageResult<Vec<Record>> {
        let conn = self.conn.lock().await;
        
        let mut sql = String::from("SELECT id, kind, key, value, created_at, updated_at, metadata FROM records WHERE 1=1");
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(kind) = &filter.kind {
            sql.push_str(" AND kind = ?");
            params_vec.push(Box::new(kind.as_str().to_string()));
        }

        if let Some(prefix) = &filter.key_prefix {
            sql.push_str(" AND key LIKE ?");
            params_vec.push(Box::new(format!("{}%", prefix)));
        }

        sql.push_str(" ORDER BY created_at DESC");

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }

        if let Some(offset) = filter.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        let mut stmt = conn.prepare(&sql).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

        let records = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(Record {
                id: row.get(0)?,
                kind: RecordKind::parse(&row.get::<_, String>(1)?),
                key: row.get(2)?,
                value: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or(serde_json::Value::Null),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                metadata: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or(serde_json::Value::Null),
            })
        }).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        let mut result = Vec::new();
        for record in records {
            result.push(record.map_err(|e| StorageError::DatabaseError(e.to_string()))?);
        }

        Ok(result)
    }

    pub async fn count(&self, kind: Option<&RecordKind>) -> StorageResult<usize> {
        let conn = self.conn.lock().await;
        
        let sql = if kind.is_some() {
            "SELECT COUNT(*) FROM records WHERE kind = ?1"
        } else {
            "SELECT COUNT(*) FROM records"
        };

        let mut stmt = conn.prepare(sql).map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        let count: usize = if let Some(k) = kind {
            stmt.query_row(params![k.as_str()], |row| row.get(0))
        } else {
            stmt.query_row([], |row| row.get(0))
        }.map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        Ok(count)
    }

    pub async fn clear(&self, kind: Option<&RecordKind>) -> StorageResult<usize> {
        let conn = self.conn.lock().await;
        
        let sql = if kind.is_some() {
            "DELETE FROM records WHERE kind = ?1"
        } else {
            "DELETE FROM records"
        };

        let rows = if let Some(k) = kind {
            conn.execute(sql, params![k.as_str()])
        } else {
            conn.execute(sql, [])
        }.map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        Ok(rows)
    }
}
