//! Interactive tree UI using Ratatui for file structure visualization

use std::io::{self, IsTerminal};

use anyhow::{Context, Result, anyhow};
use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap};
use serde_json::Value;

// Store the tree structure separately from the UI state to avoid borrowing conflicts
#[derive(Debug, Clone)]
struct TreeData {
    nodes: Vec<TreeNode>,
}

impl TreeData {
    fn new(nodes: Vec<TreeNode>) -> Self {
        Self { nodes }
    }

    fn toggle_path(&mut self, path_parts: &[String]) -> bool {
        if path_parts.is_empty() {
            return false;
        }
        toggle_node_recursive(&mut self.nodes, path_parts)
    }
}

// Standalone function to avoid self-referencing issues
fn toggle_node_recursive(nodes: &mut Vec<TreeNode>, path_parts: &[String]) -> bool {
    if path_parts.is_empty() {
        return false;
    }

    for node in nodes.iter_mut() {
        if node.name == path_parts[0] {
            if path_parts.len() == 1 {
                // This is the target node
                if node.node_type == "directory" {
                    node.toggle_expanded();
                    return true;
                }
                return false;
            } else {
                // Continue searching in the children
                let remaining_path = &path_parts[1..];
                if node.node_type == "directory" {
                    return toggle_node_recursive(&mut node.children, remaining_path);
                }
                return false;
            }
        }
    }

    false
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub name: String,
    pub node_type: String, // "file" or "directory"
    pub path: String,
    pub children: Vec<TreeNode>,
    pub expanded: bool,
}

impl TreeNode {
    pub fn new(name: String, node_type: String, path: String, children: Vec<TreeNode>) -> Self {
        Self {
            name,
            node_type,
            path,
            children,
            expanded: false,
        }
    }

    pub fn toggle_expanded(&mut self) {
        if self.node_type == "directory" {
            self.expanded = !self.expanded;
        }
    }
}

#[derive(Debug)]
pub struct TreeView {
    title: String,
    instructions: String,
    tree_data: TreeData,
    flattened_items: Vec<(String, usize, String)>, // (display_text, depth, full_path)
    selected_index: usize,
    list_state: ListState,
    number_buffer: String,
}

impl TreeView {
    pub fn new(title: &str, instructions: &str, tree_structure: &Value) -> Result<Self> {
        let root_nodes = Self::parse_tree_structure(tree_structure)?;

        let tree_data = TreeData::new(root_nodes);

        let mut tree_view = Self {
            title: title.to_string(),
            instructions: instructions.to_string(),
            tree_data,
            flattened_items: Vec::new(),
            selected_index: 0,
            list_state: ListState::default(),
            number_buffer: String::new(),
        };

        tree_view.rebuild_flattened_items();
        tree_view.list_state.select(Some(tree_view.selected_index));

        Ok(tree_view)
    }

    fn parse_tree_structure(tree_structure: &Value) -> Result<Vec<TreeNode>> {
        let items = tree_structure
            .as_array()
            .ok_or_else(|| anyhow!("Tree structure is not an array"))?;

        let mut nodes = Vec::new();
        for item in items {
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Node missing 'name' field"))?
                .to_string();

            let node_type = item
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Node missing 'type' field"))?
                .to_string();

            let path = item
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let children = if node_type == "directory" {
                let children_array = item
                    .get("children")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                Self::parse_tree_structure(&Value::Array(children_array))?
            } else {
                Vec::new()
            };

            nodes.push(TreeNode::new(name, node_type, path, children));
        }

        Ok(nodes)
    }

    fn rebuild_flattened_items(&mut self) {
        self.flattened_items.clear();

        // Start with root level nodes
        for node in &self.tree_data.nodes {
            add_node_to_flattened_recursive(node, 0, node.path.clone(), &mut self.flattened_items);
        }
    }

