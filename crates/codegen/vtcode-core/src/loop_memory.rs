//! Loop memory store for durable loop-engineering state.
//!
//! The loop memory store provides append-only markdown files that the agent
//! writes during a loop run and reads on the next iteration. This is the
//! "memory" primitive from Osmani's loop engineering pattern:
//!
//! > "The agent forgets, the repo doesn't."
//!
//! The default implementation writes to `{workspace}/.vtcode/state/notes.md`
//! and `{workspace}/.vtcode/state/decisions.md`.

use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::loop_state::state_dir;

const NOTES_FILENAME: &str = "notes.md";
const DECISIONS_FILENAME: &str = "decisions.md";

// ─── Trait ───────────────────────────────────────────────────────────────────

/// A trait for persisting loop-level memory across runs. Implementations may
/// store notes and decisions in markdown, a database, or any other durable form.
pub trait LoopMemoryStore: Send + Sync {
    /// Read all accumulated notes.
    fn read_notes(&self) -> Result<String>;

    /// Append a note. Notes are agent-written, human-readable, append-only.
    fn write_note(&self, note: &str) -> Result<()>;

    /// Read all accumulated decisions.
    fn read_decisions(&self) -> Result<String>;

    /// Append a decision entry. Decisions are the agent's choices that should
    /// survive across loop iterations.
    fn write_decision(&self, decision: &str) -> Result<()>;
}

// ─── Markdown Implementation ─────────────────────────────────────────────────

/// Markdown-backed loop memory store. Writes to `.vtcode/state/notes.md` and
/// `.vtcode/state/decisions.md` under the workspace root.
pub struct MarkdownLoopMemory {
    notes_path: PathBuf,
    decisions_path: PathBuf,
}

impl MarkdownLoopMemory {
    /// Create a new markdown loop memory store for the given workspace.
    pub fn new(workspace_root: &Path) -> Self {
        let dir = state_dir(workspace_root);
        Self {
            notes_path: dir.join(NOTES_FILENAME),
            decisions_path: dir.join(DECISIONS_FILENAME),
        }
    }

    /// Create a new markdown loop memory store with explicit paths (for testing).
    pub fn with_paths(notes_path: PathBuf, decisions_path: PathBuf) -> Self {
        Self { notes_path, decisions_path }
    }
}

impl LoopMemoryStore for MarkdownLoopMemory {
    fn read_notes(&self) -> Result<String> {
        read_markdown_file(&self.notes_path)
    }

    fn write_note(&self, note: &str) -> Result<()> {
        append_markdown_entry(&self.notes_path, note, "# Loop Notes\n\n")
    }

    fn read_decisions(&self) -> Result<String> {
        read_markdown_file(&self.decisions_path)
    }

    fn write_decision(&self, decision: &str) -> Result<()> {
        append_markdown_entry(&self.decisions_path, decision, "# Loop Decisions\n\n")
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn read_markdown_file(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))
}

fn append_markdown_entry(path: &Path, content: &str, header: &str) -> Result<()> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let entry = format!("- [{timestamp}] {trimmed}\n");

    // Open a single file handle with read+append+create to avoid TOCTOU race
    // between checking whether the header exists and writing the entry.
    let mut file = OpenOptions::new()
        .read(true)
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open {} for appending", path.display()))?;

    // Check if the file is empty (needs a header) using the same handle.
    let needs_header = {
        let metadata = file.metadata().with_context(|| format!("Failed to stat {}", path.display()))?;
        metadata.len() == 0
    };

    if needs_header {
        file.write_all(header.as_bytes())
            .with_context(|| format!("Failed to write header to {}", path.display()))?;
    }

    file.write_all(entry.as_bytes())
        .with_context(|| format!("Failed to append to {}", path.display()))?;
    Ok(())
}

// ─── Sqlite Implementation ───────────────────────────────────────────────────

/// Sqlite-backed loop memory store for faster queries. Enabled behind the
/// `sqlite` feature flag. Stores notes and decisions in a single database
/// file under `.vtcode/state/loop_memory.db`.
#[cfg(feature = "sqlite")]
pub struct SqliteLoopMemory {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

#[cfg(feature = "sqlite")]
impl SqliteLoopMemory {
    /// Create a new sqlite loop memory store for the given workspace.
    pub fn new(workspace_root: &Path) -> Result<Self> {
        let dir = state_dir(workspace_root);
        let db_path = dir.join("loop_memory.db");
        Self::from_path(db_path)
    }

