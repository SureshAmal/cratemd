use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DB_FILENAME: &str = ".cratemd.db";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaEntry {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNote {
    pub key: String,
    pub category: String,
    pub content: String,
    pub updated_at: String,
}

pub struct VizDb {
    conn: Connection,
    pub db_path: PathBuf,
}

impl VizDb {
    pub fn open(path: &Path) -> Result<Self> {
        let db_path = if path.is_file() {
            path.to_path_buf()
        } else {
            path.join(DB_FILENAME)
        };

        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

        let _ = conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;");

        Ok(Self { conn, db_path })
    }

    pub fn load_meta(&self) -> Result<Vec<MetaEntry>> {
        let mut stmt = self.conn.prepare("SELECT key, value, updated_at FROM project_meta ORDER BY key")?;
        let rows = stmt.query_map([], |row| {
            Ok(MetaEntry {
                key: row.get(0)?,
                value: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })?;

        let mut entries = Vec::new();
        for r in rows {
            entries.push(r?);
        }
        Ok(entries)
    }

    pub fn load_memories(&self) -> Result<Vec<MemoryNote>> {
        let mut stmt = self.conn.prepare("SELECT key, category, content, updated_at FROM memory_notes ORDER BY category, key")?;
        let rows = stmt.query_map([], |row| {
            Ok(MemoryNote {
                key: row.get(0)?,
                category: row.get(1)?,
                content: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?;

        let mut entries = Vec::new();
        for r in rows {
            entries.push(r?);
        }
        Ok(entries)
    }

    pub fn search_memories(&self, query: &str) -> Result<Vec<MemoryNote>> {
        if query.trim().is_empty() {
            return self.load_memories();
        }

        let clean_query = query.replace('"', "\"\"");
        let fts_query = format!("\"{}\"*", clean_query);

        let stmt = self.conn.prepare(
            r#"
            SELECT n.key, n.category, n.content, n.updated_at
            FROM memory_notes n
            JOIN context_fts f ON n.key = f.key
            WHERE context_fts MATCH ?1
            ORDER BY rank
            "#,
        );

        match stmt {
            Ok(mut s) => {
                let rows = s.query_map([&fts_query], |row| {
                    Ok(MemoryNote {
                        key: row.get(0)?,
                        category: row.get(1)?,
                        content: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                });

                if let Ok(r_iter) = rows {
                    let mut entries = Vec::new();
                    for r in r_iter {
                        if let Ok(entry) = r {
                            entries.push(entry);
                        }
                    }
                    if !entries.is_empty() {
                        return Ok(entries);
                    }
                }
            }
            Err(_) => {}
        }

        // Fallback to LIKE search if FTS syntax fails or returns empty
        let like_query = format!("%{}%", query.trim());
        let mut fallback = self.conn.prepare(
            "SELECT key, category, content, updated_at FROM memory_notes WHERE key LIKE ?1 OR content LIKE ?1 OR category LIKE ?1 ORDER BY category, key"
        )?;
        let rows = fallback.query_map([&like_query], |row| {
            Ok(MemoryNote {
                key: row.get(0)?,
                category: row.get(1)?,
                content: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })?;

        let mut entries = Vec::new();
        for r in rows {
            entries.push(r?);
        }
        Ok(entries)
    }
}