    pub fn toggle_expanded(&mut self) {
        if self.selected_index < self.flattened_items.len() {
            // Find the node to expand/collapse by reconstructing the path
            if let Some(node_path) = &self.flattened_items.get(self.selected_index) {
                let path_parts: Vec<String> =
                    node_path.2.split('/').map(|s| s.to_string()).collect();

                // Find and toggle the node
                if self.tree_data.toggle_path(&path_parts) {
                    self.rebuild_flattened_items(); // Rebuild the flattened representation

                    // Update selection to stay on the same item or closest one
                    self.selected_index = std::cmp::min(
                        self.selected_index,
                        self.flattened_items.len().saturating_sub(1),
                    );
                    self.list_state.select(Some(self.selected_index));
                }
            }
        }
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        } else if !self.flattened_items.is_empty() {
            self.selected_index = self.flattened_items.len() - 1;
        }
        self.list_state.select(Some(self.selected_index));
    }

    pub fn move_down(&mut self) {
        if self.flattened_items.is_empty() {
            return;
        }

        if self.selected_index < self.flattened_items.len() - 1 {
            self.selected_index += 1;
        } else {
            self.selected_index = 0;
        }
        self.list_state.select(Some(self.selected_index));
    }

    pub fn move_to_top(&mut self) {
        self.selected_index = 0;
        self.list_state.select(Some(self.selected_index));
    }

    pub fn move_to_bottom(&mut self) {
        if self.flattened_items.is_empty() {
            return;
        }
        self.selected_index = self.flattened_items.len() - 1;
        self.list_state.select(Some(self.selected_index));
    }

    pub fn handle_number_input(&mut self, digit: char) -> Result<Option<String>> {
        self.number_buffer.push(digit);

        if let Ok(index) = self.number_buffer.parse::<usize>() {
            if index > 0 && index <= self.flattened_items.len() {
                self.selected_index = index - 1;
                self.list_state.select(Some(self.selected_index));

                if self.number_buffer.len() >= self.flattened_items.len().to_string().len() {
                    self.number_buffer.clear();
                }

                if let Some((_, _, path)) = self.flattened_items.get(self.selected_index) {
                    return Ok(Some(path.clone()));
                }
            }
        }

        if self.number_buffer.len() >= self.flattened_items.len().to_string().len() {
            self.number_buffer.clear();
        }

        Ok(None)
    }

    pub fn get_selected_path(&self) -> Option<String> {
        if self.selected_index < self.flattened_items.len() {
            if let Some((_, _, path)) = self.flattened_items.get(self.selected_index) {
                return Some(path.clone());
            }
        }
        None
    }

    pub fn run_interactive_tree(&mut self) -> Result<Option<String>> {
        if self.flattened_items.is_empty() {
            return Err(anyhow!("No items available in tree"));
        }

        if !io::stdout().is_terminal() {
            return Err(anyhow!("Terminal UI is unavailable"));
        }

        let mut stdout = io::stdout();
        let mut terminal_guard = TerminalModeGuard::new(&self.title);
        terminal_guard.enable_raw_mode()?;
        terminal_guard.enter_alternate_screen(&mut stdout)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).with_context(|| {
            format!(
                "Failed to initialize Ratatui terminal for {} tree viewer",
                self.title
            )
        })?;
        terminal_guard.hide_cursor(&mut terminal)?;

        let selection_result = (|| -> Result<Option<String>> {
            loop {
                self.list_state.select(Some(self.selected_index));
                terminal
.draw(|frame| {
let area = frame.area();
let instruction_lines = self.instructions.lines().count().max(1) as u16;
let instruction_height = instruction_lines.saturating_add(2);
let footer_height: u16 = 4;
let layout = Layout::default()
.direction(Direction::Vertical)
.margin(1)
.vertical_margin(1)
.constraints([
Constraint::Length(
instruction_height
.min(area.height.saturating_sub(footer_height + 5)),
),
Constraint::Min(5),
Constraint::Length(footer_height),
])
.split(area);

let instructions_widget = Paragraph::new(self.instructions.as_str())
.block(
Block::default()
.title("Instructions")
.borders(Borders::ALL)
.border_type(BorderType::Rounded),
)
.wrap(Wrap { trim: true });
frame.render_widget(instructions_widget, layout[0]);

let items: Vec<ListItem> = self.flattened_items
.iter()
.enumerate()
.map(|(idx, (text, _, _))| {
let style = if idx == self.selected_index {
Style::default()
.fg(Color::Green)
.add_modifier(Modifier::BOLD.union(Modifier::REVERSED))
} else {
Style::default().fg(Color::White)
};

ListItem::new(vec![Line::from(Span::styled(
format!("{:2}. {}", idx + 1, text),
style,
))])
})
.collect();

let list = List::new(items)
.block(
Block::default()
.title(self.title.as_str())
.borders(Borders::ALL)
.border_type(BorderType::Rounded),
)
.style(Style::default().fg(Color::White))
.highlight_style(
Style::default()
.fg(Color::Green)
.add_modifier(Modifier::BOLD.union(Modifier::REVERSED)),
)
.highlight_symbol("> ")
.repeat_highlight_symbol(true);

frame.render_stateful_widget(list, layout[1], &mut self.list_state);

let selected_item = self.flattened_items
.get(self.selected_index)
.map(|(text, _, _)| text.as_str())
.unwrap_or("");
let current_path = self.get_selected_path().unwrap_or_default();

let summary_lines = vec![
Line::from(Span::styled(
format!("Selected: {}", selected_item),
Style::default().add_modifier(Modifier::BOLD),
)),
Line::from(Span::styled(
format!("Path: {}", current_path),
Style::default().fg(Color::DarkGray),
)),
Line::from(Span::raw("")), // Blank line
Line::from(Span::raw("↑/↓ j/k to move  •  Space to expand/collapse  •  Enter select  •  Esc quit")),
Line::from(Span::styled(
"Tip: Type number to jump",
Style::default().fg(Color::DarkGray),
)),
];

let footer = Paragraph::new(summary_lines)
.block(
Block::default()
.title("Selection")
.borders(Borders::ALL)
.border_type(BorderType::Rounded),
)
.wrap(Wrap { trim: true });
frame.render_widget(footer, layout[2]);
})
.with_context(|| format!("Failed to draw {} tree viewer UI", self.title))?;

                match event::read().with_context(|| {
                    format!(
                        "Failed to read terminal input for {} tree viewer",
                        self.title
                    )
                })? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.move_up();
                            self.number_buffer.clear();
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.move_down();
                            self.number_buffer.clear();
                        }
                        KeyCode::Home => {
                            self.move_to_top();
                            self.number_buffer.clear();
                        }
                        KeyCode::End => {
                            self.move_to_bottom();
                            self.number_buffer.clear();
                        }
                        KeyCode::Char(' ') => {
                            self.toggle_expanded();
                            self.number_buffer.clear();
                        }
                        KeyCode::Enter => {
                            return Ok(self.get_selected_path());
                        }
                        KeyCode::Esc => return Ok(None),
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            return Err(SelectionInterrupted.into());
                        }
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            if let Some(path) = self.handle_number_input(c)? {
                                return Ok(Some(path));
                            }
                        }
                        KeyCode::Backspace => {
                            self.number_buffer.pop();
                        }
                        _ => {}
                    },
                    Event::Resize(_, _) => {
                        self.number_buffer.clear();
                    }
                    _ => {}
                }
            }
        })();

        let cleanup_result = terminal_guard.restore_with_terminal(&mut terminal);
        cleanup_result?;
        selection_result
    }
}

