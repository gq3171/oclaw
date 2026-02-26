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
            .join(".oclaw")
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

        // Create main chunks table
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
            CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(source);",
        )?;

        // Migrate FTS table to trigram tokenizer for CJK support.
        // The trigram tokenizer enables substring matching which is essential
        // for Chinese/Japanese/Korean text where words aren't space-separated.
        self.ensure_trigram_fts(&conn)?;

        info!("Memory store initialized at {:?}", self.path);
        Ok(())
    }

    /// Ensure the FTS5 table uses the trigram tokenizer.
    /// If an old FTS table exists (without trigram), drop and recreate it.
    fn ensure_trigram_fts(&self, conn: &Connection) -> Result<()> {
        // Check if FTS table exists and what tokenizer it uses
        let needs_recreate = match conn.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='chunks_fts'",
            [],
            |row| row.get::<_, String>(0),
        ) {
            Ok(sql) => !sql.contains("trigram"),
            Err(_) => true, // table doesn't exist
        };

        if needs_recreate {
            debug!("Creating/recreating FTS5 table with trigram tokenizer");
            // Drop old FTS table and triggers if they exist
            conn.execute_batch(
                "DROP TRIGGER IF EXISTS chunks_ai;
                 DROP TRIGGER IF EXISTS chunks_ad;
                 DROP TRIGGER IF EXISTS chunks_au;
                 DROP TABLE IF EXISTS chunks_fts;",
            )?;

            // Create FTS5 with trigram tokenizer for CJK substring matching
            conn.execute_batch(
                "CREATE VIRTUAL TABLE chunks_fts USING fts5(
                    id UNINDEXED,
                    content,
                    path,
                    source,
                    content=chunks,
                    content_rowid=rowid,
                    tokenize='trigram'
                );",
            )?;

            // Recreate sync triggers
            conn.execute_batch(
                "CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
                    INSERT INTO chunks_fts(rowid, id, content, path, source)
                    VALUES (new.rowid, new.id, new.content, new.path, new.source);
                END;
                CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
                    INSERT INTO chunks_fts(chunks_fts, rowid, id, content, path, source)
                    VALUES ('delete', old.rowid, old.id, old.content, old.path, old.source);
                END;
                CREATE TRIGGER chunks_au AFTER UPDATE ON chunks BEGIN
                    INSERT INTO chunks_fts(chunks_fts, rowid, id, content, path, source)
                    VALUES ('delete', old.rowid, old.id, old.content, old.path, old.source);
                    INSERT INTO chunks_fts(rowid, id, content, path, source)
                    VALUES (new.rowid, new.id, new.content, new.path, new.source);
                END;",
            )?;

            // Re-populate FTS from existing chunks
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM chunks", [], |row| row.get(0),
            )?;
            if count > 0 {
                conn.execute_batch(
                    "INSERT INTO chunks_fts(rowid, id, content, path, source)
                     SELECT rowid, id, content, path, source FROM chunks;",
                )?;
                info!("Re-indexed {} chunks into trigram FTS", count);
            }
        }

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

        // Extract keywords for smarter searching
        let keywords = crate::query_expand::extract_keywords(query);

        // Strategy 1: FTS trigram MATCH for keywords >= 3 chars
        let long_keywords: Vec<&str> = keywords.iter()
            .filter(|k| k.chars().count() >= 3)
            .map(|k| k.as_str())
            .collect();

        if !long_keywords.is_empty()
            && let Some(fts_q) = crate::query_expand::build_fts5_query(&long_keywords.join(" "))
        {
            let results = self.fts_match(&conn, &fts_q, limit)?;
            if !results.is_empty() {
                return Ok(results);
            }
        }

        // Strategy 2: LIKE search with extracted keywords (works for any length)
        if !keywords.is_empty() {
            let results = self.keyword_like_search(&conn, &keywords, limit)?;
            if !results.is_empty() {
                return Ok(results);
            }
        }

        // Strategy 3: Plain LIKE fallback with original query
        debug!("Keyword search returned no results, falling back to plain LIKE");
        self.like_search(&conn, query, limit)
    }

    fn fts_match(&self, conn: &Connection, query: &str, limit: usize) -> Result<Vec<MemorySearchResult>> {
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
                id: row.get::<_, i64>(0)?.to_string(),
                path: row.get(1)?,
                start_line: row.get::<_, i64>(2)? as usize,
                end_line: row.get::<_, i64>(3)? as usize,
                score: -rank,
                snippet: row.get(4)?,
                source: row.get(5)?,
                updated_at_ms: 0,
            })
        })?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// LIKE-based search using extracted keywords. Matches any keyword (OR logic),
    /// scores by number of keywords matched. This handles short CJK words (< 3 chars)
    /// that the FTS5 trigram tokenizer cannot match.
    fn keyword_like_search(&self, conn: &Connection, keywords: &[String], limit: usize) -> Result<Vec<MemorySearchResult>> {
        // Build WHERE clause: content LIKE '%kw1%' OR content LIKE '%kw2%' ...
        let conditions: Vec<String> = (1..=keywords.len())
            .map(|i| format!("content LIKE ?{}", i))
            .collect();
        let where_clause = conditions.join(" OR ");

        // Build a score expression: count how many keywords match
        let score_parts: Vec<String> = (1..=keywords.len())
            .map(|i| format!("(CASE WHEN content LIKE ?{} THEN 1 ELSE 0 END)", i))
            .collect();
        let score_expr = score_parts.join(" + ");

        let sql = format!(
            "SELECT id, path, start_line, end_line, content, source, ({}) AS match_score
             FROM chunks
             WHERE {}
             ORDER BY match_score DESC, updated_at DESC
             LIMIT ?{}",
            score_expr, where_clause, keywords.len() + 1
        );

        let mut stmt = conn.prepare(&sql)?;

        // Build params: each keyword as "%keyword%"
        let patterns: Vec<String> = keywords.iter().map(|k| format!("%{}%", k)).collect();
        let limit_i64 = limit as i64;
        let mut param_values: Vec<&dyn rusqlite::ToSql> = patterns.iter()
            .map(|p| p as &dyn rusqlite::ToSql)
            .collect();
        param_values.push(&limit_i64);

        let rows = stmt.query_map(param_values.as_slice(), |row| {
            let match_score: i64 = row.get(6)?;
            Ok(MemorySearchResult {
                id: row.get::<_, i64>(0)?.to_string(),
                path: row.get(1)?,
                start_line: row.get::<_, i64>(2)? as usize,
                end_line: row.get::<_, i64>(3)? as usize,
                score: match_score as f64,
                snippet: row.get(4)?,
                source: row.get(5)?,
                updated_at_ms: 0,
            })
        })?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    fn like_search(&self, conn: &Connection, query: &str, limit: usize) -> Result<Vec<MemorySearchResult>> {
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT id, path, start_line, end_line, content, source
             FROM chunks
             WHERE content LIKE ?1
             ORDER BY updated_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], |row| {
            Ok(MemorySearchResult {
                id: row.get::<_, i64>(0)?.to_string(),
                path: row.get(1)?,
                start_line: row.get::<_, i64>(2)? as usize,
                end_line: row.get::<_, i64>(3)? as usize,
                score: 1.0, // fixed score for LIKE results
                snippet: row.get(4)?,
                source: row.get(5)?,
                updated_at_ms: 0,
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

        // Sanitize query through build_fts5_query to avoid FTS5 syntax errors
        let safe_query = match crate::query_expand::build_fts5_query(query) {
            Some(q) => q,
            None => return Ok(Vec::new()),
        };

        let mut stmt = conn.prepare(
            "SELECT c.id, rank
             FROM chunks_fts f
             JOIN chunks c ON c.id = f.id
             WHERE chunks_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![safe_query, limit as i64], |row| {
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
