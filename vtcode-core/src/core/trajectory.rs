use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

use crate::telemetry::perf::PerfSpan;
use crate::utils::async_line_writer::AsyncLineWriter;

#[derive(Clone)]
pub struct TrajectoryLogger {
    enabled: bool,
    writer: Option<Arc<AsyncLineWriter>>,
}

impl TrajectoryLogger {
    pub fn new(workspace: &Path) -> Self {
        let dir = workspace.join(".vtcode").join("logs");
        let path = dir.join("trajectory.jsonl");
        let writer = AsyncLineWriter::new(path).ok().map(Arc::new);
        let enabled = writer.is_some();
        Self { enabled, writer }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            writer: None,
        }
    }

    pub fn log<T: Serialize>(&self, record: &T) {
        if !self.enabled {
            return;
        }
        let mut perf = PerfSpan::new("vtcode.perf.trajectory_log_ms");
        perf.tag("mode", "async");
        if let Ok(line) = serde_json::to_string(record)
            && let Some(writer) = self.writer.as_ref()
        {
            writer.write_line(line);
        }
    }

    #[cfg(test)]
    pub fn flush(&self) {
        if let Some(writer) = self.writer.as_ref() {
            writer.flush();
        }
    }

    pub fn log_route(&self, turn: usize, selected_model: &str, class: &str, input_preview: &str) {
        #[derive(Serialize)]
        struct RouteRec<'a> {
            kind: &'static str,
            turn: usize,
            selected_model: &'a str,
            class: &'a str,
            input_preview: &'a str,
            ts: i64,
        }
        let rec = RouteRec {
            kind: "route",
            turn,
            selected_model,
            class,
            input_preview,
            ts: chrono::Utc::now().timestamp(),
        };
        self.log(&rec);
    }

    pub fn log_tool_call(&self, turn: usize, name: &str, args: &serde_json::Value, ok: bool) {
        #[derive(Serialize)]
        struct ToolRec<'a> {
            kind: &'static str,
            turn: usize,
            name: &'a str,
            args: serde_json::Value,
            ok: bool,
            ts: i64,
        }
        let rec = ToolRec {
            kind: "tool",
            turn,
            name,
            args: args.clone(),
            ok,
            ts: chrono::Utc::now().timestamp(),
        };
        self.log(&rec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_trajectory_logger_log_route_integration() {
        let temp_dir = TempDir::new().unwrap();
        let logger = TrajectoryLogger::new(temp_dir.path());

        // Test the logging functionality that would be called in the agent loop
        logger.log_route(
            1,
            "gemini-2.5-flash",
            "standard",
            "test user input for logging",
        );
        logger.flush();

        // Check that the log file was created and contains expected content
        let log_path = temp_dir.path().join(".vtcode/logs/trajectory.jsonl");
        assert!(log_path.exists());

        let content = fs::read_to_string(log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);

        // Parse the JSON and verify content
        let record: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(record["kind"], "route");
        assert_eq!(record["turn"], 1);
        assert_eq!(record["selected_model"], "gemini-2.5-flash");
        assert_eq!(record["class"], "standard");
        assert_eq!(record["input_preview"], "test user input for logging");
        assert!(record["ts"].is_number());
    }
}
