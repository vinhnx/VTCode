pub(crate) struct ProgressMarker {
    display: String,
}

impl ProgressMarker {
    pub(crate) fn new(current: usize, total: usize) -> Self {
        Self {
            display: format!("[{current}/{total}]"),
        }
    }

    pub(crate) fn annotate(&self, detail: &str) -> String {
        format!("{} {}", self.display, detail)
    }
}