    /// Create with an explicit database path (for testing).
    pub fn with_path(db_path: PathBuf) -> Result<Self> {
        Self::from_path(db_path)
    }

    fn from_path(db_path: PathBuf) -> Result<Self> {
        let conn = rusqlite::Connection::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                content TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                content TEXT NOT NULL
            );",
        )?;
        Ok(Self { conn: std::sync::Mutex::new(conn) })
    }
}

#[cfg(feature = "sqlite")]
impl LoopMemoryStore for SqliteLoopMemory {
    fn read_notes(&self) -> Result<String> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        let mut stmt = conn.prepare("SELECT timestamp, content FROM notes ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            let ts: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok(format!("- [{ts}] {content}"))
        })?;

        let entries: Vec<String> = rows.collect::<Result<Vec<_>, _>>()?;
        // Match Markdown behavior: return empty string when no entries exist.
        if entries.is_empty() {
            return Ok(String::new());
        }
        let mut output = String::from("# Loop Notes\n\n");
        for entry in entries {
            output.push_str(&entry);
            output.push('\n');
        }
        Ok(output)
    }

    fn write_note(&self, note: &str) -> Result<()> {
        let trimmed = note.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        conn.execute(
            "INSERT INTO notes (timestamp, content) VALUES (?1, ?2)",
            rusqlite::params![timestamp.to_string(), trimmed],
        )?;
        Ok(())
    }

    fn read_decisions(&self) -> Result<String> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        let mut stmt = conn.prepare("SELECT timestamp, content FROM decisions ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            let ts: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok(format!("- [{ts}] {content}"))
        })?;

        let entries: Vec<String> = rows.collect::<Result<Vec<_>, _>>()?;
        if entries.is_empty() {
            return Ok(String::new());
        }
        let mut output = String::from("# Loop Decisions\n\n");
        for entry in entries {
            output.push_str(&entry);
            output.push('\n');
        }
        Ok(output)
    }

    fn write_decision(&self, decision: &str) -> Result<()> {
        let trimmed = decision.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
        conn.execute(
            "INSERT INTO decisions (timestamp, content) VALUES (?1, ?2)",
            rusqlite::params![timestamp.to_string(), trimmed],
        )?;
        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn markdown_memory_read_empty_when_no_files() {
        let tmp = TempDir::new().expect("temp dir");
        let memory = MarkdownLoopMemory::new(tmp.path());
        assert_eq!(memory.read_notes().expect("read"), "");
        assert_eq!(memory.read_decisions().expect("read"), "");
    }

    #[test]
    fn markdown_memory_write_and_read_notes() {
        let tmp = TempDir::new().expect("temp dir");
        let memory = MarkdownLoopMemory::new(tmp.path());

        memory.write_note("First observation").expect("write");
        memory.write_note("Second observation").expect("write");

        let notes = memory.read_notes().expect("read");
        assert!(notes.contains("First observation"));
        assert!(notes.contains("Second observation"));
        assert!(notes.starts_with("# Loop Notes"));
    }

    #[test]
    fn markdown_memory_write_and_read_decisions() {
        let tmp = TempDir::new().expect("temp dir");
        let memory = MarkdownLoopMemory::new(tmp.path());

        memory.write_decision("Use retry with backoff").expect("write");
        memory.write_decision("Skip tests for now").expect("write");

        let decisions = memory.read_decisions().expect("read");
        assert!(decisions.contains("Use retry with backoff"));
        assert!(decisions.contains("Skip tests for now"));
        assert!(decisions.starts_with("# Loop Decisions"));
    }

    #[test]
    fn markdown_memory_ignores_empty_entries() {
        let tmp = TempDir::new().expect("temp dir");
        let memory = MarkdownLoopMemory::new(tmp.path());

        memory.write_note("").expect("write");
        memory.write_note("   ").expect("write");

        let notes = memory.read_notes().expect("read");
        assert_eq!(notes, "");
    }

    #[test]
    fn markdown_memory_entries_are_timestamped() {
        let tmp = TempDir::new().expect("temp dir");
        let notes_path = tmp.path().join("notes.md");
        let decisions_path = tmp.path().join("decisions.md");
        let memory = MarkdownLoopMemory::with_paths(notes_path.clone(), decisions_path);

        memory.write_note("test entry").expect("write");

        let content = fs::read_to_string(&notes_path).expect("read");
        // Should contain a timestamp in [YYYY-MM-DDTHH:MM:SSZ] format
        assert!(content.contains("[20"));
        assert!(content.contains("] test entry"));
    }

    #[test]
    fn markdown_memory_append_is_durable() {
        let tmp = TempDir::new().expect("temp dir");
        let notes_path = tmp.path().join("notes.md");
        let decisions_path = tmp.path().join("decisions.md");

        {
            let memory = MarkdownLoopMemory::with_paths(notes_path.clone(), decisions_path.clone());
            memory.write_note("first run note").expect("write");
        }
        // Simulate a new run (drop and recreate)
        {
            let memory = MarkdownLoopMemory::with_paths(notes_path, decisions_path);
            memory.write_note("second run note").expect("write");
            let notes = memory.read_notes().expect("read");
            assert!(notes.contains("first run note"));
            assert!(notes.contains("second run note"));
        }
    }

    #[cfg(feature = "sqlite")]
    mod sqlite_tests {
        use super::super::*;
        use tempfile::TempDir;

        #[test]
        fn sqlite_memory_read_empty_when_no_data() {
            let tmp = TempDir::new().expect("temp dir");
            let db_path = tmp.path().join("test.db");
            let memory = SqliteLoopMemory::with_path(db_path).expect("create");
            // Empty state matches Markdown behavior: returns empty string.
            assert_eq!(memory.read_notes().expect("read"), "");
            assert_eq!(memory.read_decisions().expect("read"), "");
        }

        #[test]
        fn sqlite_memory_write_and_read_notes() {
            let tmp = TempDir::new().expect("temp dir");
            let db_path = tmp.path().join("test.db");
            let memory = SqliteLoopMemory::with_path(db_path).expect("create");

            memory.write_note("First observation").expect("write");
            memory.write_note("Second observation").expect("write");

            let notes = memory.read_notes().expect("read");
            assert!(notes.contains("First observation"));
            assert!(notes.contains("Second observation"));
            assert!(notes.starts_with("# Loop Notes"));
        }

        #[test]
        fn sqlite_memory_write_and_read_decisions() {
            let tmp = TempDir::new().expect("temp dir");
            let db_path = tmp.path().join("test.db");
            let memory = SqliteLoopMemory::with_path(db_path).expect("create");

            memory.write_decision("Use retry with backoff").expect("write");
            memory.write_decision("Skip tests for now").expect("write");

            let decisions = memory.read_decisions().expect("read");
            assert!(decisions.contains("Use retry with backoff"));
            assert!(decisions.contains("Skip tests for now"));
            assert!(decisions.starts_with("# Loop Decisions"));
        }

        #[test]
        fn sqlite_memory_ignores_empty_entries() {
            let tmp = TempDir::new().expect("temp dir");
            let db_path = tmp.path().join("test.db");
            let memory = SqliteLoopMemory::with_path(db_path).expect("create");

            memory.write_note("").expect("write");
            memory.write_note("   ").expect("write");

            let notes = memory.read_notes().expect("read");
            // Empty entries are skipped, so the result is empty (no rows).
            assert_eq!(notes, "");
        }

        #[test]
        fn sqlite_memory_survives_reopen() {
            let tmp = TempDir::new().expect("temp dir");
            let db_path = tmp.path().join("test.db");

            {
                let memory = SqliteLoopMemory::with_path(db_path.clone()).expect("create");
                memory.write_note("first run note").expect("write");
            }
            {
                let memory = SqliteLoopMemory::with_path(db_path).expect("reopen");
                memory.write_note("second run note").expect("write");
                let notes = memory.read_notes().expect("read");
                assert!(notes.contains("first run note"));
                assert!(notes.contains("second run note"));
            }
        }
    }
}
