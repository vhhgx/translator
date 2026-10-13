//! Translation history, stored in SQLite in the app data dir.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub id: String,
    pub created_at: i64,
    pub source: String,
    pub translation: String,
    pub source_lang: String,
    pub target_lang: String,
    pub backend_id: String,
    pub mode: String,
}

fn db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hy-mt2-translator")
        .join("history.db")
}

pub struct History {
    conn: Connection,
}

impl History {
    pub fn open() -> AppResult<Self> {
        let path = db_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| AppError::Io(e.to_string()))?;
        }
        let conn = Connection::open(&path).map_err(|e| AppError::Io(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id          TEXT PRIMARY KEY,
                created_at  INTEGER NOT NULL,
                source      TEXT NOT NULL,
                translation TEXT NOT NULL,
                source_lang TEXT NOT NULL,
                target_lang TEXT NOT NULL,
                backend_id  TEXT NOT NULL,
                mode        TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_created ON history(created_at DESC);",
        )
        .map_err(|e| AppError::Io(e.to_string()))?;
        Ok(Self { conn })
    }

    pub fn add(&self, item: &HistoryItem) -> AppResult<()> {
        self.conn
            .execute(
                "INSERT INTO history
                 (id, created_at, source, translation, source_lang, target_lang, backend_id, mode)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    item.id,
                    item.created_at,
                    item.source,
                    item.translation,
                    item.source_lang,
                    item.target_lang,
                    item.backend_id,
                    item.mode
                ],
            )
            .map_err(|e| AppError::Io(e.to_string()))?;
        Ok(())
    }

    pub fn list(&self, search: Option<&str>, limit: i64) -> AppResult<Vec<HistoryItem>> {
        let limit = limit.clamp(1, 500);
        let mut stmt = match search.filter(|s| !s.trim().is_empty()) {
            None => self.conn.prepare(
                "SELECT id, created_at, source, translation, source_lang, target_lang, backend_id, mode
                 FROM history ORDER BY created_at DESC LIMIT ?1",
            ),
            Some(_) => self.conn.prepare(
                "SELECT id, created_at, source, translation, source_lang, target_lang, backend_id, mode
                 FROM history WHERE source LIKE ?2 OR translation LIKE ?2
                 ORDER BY created_at DESC LIMIT ?1",
            ),
        }
        .map_err(|e| AppError::Io(e.to_string()))?;

        let pattern = format!("%{}%", search.unwrap_or("").trim());
        let rows = match search.filter(|s| !s.trim().is_empty()) {
            None => stmt.query_map([limit], map_row),
            Some(_) => stmt.query_map(params![limit, pattern], map_row),
        }
        .map_err(|e| AppError::Io(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| AppError::Io(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        self.conn
            .execute("DELETE FROM history WHERE id = ?1", params![id])
            .map_err(|e| AppError::Io(e.to_string()))?;
        Ok(())
    }

    pub fn clear(&self) -> AppResult<()> {
        self.conn
            .execute("DELETE FROM history", [])
            .map_err(|e| AppError::Io(e.to_string()))?;
        Ok(())
    }
}

type Row = rusqlite::Result<HistoryItem>;

fn map_row(r: &rusqlite::Row<'_>) -> Row {
    Ok(HistoryItem {
        id: r.get(0)?,
        created_at: r.get(1)?,
        source: r.get(2)?,
        translation: r.get(3)?,
        source_lang: r.get(4)?,
        target_lang: r.get(5)?,
        backend_id: r.get(6)?,
        mode: r.get(7)?,
    })
}