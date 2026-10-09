use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::workspace::WorkspaceInfo;
use crate::locator::CrateLocator;
use crate::docgen::DocGenerator;
use crate::analyzer::CrateAnalyzer;

pub const DB_FILENAME: &str = ".cratemd.db";

/// Entry in the project context memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub key: String,
    pub category: String,
    pub content: String,
    pub updated_at: String,
}

pub struct ProjectDb {
    conn: Connection,
    pub db_path: PathBuf,
}

impl ProjectDb {
    /// Opens or creates the SQLite project database in the root of the project/workspace
    pub fn open(start_dir: Option<&Path>) -> Result<Self> {
        let base_dir = match start_dir {
            Some(d) => d.to_path_buf(),
            None => std::env::current_dir()?,
        };

        let root_dir = WorkspaceInfo::find_root(&base_dir).unwrap_or(base_dir);
        let db_path = root_dir.join(DB_FILENAME);

        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open database at {}", db_path.display()))?;

        // Initialize WAL mode for concurrency and performance
        let _ = conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;");

        let db = Self { conn, db_path };
        db.init_schema()?;
        Ok(db)
    }

    /// Creates tables for metadata, indexed files/symbols, agent memory notes, and full text search
    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS project_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS memory_notes (
                key TEXT PRIMARY KEY,
                category TEXT NOT NULL,
                content TEXT NOT NULL,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS context_fts USING fts5(
                key,
                category,
                content,
                tokenize = 'unicode61'
            );
            "#,
        )?;
        Ok(())
    }

    /// Initializes project context by scanning the workspace, indexing members, and storing blueprint
    pub fn init_project(&self, root_dir: &Path) -> Result<String> {
        let mut summary = String::new();

        // 1. Workspace info
        if let Ok(ws) = WorkspaceInfo::load(root_dir) {
            let blueprint = ws.render_blueprint();
            self.set_meta("project_type", "workspace")?;
            self.set_meta("root_dir", &root_dir.to_string_lossy())?;
            self.set_meta("members_count", &ws.members.len().to_string())?;

            self.set_memory(
                "workspace:blueprint",
                "architecture",
                &blueprint,
            )?;
            summary.push_str(&format!("Indexed workspace with {} member(s).\n", ws.members.len()));

            // Store individual member summaries
            for member in &ws.members {
                if let Ok(info) = CrateLocator::locate(&member.abs_path.to_string_lossy()) {
                    let analyzer = CrateAnalyzer::new(info);
                    if let Ok(index) = analyzer.analyze() {
                        let cheat = DocGenerator::generate_cheat_sheet(&index);
                        let mem_key = format!("crate:{}", index.info.name);
                        self.set_memory(&mem_key, "cheat_sheet", &cheat)?;
                    }
                }
            }
        } else if let Ok(info) = CrateLocator::locate(&root_dir.to_string_lossy()) {
            let analyzer = CrateAnalyzer::new(info);
            let index = analyzer.analyze()?;
            let cheat = DocGenerator::generate_cheat_sheet(&index);

            self.set_meta("project_type", "single_crate")?;
            self.set_meta("root_dir", &root_dir.to_string_lossy())?;
            self.set_meta("crate_name", &index.info.name)?;

            self.set_memory(
                &format!("crate:{}", index.info.name),
                "cheat_sheet",
                &cheat,
            )?;
            summary.push_str(&format!("Indexed crate '{}' with {} public symbols.\n", index.info.name, index.stats.public_symbols));
        } else {
            summary.push_str("Initialized empty cratemd memory database.\n");
        }

        Ok(summary)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO project_meta (key, value, updated_at) VALUES (?1, ?2, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value],
        )?;
        Ok(())
    }

    /// Stores or updates an agent memory note and updates FTS5 index
    pub fn set_memory(&self, key: &str, category: &str, content: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO memory_notes (key, category, content, updated_at) VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET category = excluded.category, content = excluded.content, updated_at = excluded.updated_at",
            params![key, category, content],
        )?;

        // Update FTS index: delete old if exists, insert new
        let _ = self.conn.execute("DELETE FROM context_fts WHERE key = ?1", params![key]);
        self.conn.execute(
            "INSERT INTO context_fts (key, category, content) VALUES (?1, ?2, ?3)",
            params![key, category, content],
        )?;

        Ok(())
    }

    /// Retrieves a memory entry by key
    pub fn get_memory(&self, key: &str) -> Result<Option<MemoryEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, category, content, updated_at FROM memory_notes WHERE key = ?1",
        )?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(MemoryEntry {
                key: row.get(0)?,
                category: row.get(1)?,
                content: row.get(2)?,
                updated_at: row.get(3)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Lists all memory keys and categories
    pub fn list_memory(&self) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT key, category, updated_at FROM memory_notes ORDER BY category, key",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;

        let mut res = Vec::new();
        for r in rows {
            res.push(r?);
        }
        Ok(res)
    }

    /// Searches memory and context using SQLite FTS5 full-text search
    pub fn search_memory(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }

        // Clean query for FTS5 (escape quotes)
        let fts_query = format!("\"{}\"", q.replace('"', "\"\""));

        let mut stmt = self.conn.prepare(
            "SELECT m.key, m.category, m.content, m.updated_at
             FROM context_fts f
             JOIN memory_notes m ON f.key = m.key
             WHERE context_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
            Ok(MemoryEntry {
                key: row.get(0)?,
                category: row.get(1)?,
                content: row.get(2)?,
                updated_at: row.get(3)?,
            })
        });

        match rows {
            Ok(mapped) => {
                let mut results = Vec::new();
                for r in mapped {
                    results.push(r?);
                }
                Ok(results)
            }
            Err(_) => {
                // Fallback to LIKE if FTS expression has syntax peculiarities
                let like_param = format!("%{}%", q);
                let mut fallback_stmt = self.conn.prepare(
                    "SELECT key, category, content, updated_at
                     FROM memory_notes
                     WHERE key LIKE ?1 OR content LIKE ?1
                     LIMIT ?2",
                )?;
                let fb_rows = fallback_stmt.query_map(params![like_param, limit as i64], |row| {
                    Ok(MemoryEntry {
                        key: row.get(0)?,
                        category: row.get(1)?,
                        content: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                })?;
                let mut results = Vec::new();
                for r in fb_rows {
                    results.push(r?);
                }
                Ok(results)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_db_in_memory() {
        let conn = Connection::open_in_memory().unwrap();
        let db = ProjectDb {
            conn,
            db_path: PathBuf::from(":memory:"),
        };
        db.init_schema().unwrap();

        db.set_memory("auth:flow", "architecture", "OAuth2 PKCE flow implemented in router")
            .unwrap();

        let mem = db.get_memory("auth:flow").unwrap().unwrap();
        assert_eq!(mem.category, "architecture");
        assert!(mem.content.contains("OAuth2 PKCE"));

        let search_res = db.search_memory("PKCE", 5).unwrap();
        assert_eq!(search_res.len(), 1);
        assert_eq!(search_res[0].key, "auth:flow");
    }
}
