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
    pub starred: bool,
}

fn db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("zephyr-translator")
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
                mode        TEXT NOT NULL,
                starred     INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_created ON history(created_at DESC);",
        )
        .map_err(|e| AppError::Io(e.to_string()))?;
        // 旧库升级：建表语句只对新建库生效，已有库需要补列。
        let has_starred = {
            let mut stmt = conn
                .prepare("PRAGMA table_info(history)")
                .map_err(|e| AppError::Io(e.to_string()))?;
            let cols = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(|e| AppError::Io(e.to_string()))?;
            let has = cols.flatten().any(|c| c == "starred");
            has
        };
        if !has_starred {
            conn.execute_batch("ALTER TABLE history ADD COLUMN starred INTEGER NOT NULL DEFAULT 0")
                .map_err(|e| AppError::Io(e.to_string()))?;
        }
        Ok(Self { conn })
    }

    pub fn add(&self, item: &HistoryItem) -> AppResult<()> {
        self.conn
            .execute(
                "INSERT INTO history
                 (id, created_at, source, translation, source_lang, target_lang, backend_id, mode, starred)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    item.id,
                    item.created_at,
                    item.source,
                    item.translation,
                    item.source_lang,
                    item.target_lang,
                    item.backend_id,
                    item.mode,
                    item.starred as i64
                ],
            )
            .map_err(|e| AppError::Io(e.to_string()))?;
        Ok(())
    }

    /// `starred`: `Some(true)` 只取收藏，`Some(false)` 只取未收藏，`None` 全部。
    pub fn list(
        &self,
        search: Option<&str>,
        starred: Option<bool>,
        limit: i64,
    ) -> AppResult<Vec<HistoryItem>> {
        let limit = limit.clamp(1, 500);
        const COLS: &str = "id, created_at, source, translation, source_lang, target_lang, backend_id, mode, starred";
        let (sql, starred_filter) = match (search.filter(|s| !s.trim().is_empty()), starred) {
            (None, None) => (
                format!("SELECT {COLS} FROM history ORDER BY created_at DESC LIMIT ?1"),
                None,
            ),
            (Some(_), None) => (
                format!(
                    "SELECT {COLS} FROM history WHERE source LIKE ?2 OR translation LIKE ?2
                     ORDER BY created_at DESC LIMIT ?1"
                ),
                None,
            ),
            (None, Some(s)) => (
                format!(
                    "SELECT {COLS} FROM history WHERE starred = ?2
                     ORDER BY created_at DESC LIMIT ?1"
                ),
                Some(s),
            ),
            (Some(_), Some(s)) => (
                format!(
                    "SELECT {COLS} FROM history
                     WHERE (source LIKE ?3 OR translation LIKE ?3) AND starred = ?2
                     ORDER BY created_at DESC LIMIT ?1"
                ),
                Some(s),
            ),
        };
        let mut stmt = self.conn.prepare(&sql).map_err(|e| AppError::Io(e.to_string()))?;

        let pattern = format!("%{}%", search.unwrap_or("").trim());
        let rows = match (search.filter(|s| !s.trim().is_empty()), starred_filter) {
            (None, None) => stmt.query_map([limit], map_row),
            (Some(_), None) => stmt.query_map(params![limit, pattern], map_row),
            (None, Some(s)) => stmt.query_map(params![limit, s as i64], map_row),
            (Some(_), Some(s)) => stmt.query_map(params![limit, s as i64, pattern], map_row),
        }
        .map_err(|e| AppError::Io(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| AppError::Io(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn count(&self) -> AppResult<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .map_err(|e| AppError::Io(e.to_string()))
    }

    pub fn set_starred(&self, id: &str, starred: bool) -> AppResult<()> {
        self.conn
            .execute(
                "UPDATE history SET starred = ?2 WHERE id = ?1",
                params![id, starred as i64],
            )
            .map_err(|e| AppError::Io(e.to_string()))?;
        Ok(())
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
        starred: r.get::<_, i64>(8)? != 0,
    })
}