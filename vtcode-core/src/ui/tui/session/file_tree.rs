use std::path::Path;
use tui_tree_widget::TreeItem;

#[derive(Debug, Clone)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Vec<FileTreeNode>,
}

impl FileTreeNode {
    /// Build a tree structure from a flat list of file paths
    pub fn build_tree(files: Vec<String>, workspace: &Path) -> Self {
        let mut root = FileTreeNode {
            name: ".".to_string(),
            path: workspace.to_string_lossy().to_string(),
            is_dir: true,
            children: Vec::new(),
        };

        for file_path in files {
            root.insert_file(&file_path, workspace);
        }

        root.sort_children_recursive();
        root
    }

    /// Insert a file into the tree
    fn insert_file(&mut self, file_path: &str, workspace: &Path) {
        let path = Path::new(file_path);
        let relative = path.strip_prefix(workspace).unwrap_or(path);

        let components: Vec<&str> = relative
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();

        if !components.is_empty() {
            self.insert_components(&components, file_path, workspace);
        }
    }

    /// Recursively insert path components
    fn insert_components(&mut self, components: &[&str], full_path: &str, workspace: &Path) {
        if components.is_empty() {
            return;
        }

        let name = components[0];
        let is_last = components.len() == 1;

        // Find or create child node
        let child = self.children.iter_mut().find(|c| c.name == name);

        if let Some(child) = child {
            if !is_last {
                child.insert_components(&components[1..], full_path, workspace);
            }
        } else {
            // Construct unique path for this node (needed for TreeItem identifier)
            let node_path = if is_last {
                full_path.to_string()
            } else {
                // For directories, construct the path from parent path + name
                let parent_path = &self.path;
                if parent_path.is_empty() || parent_path == &workspace.to_string_lossy().to_string()
                {
                    workspace.join(name).to_string_lossy().to_string()
                } else {
                    Path::new(parent_path)
                        .join(name)
                        .to_string_lossy()
                        .to_string()
                }
            };

            // Create new node
            let mut new_node = FileTreeNode {
                name: name.to_string(),
                path: node_path,
                is_dir: !is_last,
                children: Vec::new(),
            };

            if !is_last {
                new_node.insert_components(&components[1..], full_path, workspace);
            }

            self.children.push(new_node);
        }
    }

    /// Sort children recursively (directories first, then alphabetically)
    fn sort_children_recursive(&mut self) {
        self.children.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        for child in &mut self.children {
            child.sort_children_recursive();
        }
    }

    /// Convert to TreeItem for tui-tree-widget
    pub fn to_tree_items(&self) -> Vec<TreeItem<'static, String>> {
        self.children
            .iter()
            .map(|child| child.to_tree_item())
            .collect()
    }

    fn to_tree_item(&self) -> TreeItem<'static, String> {
        let display_text = if self.is_dir {
            // Tree widget adds its own arrow, just add folder indicator
            format!("{}/", self.name)
        } else {
            self.name.clone()
        };

        if self.is_dir && !self.children.is_empty() {
            let children: Vec<TreeItem<'static, String>> = self
                .children
                .iter()
                .map(|child| child.to_tree_item())
                .collect();
            TreeItem::new(self.path.clone(), display_text, children)
                .expect("Failed to create tree item")
        } else {
            TreeItem::new_leaf(self.path.clone(), display_text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_tree_building() {
        let workspace = PathBuf::from("/workspace");
        let files = vec![
            "/workspace/src/main.rs".to_string(),
            "/workspace/src/lib.rs".to_string(),
            "/workspace/tests/test.rs".to_string(),
            "/workspace/README.md".to_string(),
        ];

        let tree = FileTreeNode::build_tree(files, &workspace);

        assert_eq!(tree.children.len(), 3); // src/, tests/, README.md
        assert!(tree.children[0].is_dir); // src/
        assert!(tree.children[1].is_dir); // tests/
        assert!(!tree.children[2].is_dir); // README.md
    }

    #[test]
    fn test_sorting() {
        let workspace = PathBuf::from("/workspace");
        let files = vec![
            "/workspace/file.txt".to_string(),
            "/workspace/src/main.rs".to_string(),
            "/workspace/another.txt".to_string(),
        ];

        let tree = FileTreeNode::build_tree(files, &workspace);

        // Directories should come first
        assert!(tree.children[0].is_dir);
        assert_eq!(tree.children[0].name, "src");

        // Then files alphabetically
        assert_eq!(tree.children[1].name, "another.txt");
        assert_eq!(tree.children[2].name, "file.txt");
    }

    #[test]
    fn test_nested_directories() {
        let workspace = PathBuf::from("/workspace");
        let files = vec![
            "/workspace/src/agent/mod.rs".to_string(),
            "/workspace/src/agent/runloop.rs".to_string(),
        ];

        let tree = FileTreeNode::build_tree(files, &workspace);

        assert_eq!(tree.children.len(), 1); // src/
        assert_eq!(tree.children[0].children.len(), 1); // agent/
        assert_eq!(tree.children[0].children[0].children.len(), 2); // mod.rs, runloop.rs
    }
}
