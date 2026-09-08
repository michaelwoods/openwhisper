use anyhow::{Context, Result};
use directories::ProjectDirs;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub mod gui;

/// Represents a single recorded transcription event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: Option<i64>,
    pub timestamp: String,
    pub text: String,
    pub raw_text: Option<String>,
    pub duration_secs: f32,
    pub char_count: usize,
    pub model: String,
    pub output_mode: String,
    pub audio_path: Option<String>,
}

/// Thread-safe SQLite persistence manager for dictation history.
pub struct HistoryManager {
    conn: Mutex<Connection>,
    db_path: Option<PathBuf>,
}

impl HistoryManager {
    /// Opens or creates the SQLite history database at the standard user data location
    /// (`~/.local/share/openwhisper/history.sqlite3`) or a custom provided path.
    pub fn new(db_path: Option<PathBuf>) -> Result<Self> {
        let path = match db_path {
            Some(p) => p,
            None => {
                let dirs = ProjectDirs::from("net", "local", "openwhisper")
                    .context("Could not determine user local data directory")?;
                let data_dir = dirs.data_dir();
                std::fs::create_dir_all(data_dir)
                    .with_context(|| format!("Failed to create directory {:?}", data_dir))?;
                data_dir.join("history.sqlite3")
            }
        };

        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open history database at {:?}", path))?;

        // Enable Write-Ahead Logging (WAL) and synchronous = NORMAL for high-concurrency and speed.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )
        .context("Failed to configure SQLite pragmas")?;

        let mgr = Self {
            conn: Mutex::new(conn),
            db_path: Some(path),
        };
        mgr.init_db()?;
        Ok(mgr)
    }

    /// Creates an in-memory database instance (primarily for fast, isolated unit tests).
    pub fn in_memory() -> Result<Self> {
        let conn =
            Connection::open_in_memory().context("Failed to open in-memory SQLite database")?;
        let mgr = Self {
            conn: Mutex::new(conn),
            db_path: None,
        };
        mgr.init_db()?;
        Ok(mgr)
    }

    /// Initializes schema tables and indexes.
    fn init_db(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS transcriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                text TEXT NOT NULL,
                raw_text TEXT,
                duration_secs REAL NOT NULL,
                char_count INTEGER NOT NULL,
                model TEXT NOT NULL,
                output_mode TEXT NOT NULL,
                audio_path TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_transcriptions_timestamp ON transcriptions(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_transcriptions_id_desc ON transcriptions(id DESC);",
        )
        .context("Failed to initialize transcriptions table schema")?;

        // Non-breaking migration: ensure audio_path column exists for databases created before this version
        let _ = conn.execute("ALTER TABLE transcriptions ADD COLUMN audio_path TEXT;", []);

        Ok(())
    }

