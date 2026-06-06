use super::Terminal;

impl Terminal {
    /// Handle Device Status Report (DSR).
    pub(crate) fn device_status_report(&mut self, private: bool, params: &[Option<usize>]) {
        match (private, params.first().copied().flatten().unwrap_or(0)) {
            (false, 5) => {
                // Terminal OK
                self.output.extend_from_slice(b"\x1B[0n");
            }
            (false, 6) => {
                // Cursor position report (1-based)
                let row = self.cursor().row + 1;
                let col = self.cursor().col + 1;
                let report = format!("\x1B[{};{}R", row, col);
                self.output.extend_from_slice(report.as_bytes());
            }
            (true, 996) => {
                // Dark appearance report
                self.output.extend_from_slice(b"\x1B[?997;1n");
            }
            _ => {}
        }
    }

    /// Handle Device Attributes (DA) query.
    pub(crate) fn device_attributes(&mut self, raw_csi: &str) {
        if raw_csi.starts_with('>') {
            // Secondary DA
            self.output.extend_from_slice(b"\x1B[>1;0;0c");
        } else {
            // Primary DA
            self.output.extend_from_slice(b"\x1B[?62;22c");
        }
    }
}
