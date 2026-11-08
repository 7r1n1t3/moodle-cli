//! SQLite store in `~/.local/share/moodle-cli/moodle-cli.db`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

const MIGRATIONS: &[&str] = &["
    CREATE TABLE todo_items(
        key TEXT PRIMARY KEY,
        course_id INTEGER NOT NULL,
        course_name TEXT NOT NULL,
        kind TEXT NOT NULL,
        moodle_id INTEGER NOT NULL,
        name TEXT NOT NULL,
        due INTEGER NOT NULL,
        status TEXT NOT NULL,
        first_seen INTEGER NOT NULL,
        last_seen INTEGER NOT NULL
    );
    CREATE TABLE downloads(
        id INTEGER PRIMARY KEY,
        course_id INTEGER NOT NULL,
        course_name TEXT NOT NULL,
        file_url TEXT NOT NULL,
        filename TEXT NOT NULL,
        timemodified INTEGER NOT NULL,
        saved_path TEXT NOT NULL,
        downloaded_at INTEGER NOT NULL
    );
    CREATE INDEX downloads_url ON downloads(file_url);
"];

pub struct Db {
    conn: Connection,
}

pub struct TodoRow {
    pub key: String,
    pub course_id: i64,
    pub course_name: String,
    pub kind: &'static str,
    pub moodle_id: i64,
    pub name: String,
    pub due: i64,
    pub status: String,
}

pub struct DownloadRow {
    pub course_id: i64,
    pub course_name: String,
    pub file_url: String,
    pub filename: String,
    pub timemodified: i64,
    pub saved_path: String,
    pub downloaded_at: i64,
}

/// Most recent download of a file.
#[derive(Debug, PartialEq)]
pub struct LastDownload {
    pub timemodified: i64,
    pub saved_path: String,
    pub downloaded_at: i64,
}

impl Db {
    pub fn open_default() -> Result<Self> {
        let dir = dirs::data_dir()
            .context("cannot find the data directory")?
            .join("moodle-cli");
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        Self::open(&dir.join("moodle-cli.db"))
    }

    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path).with_context(|| format!("opening {}", path.display()))?)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let tx = conn.transaction()?;
        for (i, sql) in (0i64..).zip(MIGRATIONS).skip(version as usize) {
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i + 1)?;
        }
        tx.commit()?;
        Ok(Self { conn })
    }

    pub fn todo_keys(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT key FROM todo_items")?;
        let keys = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(keys)
    }

    pub fn upsert_todos(&mut self, rows: &[TodoRow], now: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO todo_items
                    (key, course_id, course_name, kind, moodle_id, name, due, status, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
                 ON CONFLICT(key) DO UPDATE SET
                    course_name = excluded.course_name, name = excluded.name, due = excluded.due,
                    status = excluded.status, last_seen = excluded.last_seen",
            )?;
            for r in rows {
                stmt.execute(params![
                    r.key,
                    r.course_id,
                    r.course_name,
                    r.kind,
                    r.moodle_id,
                    r.name,
                    r.due,
                    r.status,
                    now
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn record_download(&self, r: &DownloadRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO downloads
                (course_id, course_name, file_url, filename, timemodified, saved_path, downloaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                r.course_id,
                r.course_name,
                r.file_url,
                r.filename,
                r.timemodified,
                r.saved_path,
                r.downloaded_at
            ],
        )?;
        Ok(())
    }

    /// Latest download per file URL.
    pub fn last_downloads(&self) -> Result<HashMap<String, LastDownload>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_url, timemodified, saved_path, downloaded_at FROM downloads
             ORDER BY downloaded_at, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                LastDownload {
                    timemodified: r.get(1)?,
                    saved_path: r.get(2)?,
                    downloaded_at: r.get(3)?,
                },
            ))
        })?;
        // Later rows overwrite earlier ones.
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo(key: &str, name: &str) -> TodoRow {
        TodoRow {
            key: key.into(),
            course_id: 1,
            course_name: "C".into(),
            kind: "assign",
            moodle_id: 7,
            name: name.into(),
            due: 100,
            status: "new".into(),
        }
    }

    #[test]
    fn todos_upsert_keeps_first_seen() {
        let mut db = Db::in_memory().unwrap();
        assert!(db.todo_keys().unwrap().is_empty());
        db.upsert_todos(&[todo("assign:7", "A")], 10).unwrap();
        db.upsert_todos(&[todo("assign:7", "B")], 20).unwrap();
        assert!(db.todo_keys().unwrap().contains("assign:7"));
        let (name, first, last): (String, i64, i64) = db
            .conn
            .query_row(
                "SELECT name, first_seen, last_seen FROM todo_items",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((name.as_str(), first, last), ("B", 10, 20));
    }

    #[test]
    fn last_download_wins() {
        let db = Db::in_memory().unwrap();
        for (path, at) in [("/a", 1), ("/b", 2)] {
            db.record_download(&DownloadRow {
                course_id: 1,
                course_name: "C".into(),
                file_url: "u".into(),
                filename: "f".into(),
                timemodified: 5,
                saved_path: path.into(),
                downloaded_at: at,
            })
            .unwrap();
        }
        let last = db.last_downloads().unwrap();
        assert_eq!(
            last["u"],
            LastDownload {
                timemodified: 5,
                saved_path: "/b".into(),
                downloaded_at: 2
            }
        );
    }
}
