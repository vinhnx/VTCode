use std::collections::HashMap;
/// Workspace Context Management
///
/// Provides rich workspace structure and file context for VT Code commands.
/// Handles workspace analysis, file discovery, and content context passing.
use std::path::PathBuf;

/// Workspace structure and metadata
#[derive(Debug, Clone)]
pub struct WorkspaceContext {
    /// Root directory of the workspace
    pub root: PathBuf,
    /// List of source files in workspace
    pub files: Vec<WorkspaceFile>,
    /// Project structure (directories and files)
    pub structure: ProjectStructure,
    /// Configuration files found in workspace
    pub config_files: Vec<PathBuf>,
    /// Language distribution
    pub languages: HashMap<String, usize>,
}

/// Information about a single file in the workspace
#[derive(Debug, Clone)]
pub struct WorkspaceFile {
    /// Absolute path to the file
    pub path: PathBuf,
    /// Relative path from workspace root
    pub relative_path: PathBuf,
    /// File language/extension
    pub language: String,
    /// File size in bytes
    pub size: usize,
    /// Whether the file is binary
    pub is_binary: bool,
    /// Number of lines (for text files)
    pub line_count: Option<usize>,
}

/// Project structure representation
#[derive(Debug, Clone)]
pub struct ProjectStructure {
    /// Root node
    pub root: DirectoryNode,
    /// Total file count
    pub total_files: usize,
    /// Total directory count
    pub total_directories: usize,
}

/// Directory or file node in project structure
#[derive(Debug, Clone)]
pub struct DirectoryNode {
    /// Name of this directory
    pub name: String,
    /// Absolute path
    pub path: PathBuf,
    /// Child nodes
    pub children: Vec<DirectoryNode>,
    /// Files in this directory
    pub files: Vec<String>,
}

impl WorkspaceContext {
    /// Create a new workspace context for the given root
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            files: Vec::new(),
            structure: ProjectStructure::new(String::from("root"), PathBuf::new()),
            config_files: Vec::new(),
            languages: HashMap::new(),
        }
    }

    /// Add a file to the workspace context
    pub fn add_file(&mut self, file: WorkspaceFile) {
        // Update language distribution
        let lang = file.language.clone();
        *self.languages.entry(lang).or_insert(0) += 1;

        self.files.push(file);
    }

    /// Add a configuration file
    pub fn add_config_file(&mut self, path: PathBuf) {
        self.config_files.push(path);
    }

    /// Get files by language
    pub fn files_by_language(&self, language: &str) -> Vec<&WorkspaceFile> {
        self.files
            .iter()
            .filter(|f| f.language.eq_ignore_ascii_case(language))
            .collect()
    }

    /// Get all text files (non-binary)
    pub fn text_files(&self) -> Vec<&WorkspaceFile> {
        self.files.iter().filter(|f| !f.is_binary).collect()
    }

    /// Count files by language
    pub fn file_count(&self, language: &str) -> usize {
        self.languages.get(language).copied().unwrap_or(0)
    }

    /// Get the primary language (most files)
    pub fn primary_language(&self) -> Option<String> {
        self.languages
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(lang, _)| lang.clone())
    }

    /// Build a summary of the workspace
    pub fn summary(&self) -> String {
        let total_files = self.files.len();
        let primary = self
            .primary_language()
            .unwrap_or_else(|| "unknown".to_string());
        let config_count = self.config_files.len();

        let lang_summary = self
            .languages
            .iter()
            .map(|(lang, count)| format!("{}: {}", lang, count))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "Workspace: {} files (primary: {}), {} config files\nLanguages: {}",
            total_files, primary, config_count, lang_summary
        )
    }
}

impl ProjectStructure {
    /// Create a new project structure
    pub fn new(name: String, path: PathBuf) -> Self {
        Self {
            root: DirectoryNode {
                name,
                path,
                children: Vec::new(),
                files: Vec::new(),
            },
            total_files: 0,
            total_directories: 1,
        }
    }

    /// Add a directory to the structure
    pub fn add_directory(&mut self, _path: PathBuf, _name: String) {
        self.total_directories += 1;
        // In a real implementation, this would traverse the tree and add the directory
    }

    /// Add a file to a specific directory
    pub fn add_file(&mut self, _path: PathBuf, filename: String) {
        self.total_files += 1;
        // In a real implementation, this would traverse the tree and add the file
        self.root.files.push(filename);
    }

    /// Get the depth of the structure
    pub fn depth(&self) -> usize {
        self._calculate_depth(&self.root)
    }

    #[allow(clippy::only_used_in_recursion)]
    fn _calculate_depth(&self, node: &DirectoryNode) -> usize {
        if node.children.is_empty() {
            0
        } else {
            1 + node
                .children
                .iter()
                .map(|child| self._calculate_depth(child))
                .max()
                .unwrap_or(0)
        }
    }
}

