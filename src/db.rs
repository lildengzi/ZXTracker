use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                app_id     TEXT    NOT NULL,
                pid        INTEGER NOT NULL,
                path       TEXT    DEFAULT '',
                start_time INTEGER NOT NULL,
                end_time   INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_app  ON sessions(app_id);
            CREATE INDEX IF NOT EXISTS idx_sessions_time ON sessions(start_time);

            CREATE TABLE IF NOT EXISTS app_meta (
                app_id       TEXT PRIMARY KEY,
                display_name TEXT DEFAULT '',
                icon_path    TEXT DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS tags (
                name TEXT NOT NULL PRIMARY KEY
            );

            CREATE TABLE IF NOT EXISTS app_tags (
                app_id   TEXT NOT NULL,
                tag_name TEXT NOT NULL REFERENCES tags(name) ON DELETE CASCADE,
                PRIMARY KEY (app_id, tag_name)
            );
            CREATE INDEX IF NOT EXISTS idx_app_tags_tag ON app_tags(tag_name);

            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            ",
        )?;
        Ok(Database { conn: Mutex::new(conn) })
    }

    pub fn start_session(&self, app_id: &str, pid: i64, path: &str, timestamp: i64) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (app_id, pid, path, start_time) VALUES (?1, ?2, ?3, ?4)",
            params![app_id, pid, path, timestamp],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn end_session(&self, id: i64, timestamp: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET end_time = ?1 WHERE id = ?2 AND end_time IS NULL",
            params![timestamp, id],
        )?;
        Ok(())
    }

    pub fn close_active_sessions(&self, timestamp: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET end_time = ?1 WHERE end_time IS NULL",
            params![timestamp],
        )?;
        Ok(())
    }

    pub fn today_usage(&self, date: &str) -> Result<Vec<(String, i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT app_id,
                    COALESCE(SUM(COALESCE(end_time, unixepoch()) - start_time), 0),
                    COUNT(*)
             FROM sessions
             WHERE date(start_time, 'unixepoch', 'localtime') = ?1
             GROUP BY app_id ORDER BY 2 DESC",
        )?;
        let rows = stmt.query_map(params![date], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn range_usage(
        &self,
        from_date: &str,
        to_date: &str,
    ) -> Result<Vec<(String, i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT app_id,
                    COALESCE(SUM(COALESCE(end_time, unixepoch()) - start_time), 0),
                    COUNT(*)
             FROM sessions
             WHERE date(start_time, 'unixepoch', 'localtime') >= ?1 AND date(start_time, 'unixepoch', 'localtime') <= ?2
             GROUP BY app_id ORDER BY 2 DESC",
        )?;
        let rows = stmt.query_map(params![from_date, to_date], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn range_hourly_usage(
        &self,
        from_date: &str,
        to_date: &str,
    ) -> Result<Vec<(String, i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT app_id,
                    CAST(strftime('%H', start_time, 'unixepoch', 'localtime') AS INTEGER) as hour,
                    COALESCE(SUM(COALESCE(end_time, unixepoch()) - start_time), 0)
             FROM sessions
             WHERE date(start_time, 'unixepoch', 'localtime') >= ?1 AND date(start_time, 'unixepoch', 'localtime') <= ?2
             GROUP BY app_id, hour
             ORDER BY app_id, hour",
        )?;
        let rows = stmt.query_map(params![from_date, to_date], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn range_first_seen(
        &self,
        from_date: &str,
        to_date: &str,
    ) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT app_id, CAST(AVG(first_seen) AS INTEGER) FROM (
                SELECT app_id, date(start_time, 'unixepoch', 'localtime') as d, MIN(start_time) as first_seen
                FROM sessions
                WHERE date(start_time, 'unixepoch', 'localtime') >= ?1 AND date(start_time, 'unixepoch', 'localtime') <= ?2
                GROUP BY app_id, d
             ) GROUP BY app_id ORDER BY AVG(first_seen)",
        )?;
        let rows = stmt.query_map(params![from_date, to_date], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn upsert_app_meta(&self, app_id: &str, display_name: &str, icon_path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO app_meta (app_id, display_name, icon_path) VALUES (?1, ?2, ?3)
             ON CONFLICT(app_id) DO UPDATE SET display_name = ?2, icon_path = ?3",
            params![app_id, display_name, icon_path],
        )?;
        Ok(())
    }

    pub fn get_app_meta(&self, app_id: &str) -> Result<Option<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT display_name, icon_path FROM app_meta WHERE app_id = ?1",
        )?;
        let result = stmt
            .query_row(params![app_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .ok();
        Ok(result)
    }

    pub fn get_all_app_meta(&self) -> Result<std::collections::HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT app_id, display_name FROM app_meta WHERE display_name != ''",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut map = std::collections::HashMap::new();
        for (id, name) in rows.flatten() {
            map.insert(id, name);
        }
        Ok(map)
    }

    pub fn add_tag(&self, app_id: &str, tag_name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![tag_name])?;
        conn.execute(
            "INSERT OR IGNORE INTO app_tags (app_id, tag_name) VALUES (?1, ?2)",
            params![app_id, tag_name],
        )?;
        Ok(())
    }

    pub fn remove_tag(&self, app_id: &str, tag_name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM app_tags WHERE app_id = ?1 AND tag_name = ?2",
            params![app_id, tag_name],
        )?;
        Ok(())
    }

    pub fn list_tags(&self) -> Result<Vec<(String, Vec<String>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT t.name, GROUP_CONCAT(at.app_id)
             FROM tags t
             LEFT JOIN app_tags at ON at.tag_name = t.name
             GROUP BY t.name ORDER BY t.name",
        )?;
        let rows = stmt.query_map([], |row| {
            let name: String = row.get(0)?;
            let apps: String = row.get::<_, String>(1).unwrap_or_default();
            let apps: Vec<String> = if apps.is_empty() {
                vec![]
            } else {
                apps.split(',').map(|s| s.to_string()).collect()
            };
            Ok((name, apps))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn labeled_usage(&self, tag_name: &str, date: &str) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.app_id, COALESCE(SUM(COALESCE(s.end_time, unixepoch()) - s.start_time), 0)
             FROM sessions s
             JOIN app_tags at ON at.app_id = s.app_id
             WHERE at.tag_name = ?1 AND date(s.start_time, 'unixepoch') = ?2
             GROUP BY s.app_id ORDER BY 2 DESC",
        )?;
        let rows = stmt.query_map(params![tag_name, date], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    pub fn labeled_range_usage(
        &self,
        tag_name: &str,
        from_date: &str,
        to_date: &str,
    ) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.app_id, COALESCE(SUM(COALESCE(s.end_time, unixepoch()) - s.start_time), 0)
             FROM sessions s
             JOIN app_tags at ON at.app_id = s.app_id
             WHERE at.tag_name = ?1
               AND date(s.start_time, 'unixepoch') >= ?2
               AND date(s.start_time, 'unixepoch') <= ?3
             GROUP BY s.app_id ORDER BY 2 DESC",
        )?;
        let rows = stmt.query_map(params![tag_name, from_date, to_date], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}