// Standalone function to build flattened items to avoid borrowing issues
fn add_node_to_flattened_recursive(
    node: &TreeNode,
    depth: usize,
    parent_path: String,
    flattened_items: &mut Vec<(String, usize, String)>,
) {
    let indent = "  ".repeat(depth);
    let icon = if node.node_type == "directory" {
        if node.expanded { "[+]" } else { "[-]" }
    } else {
        "[F]"
    };

    let display_text = format!("{}{} {}", indent, icon, node.name);
    let full_path = node.path.clone();

    flattened_items.push((display_text, depth, full_path));

    if node.node_type == "directory" && node.expanded {
        for child in &node.children {
            add_node_to_flattened_recursive(
                child,
                depth + 1,
                format!("{}/{}", parent_path, child.name),
                flattened_items,
            );
        }
    }
}

#[derive(Debug)]
pub struct SelectionInterrupted;

impl std::fmt::Display for SelectionInterrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("selection interrupted by Ctrl+C")
    }
}

impl std::error::Error for SelectionInterrupted {}

struct TerminalModeGuard {
    label: String,
    raw_mode_enabled: bool,
    alternate_screen: bool,
    cursor_hidden: bool,
}

impl TerminalModeGuard {
    fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            raw_mode_enabled: false,
            alternate_screen: false,
            cursor_hidden: false,
        }
    }

    fn enable_raw_mode(&mut self) -> Result<()> {
        enable_raw_mode()
            .with_context(|| format!("Failed to enable raw mode for {} tree viewer", self.label))?;
        self.raw_mode_enabled = true;
        Ok(())
    }

    fn enter_alternate_screen(&mut self, stdout: &mut io::Stdout) -> Result<()> {
        execute!(stdout, EnterAlternateScreen).with_context(|| {
            format!(
                "Failed to enter alternate screen for {} tree viewer",
                self.label
            )
        })?;
        self.alternate_screen = true;
        Ok(())
    }

    fn hide_cursor(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        terminal
            .hide_cursor()
            .with_context(|| format!("Failed to hide cursor for {} tree viewer", self.label))?;
        self.cursor_hidden = true;
        Ok(())
    }

    fn restore_with_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        if self.raw_mode_enabled {
            disable_raw_mode().with_context(|| {
                format!(
                    "Failed to disable raw mode after {} tree viewer",
                    self.label
                )
            })?;
            self.raw_mode_enabled = false;
        }

        if self.alternate_screen {
            execute!(terminal.backend_mut(), LeaveAlternateScreen).with_context(|| {
                format!(
                    "Failed to leave alternate screen after {} tree viewer",
                    self.label
                )
            })?;
            self.alternate_screen = false;
        }

        if self.cursor_hidden {
            terminal.show_cursor().with_context(|| {
                format!("Failed to show cursor after {} tree viewer", self.label)
            })?;
            self.cursor_hidden = false;
        }

        Ok(())
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        if self.raw_mode_enabled {
            let _ = disable_raw_mode();
            self.raw_mode_enabled = false;
        }

        if self.alternate_screen {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, LeaveAlternateScreen);
            self.alternate_screen = false;
        }

        if self.cursor_hidden {
            let mut stdout = io::stdout();
            let _ = execute!(stdout, Show);
            self.cursor_hidden = false;
        }
    }
}