impl WorkspaceFile {
    /// Create a new workspace file entry
    pub fn new(
        path: PathBuf,
        relative_path: PathBuf,
        language: String,
        size: usize,
        is_binary: bool,
    ) -> Self {
        Self {
            path,
            relative_path,
            language,
            size,
            is_binary,
            line_count: None,
        }
    }

    /// Set the line count for text files
    pub fn with_line_count(mut self, count: usize) -> Self {
        if !self.is_binary {
            self.line_count = Some(count);
        }
        self
    }

    /// Get a display string for the file
    pub fn display_string(&self) -> String {
        let line_info = self
            .line_count
            .map(|c| format!(", {} lines", c))
            .unwrap_or_default();

        format!(
            "{} ({} bytes{})",
            self.relative_path.display(),
            self.size,
            line_info
        )
    }
}

impl DirectoryNode {
    /// Create a new directory node
    pub fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            children: Vec::new(),
            files: Vec::new(),
        }
    }

    /// Add a child directory
    pub fn add_child(&mut self, child: DirectoryNode) {
        self.children.push(child);
    }

    /// Add a file to this directory
    pub fn add_file(&mut self, filename: String) {
        self.files.push(filename);
    }

    /// Get total number of files in this subtree
    pub fn total_files(&self) -> usize {
        let mut count = self.files.len();
        for child in &self.children {
            count += child.total_files();
        }
        count
    }
}

/// Context about file content and selection
#[derive(Debug, Clone)]
pub struct FileContentContext {
    /// File path
    pub file: PathBuf,
    /// File content (limited to avoid memory issues)
    pub content: Option<String>,
    /// Maximum content size to capture (in bytes)
    pub max_size: usize,
    /// Selection range if any
    pub selection_range: Option<(usize, usize)>,
    /// Selected text
    pub selected_text: Option<String>,
}

impl FileContentContext {
    /// Create a new file content context
    pub fn new(file: PathBuf) -> Self {
        Self {
            file,
            content: None,
            max_size: 1024 * 1024, // 1MB default limit
            selection_range: None,
            selected_text: None,
        }
    }

    /// Set the file content
    pub fn with_content(mut self, content: String) -> Self {
        // Truncate if too large
        if content.len() > self.max_size {
            self.content = Some(content[..self.max_size].to_string());
        } else {
            self.content = Some(content);
        }
        self
    }

    /// Set the maximum content size
    pub fn with_max_size(mut self, size: usize) -> Self {
        self.max_size = size;
        self
    }

    /// Set selection info
    pub fn with_selection(mut self, start: usize, end: usize, text: String) -> Self {
        self.selection_range = Some((start, end));
        self.selected_text = Some(text);
        self
    }

    /// Get a preview of the file content
    pub fn preview(&self, lines: usize) -> Option<String> {
        self.content
            .as_ref()
            .map(|c| c.lines().take(lines).collect::<Vec<_>>().join("\n"))
    }

    /// Get the total number of lines in the file
    pub fn line_count(&self) -> usize {
        self.content
            .as_ref()
            .map(|c| c.lines().count())
            .unwrap_or(0)
    }

    /// Get the size in bytes
    pub fn size(&self) -> usize {
        self.content.as_ref().map(|c| c.len()).unwrap_or(0)
    }
}

/// Open buffers context (currently open files in editor)
#[derive(Debug, Clone)]
pub struct OpenBuffersContext {
    /// List of open files
    pub buffers: Vec<OpenBuffer>,
    /// Index of the active buffer
    pub active_index: Option<usize>,
}

/// Information about an open buffer
#[derive(Debug, Clone)]
pub struct OpenBuffer {
    /// File path
    pub path: PathBuf,
    /// Language
    pub language: String,
    /// Unsaved changes
    pub is_dirty: bool,
    /// Current cursor position
    pub cursor_position: Option<(usize, usize)>,
}

impl OpenBuffersContext {
    /// Create a new open buffers context
    pub fn new() -> Self {
        Self {
            buffers: Vec::new(),
            active_index: None,
        }
    }

    /// Add an open buffer
    pub fn add_buffer(&mut self, buffer: OpenBuffer) {
        self.buffers.push(buffer);
    }

    /// Set the active buffer index
    pub fn set_active(&mut self, index: usize) {
        if index < self.buffers.len() {
            self.active_index = Some(index);
        }
    }

    /// Get the active buffer
    pub fn active_buffer(&self) -> Option<&OpenBuffer> {
        self.active_index.and_then(|i| self.buffers.get(i))
    }

    /// Get buffers with unsaved changes
    pub fn dirty_buffers(&self) -> Vec<&OpenBuffer> {
        self.buffers.iter().filter(|b| b.is_dirty).collect()
    }

