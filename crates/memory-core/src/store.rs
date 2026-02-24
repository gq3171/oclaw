use crate::types::{MemoryChunk, MemorySearchResult};
use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use tracing::{debug, info};

pub struct MemoryStore {
    path: PathBuf,
}

impl MemoryStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".oclaws")
            .join("memory.db")
    }

    fn open(&self) -> Result<Connection> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&self.path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Ok(conn)
    }

    pub fn init(&self) -> Result<()> {
        let conn = self.open()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL,
                source TEXT NOT NULL,
                embedding BLOB,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chunks_path ON chunks(path);
            CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(source);
            CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                id UNINDEXED,
                content,
                path,
                source,
                content=chunks,
                content_rowid=rowid
            );
            CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
                INSERT INTO chunks_fts(rowid, id, content, path, source)
                VALUES (new.rowid, new.id, new.content, new.path, new.source);
            END;
            CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid, id, content, path, source)
                VALUES ('delete', old.rowid, old.id, old.content, old.path, old.source);
            END;
            CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid, id, content, path, source)
                VALUES ('delete', old.rowid, old.id, old.content, old.path, old.source);
                INSERT INTO chunks_fts(rowid, id, content, path, source)
                VALUES (new.rowid, new.id, new.content, new.path, new.source);
            END;",
        )?;
        info!("Memory store initialized at {:?}", self.path);
        Ok(())
    }

    pub fn upsert(&self, chunk: &MemoryChunk) -> Result<()> {
        let conn = self.open()?;
        let embedding_blob = chunk.embedding.as_ref().map(|v| {
            v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>()
        });
        conn.execute(
            "INSERT INTO chunks (id, path, start_line, end_line, content, source, embedding, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                path=excluded.path, start_line=excluded.start_line, end_line=excluded.end_line,
                content=excluded.content, source=excluded.source, embedding=excluded.embedding,
                updated_at=excluded.updated_at",
            params![
                chunk.id,
                chunk.path,
                chunk.start_line as i64,
                chunk.end_line as i64,
                chunk.content,
                chunk.source,
                embedding_blob,
                chunk.updated_at_ms as i64,
            ],
        )?;
        debug!("Upserted chunk {}", chunk.id);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<MemoryChunk>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT id, path, start_line, end_line, content, source, embedding, updated_at
             FROM chunks WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_chunk(row)?)),
            None => Ok(None),
        }
    }

    pub fn delete_by_path(&self, path: &str) -> Result<usize> {
        let conn = self.open()?;
        let count = conn.execute("DELETE FROM chunks WHERE path = ?1", params![path])?;
        debug!("Deleted {} chunks for path {}", count, path);
        Ok(count)
    }

    pub fn fts_search(&self, query: &str, limit: usize) -> Result<Vec<MemorySearchResult>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT c.id, c.path, c.start_line, c.end_line, c.content, c.source,
                    rank
             FROM chunks_fts f
             JOIN chunks c ON c.id = f.id
             WHERE chunks_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| {
            let rank: f64 = row.get(6)?;
            Ok(MemorySearchResult {
                path: row.get(1)?,
                start_line: row.get::<_, i64>(2)? as usize,
                end_line: row.get::<_, i64>(3)? as usize,
                score: -rank, // FTS5 rank is negative (lower = better)
                snippet: row.get(4)?,
                source: row.get(5)?,
            })
        })?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn fts_search_ids(&self, query: &str, limit: usize) -> Result<Vec<(String, f64)>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT c.id, rank
             FROM chunks_fts f
             JOIN chunks c ON c.id = f.id
             WHERE chunks_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], |row| {
            let id: String = row.get(0)?;
            let rank: f64 = row.get(1)?;
            Ok((id, -rank))
        })?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn all_with_embeddings(&self) -> Result<Vec<(String, Vec<f32>)>> {
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?;
        let mut results = Vec::new();
        for r in rows {
            let (id, blob) = r?;
            let embedding = blob_to_embedding(&blob);
            results.push((id, embedding));
        }
        Ok(results)
    }

    pub fn get_by_ids(&self, ids: &[String]) -> Result<Vec<MemoryChunk>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT id, path, start_line, end_line, content, source, embedding, updated_at
             FROM chunks WHERE id IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok(row_to_chunk_rusqlite(row))
        })?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r??);
        }
        Ok(results)
    }

    pub fn count(&self) -> Result<usize> {
        let conn = self.open()?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

fn row_to_chunk(row: &rusqlite::Row) -> Result<MemoryChunk> {
    let embedding_blob: Option<Vec<u8>> = row.get(6)?;
    let embedding = embedding_blob.map(|b| blob_to_embedding(&b));
    Ok(MemoryChunk {
        id: row.get(0)?,
        path: row.get(1)?,
        start_line: row.get::<_, i64>(2)? as usize,
        end_line: row.get::<_, i64>(3)? as usize,
        content: row.get(4)?,
        source: row.get(5)?,
        embedding,
        updated_at_ms: row.get::<_, i64>(7)? as u64,
    })
}

fn row_to_chunk_rusqlite(row: &rusqlite::Row) -> Result<MemoryChunk> {
    row_to_chunk(row)
}

fn blob_to_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}
