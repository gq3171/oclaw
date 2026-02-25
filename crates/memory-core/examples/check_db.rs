use rusqlite::Connection;
use std::path::PathBuf;

fn main() {
    let path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oclaws")
        .join("memory.db");

    println!("DB path: {:?}", path);
    let conn = Connection::open(&path).unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap();
    println!("Total chunks: {}", count);

    let mut stmt = conn
        .prepare(
            "SELECT id, substr(content,1,120), source, updated_at FROM chunks ORDER BY updated_at DESC LIMIT 10",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .unwrap();
    for r in rows {
        let (id, content, source, ts) = r.unwrap();
        println!("---\nID: {}\nContent: {}\nSource: {}\nTS: {}", id, content, source, ts);
    }

    // Check FTS
    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks_fts", [], |r| r.get(0))
        .unwrap_or(-1);
    println!("\nFTS entries: {}", fts_count);

    // Test trigram search vs LIKE search
    for q in &["小溪", "回来", "API", "记忆"] {
        let fts_q = format!("\"{}\"", q);
        let fts_hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
                [&fts_q],
                |r| r.get(0),
            )
            .unwrap_or(-1);

        let like_pattern = format!("%{}%", q);
        let like_hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE content LIKE ?1",
                [&like_pattern],
                |r| r.get(0),
            )
            .unwrap_or(-1);

        println!(
            "Search '{}': FTS={} hits, LIKE={} hits {}",
            q, fts_hits, like_hits,
            if fts_hits == 0 && like_hits > 0 { "<-- LIKE fallback needed" } else { "" }
        );
    }
}