    /// Count total open buffers
    pub fn count(&self) -> usize {
        self.buffers.len()
    }

    /// Get languages used in open buffers
    pub fn languages(&self) -> Vec<String> {
        let mut langs: Vec<String> = self.buffers.iter().map(|b| &b.language).cloned().collect();
        langs.sort();
        langs.dedup();
        langs
    }
}

impl Default for OpenBuffersContext {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenBuffer {
    /// Create a new open buffer
    pub fn new(path: PathBuf, language: String) -> Self {
        Self {
            path,
            language,
            is_dirty: false,
            cursor_position: None,
        }
    }

    /// Mark the buffer as dirty
    pub fn with_dirty(mut self, dirty: bool) -> Self {
        self.is_dirty = dirty;
        self
    }

    /// Set cursor position
    pub fn with_cursor(mut self, line: usize, column: usize) -> Self {
        self.cursor_position = Some((line, column));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_context_creation() {
        let ctx = WorkspaceContext::new(PathBuf::from("/workspace"));
        assert_eq!(ctx.files.len(), 0);
        assert_eq!(ctx.config_files.len(), 0);
    }

    #[test]
    fn test_add_file_to_workspace() {
        let mut ctx = WorkspaceContext::new(PathBuf::from("/workspace"));
        let file = WorkspaceFile::new(
            PathBuf::from("/workspace/main.rs"),
            PathBuf::from("main.rs"),
            "rs".to_string(),
            1024,
            false,
        );
        ctx.add_file(file);

        assert_eq!(ctx.files.len(), 1);
        assert_eq!(ctx.file_count("rs"), 1);
    }

    #[test]
    fn test_language_distribution() {
        let mut ctx = WorkspaceContext::new(PathBuf::from("/workspace"));
        ctx.add_file(WorkspaceFile::new(
            PathBuf::from("/workspace/main.rs"),
            PathBuf::from("main.rs"),
            "rs".to_string(),
            1024,
            false,
        ));
        ctx.add_file(WorkspaceFile::new(
            PathBuf::from("/workspace/lib.rs"),
            PathBuf::from("lib.rs"),
            "rs".to_string(),
            2048,
            false,
        ));
        ctx.add_file(WorkspaceFile::new(
            PathBuf::from("/workspace/main.py"),
            PathBuf::from("main.py"),
            "py".to_string(),
            512,
            false,
        ));

        assert_eq!(ctx.file_count("rs"), 2);
        assert_eq!(ctx.file_count("py"), 1);
        assert_eq!(ctx.primary_language(), Some("rs".to_string()));
    }

    #[test]
    fn test_files_by_language() {
        let mut ctx = WorkspaceContext::new(PathBuf::from("/workspace"));
        ctx.add_file(WorkspaceFile::new(
            PathBuf::from("/workspace/main.rs"),
            PathBuf::from("main.rs"),
            "rs".to_string(),
            1024,
            false,
        ));
        ctx.add_file(WorkspaceFile::new(
            PathBuf::from("/workspace/main.py"),
            PathBuf::from("main.py"),
            "py".to_string(),
            512,
            false,
        ));

        let rust_files = ctx.files_by_language("rs");
        assert_eq!(rust_files.len(), 1);
    }

    #[test]
    fn test_text_files_filter() {
        let mut ctx = WorkspaceContext::new(PathBuf::from("/workspace"));
        ctx.add_file(WorkspaceFile::new(
            PathBuf::from("/workspace/main.rs"),
            PathBuf::from("main.rs"),
            "rs".to_string(),
            1024,
            false,
        ));
        ctx.add_file(WorkspaceFile::new(
            PathBuf::from("/workspace/image.png"),
            PathBuf::from("image.png"),
            "png".to_string(),
            10240,
            true,
        ));

        let text_files = ctx.text_files();
        assert_eq!(text_files.len(), 1);
    }

    #[test]
    fn test_workspace_summary() {
        let mut ctx = WorkspaceContext::new(PathBuf::from("/workspace"));
        ctx.add_file(WorkspaceFile::new(
            PathBuf::from("/workspace/main.rs"),
            PathBuf::from("main.rs"),
            "rs".to_string(),
            1024,
            false,
        ));
        ctx.add_config_file(PathBuf::from("/workspace/Cargo.toml"));

        let summary = ctx.summary();
        assert!(summary.contains("1 files"));
        assert!(summary.contains("1 config files"));
    }

    #[test]
    fn test_workspace_file_creation() {
        let file = WorkspaceFile::new(
            PathBuf::from("/workspace/main.rs"),
            PathBuf::from("main.rs"),
            "rs".to_string(),
            1024,
            false,
        );

        assert_eq!(file.language, "rs");
        assert_eq!(file.size, 1024);
        assert!(!file.is_binary);
    }

    #[test]
    fn test_workspace_file_with_line_count() {
        let file = WorkspaceFile::new(
            PathBuf::from("/workspace/main.rs"),
            PathBuf::from("main.rs"),
            "rs".to_string(),
            1024,
            false,
        )
        .with_line_count(42);

        assert_eq!(file.line_count, Some(42));
    }

    #[test]
    fn test_file_content_context_creation() {
        let ctx = FileContentContext::new(PathBuf::from("/workspace/main.rs"));
        assert!(ctx.content.is_none());
        assert_eq!(ctx.max_size, 1024 * 1024);
    }

    #[test]
    fn test_file_content_with_content() {
        let ctx = FileContentContext::new(PathBuf::from("/workspace/main.rs"))
            .with_content("fn main() {}".to_string());

        assert!(ctx.content.is_some());
        assert_eq!(ctx.size(), 12);
    }

    #[test]
    fn test_file_content_truncation() {
        let content = "x".repeat(2000);
        let ctx = FileContentContext::new(PathBuf::from("/workspace/main.rs"))
            .with_max_size(1000)
            .with_content(content);

        assert_eq!(ctx.size(), 1000);
    }

    #[test]
    fn test_file_content_preview() {
        let content = "line1\nline2\nline3\n".to_string();
        let ctx =
            FileContentContext::new(PathBuf::from("/workspace/main.rs")).with_content(content);

        let preview = ctx.preview(2);
        assert!(preview.is_some());
        assert_eq!(preview.unwrap().lines().count(), 2);
    }

    #[test]
    fn test_file_content_line_count() {
        let content = "line1\nline2\nline3\n".to_string();
        let ctx =
            FileContentContext::new(PathBuf::from("/workspace/main.rs")).with_content(content);

        assert_eq!(ctx.line_count(), 3);
    }

    #[test]
    fn test_open_buffers_context_creation() {
        let ctx = OpenBuffersContext::new();
        assert_eq!(ctx.count(), 0);
        assert!(ctx.active_buffer().is_none());
    }

    #[test]
    fn test_add_buffer_to_context() {
        let mut ctx = OpenBuffersContext::new();
        let buffer = OpenBuffer::new(PathBuf::from("/workspace/main.rs"), "rs".to_string());
        ctx.add_buffer(buffer);

        assert_eq!(ctx.count(), 1);
    }

    #[test]
    fn test_set_active_buffer() {
        let mut ctx = OpenBuffersContext::new();
        ctx.add_buffer(OpenBuffer::new(
            PathBuf::from("/workspace/main.rs"),
            "rs".to_string(),
        ));
        ctx.set_active(0);

        assert!(ctx.active_buffer().is_some());
        assert_eq!(ctx.active_buffer().unwrap().language, "rs");
    }

    #[test]
    fn test_dirty_buffers() {
        let mut ctx = OpenBuffersContext::new();
        ctx.add_buffer(
            OpenBuffer::new(PathBuf::from("/workspace/main.rs"), "rs".to_string()).with_dirty(true),
        );
        ctx.add_buffer(OpenBuffer::new(
            PathBuf::from("/workspace/lib.rs"),
            "rs".to_string(),
        ));

        assert_eq!(ctx.dirty_buffers().len(), 1);
    }

    #[test]
    fn test_open_buffers_languages() {
        let mut ctx = OpenBuffersContext::new();
        ctx.add_buffer(OpenBuffer::new(
            PathBuf::from("/workspace/main.rs"),
            "rs".to_string(),
        ));
        ctx.add_buffer(OpenBuffer::new(
            PathBuf::from("/workspace/main.py"),
            "py".to_string(),
        ));
        ctx.add_buffer(OpenBuffer::new(
            PathBuf::from("/workspace/lib.rs"),
            "rs".to_string(),
        ));

        let langs = ctx.languages();
        assert_eq!(langs.len(), 2);
        assert!(langs.contains(&"rs".to_string()));
        assert!(langs.contains(&"py".to_string()));
    }

    #[test]
    fn test_project_structure_creation() {
        let structure = ProjectStructure::new("root".to_string(), PathBuf::from("/"));
        assert_eq!(structure.total_files, 0);
        assert_eq!(structure.total_directories, 1);
    }

    #[test]
    fn test_directory_node_add_child() {
        let mut root = DirectoryNode::new("root".to_string(), PathBuf::from("/"));
        let child = DirectoryNode::new("src".to_string(), PathBuf::from("/src"));
        root.add_child(child);

        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn test_directory_node_total_files() {
        let mut root = DirectoryNode::new("root".to_string(), PathBuf::from("/"));
        root.add_file("file1.rs".to_string());
        root.add_file("file2.rs".to_string());

        assert_eq!(root.total_files(), 2);
    }
}