    /// Returns the database path on disk, or None if in-memory.
    pub fn path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }

    /// Inserts a new transcription entry, returning the auto-generated row ID.
    pub fn record(&self, entry: &HistoryEntry) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO transcriptions (timestamp, text, raw_text, duration_secs, char_count, model, output_mode, audio_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entry.timestamp,
                entry.text,
                entry.raw_text,
                entry.duration_secs,
                entry.char_count as i64,
                entry.model,
                entry.output_mode,
                entry.audio_path,
            ],
        )
        .context("Failed to insert history entry")?;

        let id = conn.last_insert_rowid();
        Ok(id)
    }

    /// Lists entries ordered from newest to oldest with pagination.
    pub fn list(&self, limit: usize, offset: usize) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, text, raw_text, duration_secs, char_count, model, output_mode, audio_path
             FROM transcriptions
             ORDER BY id DESC
             LIMIT ?1 OFFSET ?2",
        )?;

        let rows = stmt.query_map(params![limit as i64, offset as i64], |row| {
            let char_count: i64 = row.get(5)?;
            Ok(HistoryEntry {
                id: Some(row.get(0)?),
                timestamp: row.get(1)?,
                text: row.get(2)?,
                raw_text: row.get(3)?,
                duration_secs: row.get(4)?,
                char_count: char_count as usize,
                model: row.get(6)?,
                output_mode: row.get(7)?,
                audio_path: row.get(8)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// Searches transcriptions for matching substring in the text, ordered from newest to oldest.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, text, raw_text, duration_secs, char_count, model, output_mode, audio_path
             FROM transcriptions
             WHERE text LIKE ?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![pattern, limit as i64], |row| {
            let char_count: i64 = row.get(5)?;
            Ok(HistoryEntry {
                id: Some(row.get(0)?),
                timestamp: row.get(1)?,
                text: row.get(2)?,
                raw_text: row.get(3)?,
                duration_secs: row.get(4)?,
                char_count: char_count as usize,
                model: row.get(6)?,
                output_mode: row.get(7)?,
                audio_path: row.get(8)?,
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    /// Retrieves a single history entry by its ID.
    pub fn get_by_id(&self, id: i64) -> Result<Option<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, text, raw_text, duration_secs, char_count, model, output_mode, audio_path
             FROM transcriptions
             WHERE id = ?1",
        )?;

        let mut rows = stmt.query_map(params![id], |row| {
            let char_count: i64 = row.get(5)?;
            Ok(HistoryEntry {
                id: Some(row.get(0)?),
                timestamp: row.get(1)?,
                text: row.get(2)?,
                raw_text: row.get(3)?,
                duration_secs: row.get(4)?,
                char_count: char_count as usize,
                model: row.get(6)?,
                output_mode: row.get(7)?,
                audio_path: row.get(8)?,
            })
        })?;

        if let Some(res) = rows.next() {
            Ok(Some(res?))
        } else {
            Ok(None)
        }
    }

    /// Updates the audio_path for a specific history entry.
    #[allow(dead_code)]
    pub fn update_audio_path(&self, id: i64, audio_path: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE transcriptions SET audio_path = ?1 WHERE id = ?2",
            params![audio_path, id],
        )?;
        Ok(())
    }

    /// Scans a recordings directory and links previously unlinked history entries
    /// to matching .wav audio files by transcript text or timestamp.
    /// Returns the number of entries successfully linked.
    pub fn backfill_audio_paths(&self, dir: &Path) -> Result<usize> {
        if !dir.is_dir() {
            return Ok(0);
        }

        let mut linked = 0;
        let entries = std::fs::read_dir(dir)?;
        let mut wav_files = Vec::new();

        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|ext| ext == "wav") {
                wav_files.push(p);
            }
        }

        // Sort WAV files so older ones match older entries first
        wav_files.sort();

        let conn = self.conn.lock().unwrap();
        for wav_path in wav_files {
            let txt_path = wav_path.with_extension("txt");
            let wav_str = wav_path.to_string_lossy().to_string();

            // First check if this audio_path is already recorded
            let already_linked: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM transcriptions WHERE audio_path = ?1",
                    params![&wav_str],
                    |r| r.get(0),
                )
                .unwrap_or(false);

            if already_linked {
                continue;
            }

            if txt_path.exists()
                && let Ok(content) = std::fs::read_to_string(&txt_path)
            {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    // Find the oldest unlinked entry matching the trimmed text
                    let target_id: Option<i64> = conn
                        .query_row(
                            "SELECT id FROM transcriptions
                                 WHERE (audio_path IS NULL OR audio_path = '') AND TRIM(text) = ?1
                                 ORDER BY id ASC LIMIT 1",
                            params![trimmed],
                            |r| r.get(0),
                        )
                        .ok();

                    if let Some(id) = target_id {
                        let _ = conn.execute(
                            "UPDATE transcriptions SET audio_path = ?1 WHERE id = ?2",
                            params![&wav_str, id],
                        );
                        tracing::info!(
                            "Backfilled audio_path for history entry #{}: {:?}",
                            id,
                            wav_str
                        );
                        linked += 1;
                    }
                }
            }
        }

        Ok(linked)
    }

    /// Deletes a single history entry by its ID. Returns true if a row was deleted.
    pub fn delete(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count = conn.execute("DELETE FROM transcriptions WHERE id = ?1", params![id])?;
        Ok(count > 0)
    }

    /// Clears all history entries and compacts the SQLite file.
    pub fn clear_all(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM transcriptions", [])?;
        conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    /// Returns the total count of stored transcription entries.
    pub fn count(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM transcriptions", [], |r| r.get(0))?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(text: &str) -> HistoryEntry {
        HistoryEntry {
            id: None,
            timestamp: "2026-09-07T21:00:00Z".to_string(),
            text: text.to_string(),
            raw_text: Some(text.to_string()),
            duration_secs: 2.5,
            char_count: text.chars().count(),
            model: "whisper-base".to_string(),
            output_mode: "clipboard_and_typing".to_string(),
            audio_path: None,
        }
    }

    #[test]
    fn test_history_audio_path_and_backfill() {
        let mgr = HistoryManager::in_memory().expect("in-memory db");
        let mut entry = sample_entry("Dictation with audio");
        entry.audio_path = Some("/path/to/test.wav".to_string());
        let id = mgr.record(&entry).unwrap();

        let retrieved = mgr.get_by_id(id).unwrap().unwrap();
        assert_eq!(retrieved.audio_path.as_deref(), Some("/path/to/test.wav"));

        // Test backfill
        let tmp_dir =
            std::env::temp_dir().join(format!("openwhisper_test_backfill_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let wav_file = tmp_dir.join("whisper_20260907_120000.wav");
        let txt_file = tmp_dir.join("whisper_20260907_120000.txt");
        std::fs::write(&wav_file, b"RIFFFAKE").unwrap();
        std::fs::write(&txt_file, "Unlinked test phrase").unwrap();

        let unlinked_id = mgr.record(&sample_entry("Unlinked test phrase")).unwrap();
        let unlinked_entry = mgr.get_by_id(unlinked_id).unwrap().unwrap();
        assert!(unlinked_entry.audio_path.is_none());

        let count = mgr.backfill_audio_paths(&tmp_dir).unwrap();
        assert_eq!(count, 1);

        let linked_entry = mgr.get_by_id(unlinked_id).unwrap().unwrap();
        assert_eq!(
            linked_entry.audio_path,
            Some(wav_file.to_string_lossy().to_string())
        );

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn test_history_crud() {
        let mgr = HistoryManager::in_memory().expect("in-memory db");
        assert_eq!(mgr.count().unwrap(), 0);

        // Record
        let id1 = mgr.record(&sample_entry("Hello world")).unwrap();
        let id2 = mgr.record(&sample_entry("Rust is awesome")).unwrap();
        assert_eq!(mgr.count().unwrap(), 2);
        assert!(id2 > id1);

        // List
        let list = mgr.list(10, 0).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].text, "Rust is awesome");
        assert_eq!(list[1].text, "Hello world");

        // Get by ID
        let entry = mgr.get_by_id(id1).unwrap().expect("found entry");
        assert_eq!(entry.text, "Hello world");

        // Delete
        assert!(mgr.delete(id1).unwrap());
        assert_eq!(mgr.count().unwrap(), 1);
        assert!(mgr.get_by_id(id1).unwrap().is_none());

        // Clear all
        mgr.clear_all().unwrap();
        assert_eq!(mgr.count().unwrap(), 0);
    }

    #[test]
    fn test_history_search() {
        let mgr = HistoryManager::in_memory().expect("in-memory db");
        mgr.record(&sample_entry("Meeting notes for Monday"))
            .unwrap();
        mgr.record(&sample_entry("Code review on Wayland text input"))
            .unwrap();
        mgr.record(&sample_entry("Call with team on Tuesday"))
            .unwrap();

        let results = mgr.search("Monday", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text, "Meeting notes for Monday");

        let results_case = mgr.search("wayland", 10).unwrap(); // SQLite LIKE is case-insensitive for ASCII
        assert_eq!(results_case.len(), 1);
        assert_eq!(results_case[0].text, "Code review on Wayland text input");

        let results_multi = mgr.search("team", 10).unwrap();
        assert_eq!(results_multi.len(), 1);

        let results_none = mgr.search("nonexistent", 10).unwrap();
        assert_eq!(results_none.len(), 0);
    }
}
