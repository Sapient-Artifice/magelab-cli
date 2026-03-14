use crossterm::cursor::MoveUp;
use crossterm::execute;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use std::io::{self, Write};
use std::time::Instant;

const SPINNER_FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum NodeStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: String,
    pub label: String,
    pub status: NodeStatus,
    pub detail: Option<String>,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            status: NodeStatus::Running,
            detail: None,
            children: Vec::new(),
        }
    }

    fn status_icon(&self, frame: usize) -> (char, Color) {
        match self.status {
            NodeStatus::Pending => ('○', Color::DarkGrey),
            NodeStatus::Running => (SPINNER_FRAMES[frame % SPINNER_FRAMES.len()], Color::Cyan),
            NodeStatus::Completed => ('✓', Color::Green),
            NodeStatus::Failed => ('✗', Color::Red),
            NodeStatus::Cancelled => ('⊘', Color::DarkGrey),
        }
    }

    /// Find a mutable reference to a node by id (depth-first)
    fn find_mut(&mut self, id: &str) -> Option<&mut TreeNode> {
        if self.id == id {
            return Some(self);
        }
        for child in &mut self.children {
            if let Some(found) = child.find_mut(id) {
                return Some(found);
            }
        }
        None
    }

    /// Render this node and children to lines
    fn render_lines(&self, prefix: &str, is_last: bool, frame: usize) -> Vec<RenderedLine> {
        let mut lines = Vec::new();
        let connector = if is_last { "└─" } else { "├─" };
        let (icon, color) = self.status_icon(frame);

        lines.push(RenderedLine {
            text: format!("{}{} {} {}", prefix, connector, icon, self.label),
            icon_color: color,
            // Position of the icon character for coloring
            icon_offset: prefix.len() + connector.len() + 1,
        });

        // Show detail line for completed/failed nodes
        if let Some(ref detail) = self.detail {
            if self.status != NodeStatus::Running {
                let child_prefix = if is_last {
                    format!("{}   ", prefix)
                } else {
                    format!("{}│  ", prefix)
                };
                let preview = if detail.len() > 60 {
                    format!("{}...", &detail[..57])
                } else {
                    detail.clone()
                };
                lines.push(RenderedLine {
                    text: format!("{}└─ {}", child_prefix, preview),
                    icon_color: Color::DarkGrey,
                    icon_offset: 0, // no icon
                });
            }
        }

        // Render children
        let child_prefix = if is_last {
            format!("{}   ", prefix)
        } else {
            format!("{}│  ", prefix)
        };
        let child_count = self.children.len();
        for (i, child) in self.children.iter().enumerate() {
            let child_is_last = i == child_count - 1;
            lines.extend(child.render_lines(&child_prefix, child_is_last, frame));
        }

        lines
    }
}

struct RenderedLine {
    text: String,
    icon_color: Color,
    icon_offset: usize,
}

pub struct TreeRenderer {
    roots: Vec<TreeNode>,
    rendered_lines: usize,
    start: Instant,
}

impl Default for TreeRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeRenderer {
    pub fn new() -> Self {
        Self {
            roots: Vec::new(),
            rendered_lines: 0,
            start: Instant::now(),
        }
    }

    /// Check if the tree has any nodes
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Check if a node with the given id exists anywhere in the tree
    pub fn contains(&self, id: &str) -> bool {
        fn search(nodes: &[TreeNode], id: &str) -> bool {
            nodes.iter().any(|n| n.id == id || search(&n.children, id))
        }
        search(&self.roots, id)
    }

    /// Push a new root-level node
    pub fn push(&mut self, id: &str, label: &str) {
        self.roots.push(TreeNode::new(id, label));
    }

    /// Push a child node under a parent (found by id)
    #[allow(dead_code)]
    pub fn push_child(&mut self, parent_id: &str, id: &str, label: &str) {
        for root in &mut self.roots {
            if let Some(parent) = root.find_mut(parent_id) {
                parent.children.push(TreeNode::new(id, label));
                return;
            }
        }
        // Parent not found — add as root
        self.roots.push(TreeNode::new(id, label));
    }

    /// Update a node's status
    pub fn update_status(&mut self, id: &str, status: NodeStatus) {
        for root in &mut self.roots {
            if let Some(node) = root.find_mut(id) {
                node.status = status;
                return;
            }
        }
    }

    /// Complete a node with optional result preview
    pub fn complete(&mut self, id: &str, preview: Option<&str>) {
        for root in &mut self.roots {
            if let Some(node) = root.find_mut(id) {
                node.status = NodeStatus::Completed;
                node.detail = preview.map(String::from);
                return;
            }
        }
    }

    /// Fail a node with error message
    pub fn fail(&mut self, id: &str, error: &str) {
        for root in &mut self.roots {
            if let Some(node) = root.find_mut(id) {
                node.status = NodeStatus::Failed;
                node.detail = Some(error.to_string());
                return;
            }
        }
    }

    /// Get the current spinner frame based on elapsed time
    fn frame(&self) -> usize {
        (self.start.elapsed().as_millis() / 80) as usize
    }

    /// Clear previous render and redraw the tree
    pub fn render(&mut self) {
        let mut stdout = io::stdout();

        // Clear previous output
        if self.rendered_lines > 0 {
            for _ in 0..self.rendered_lines {
                execute!(stdout, MoveUp(1), Clear(ClearType::CurrentLine)).ok();
            }
        }

        if self.roots.is_empty() {
            self.rendered_lines = 0;
            return;
        }

        let frame = self.frame();
        let root_count = self.roots.len();
        let mut all_lines = Vec::new();

        for (i, root) in self.roots.iter().enumerate() {
            let is_last = i == root_count - 1;
            all_lines.extend(root.render_lines("", is_last, frame));
        }

        for line in &all_lines {
            if line.icon_offset > 0 && line.icon_offset < line.text.len() {
                // Split around the icon to color it
                let before = &line.text[..line.icon_offset];
                let icon_char = &line.text[line.icon_offset..].chars().next().unwrap();
                let after_start = line.icon_offset + icon_char.len_utf8();
                let after = if after_start < line.text.len() {
                    &line.text[after_start..]
                } else {
                    ""
                };
                execute!(
                    stdout,
                    SetForegroundColor(Color::DarkGrey),
                    Print(before),
                    SetForegroundColor(line.icon_color),
                    Print(format!("{}", icon_char)),
                    ResetColor,
                    Print(format!("{}\n", after)),
                )
                .ok();
            } else {
                execute!(
                    stdout,
                    SetForegroundColor(Color::DarkGrey),
                    Print(format!("{}\n", line.text)),
                    ResetColor,
                )
                .ok();
            }
        }

        stdout.flush().ok();
        self.rendered_lines = all_lines.len();
    }

    /// Clear the tree from terminal completely
    pub fn clear(&mut self) {
        let mut stdout = io::stdout();
        if self.rendered_lines > 0 {
            for _ in 0..self.rendered_lines {
                execute!(stdout, MoveUp(1), Clear(ClearType::CurrentLine)).ok();
            }
            self.rendered_lines = 0;
        }
    }

    /// Check if any node is still running
    #[allow(dead_code)]
    pub fn has_running(&self) -> bool {
        fn check(nodes: &[TreeNode]) -> bool {
            nodes.iter().any(|n| {
                n.status == NodeStatus::Running
                    || n.status == NodeStatus::Pending
                    || check(&n.children)
            })
        }
        check(&self.roots)
    }
}

#[cfg(test)]
/// Format a tree to a string (for testing, no colors)
pub fn render_to_string(roots: &[TreeNode]) -> String {
    let mut lines = Vec::new();
    let root_count = roots.len();
    for (i, root) in roots.iter().enumerate() {
        let is_last = i == root_count - 1;
        for line in root.render_lines("", is_last, 0) {
            lines.push(line.text);
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_node() {
        let mut tree = TreeRenderer::new();
        tree.push("t1", "bash_commands: ls -la");
        tree.complete("t1", Some("12 files"));
        let output = render_to_string(&tree.roots);
        assert!(output.contains("└─ ✓ bash_commands: ls -la"));
        assert!(output.contains("12 files"));
    }

    #[test]
    fn test_sibling_nodes() {
        let mut tree = TreeRenderer::new();
        tree.push("t1", "read_file: main.rs");
        tree.complete("t1", Some("245 lines"));
        tree.push("t2", "write_file: config.rs");
        tree.complete("t2", None);
        let output = render_to_string(&tree.roots);
        assert!(output.contains("├─ ✓ read_file: main.rs"));
        assert!(output.contains("└─ ✓ write_file: config.rs"));
    }

    #[test]
    fn test_nested_children() {
        let mut tree = TreeRenderer::new();
        tree.push("sub1", "Research: find auth examples");
        tree.push_child("sub1", "t1", "BraveSearch: rust oauth2");
        tree.complete("t1", Some("5 results"));
        tree.push_child("sub1", "t2", "read_file: examples/oauth.rs");
        tree.complete("t2", Some("89 lines"));
        tree.complete("sub1", None);
        let output = render_to_string(&tree.roots);
        assert!(output.contains("✓ Research: find auth examples"));
        assert!(output.contains("├─ ✓ BraveSearch: rust oauth2"));
        assert!(output.contains("└─ ✓ read_file: examples/oauth.rs"));
    }

    #[test]
    fn test_failed_node() {
        let mut tree = TreeRenderer::new();
        tree.push("t1", "bash_commands: rm -rf /");
        tree.fail("t1", "Permission denied");
        let output = render_to_string(&tree.roots);
        assert!(output.contains("✗ bash_commands"));
        assert!(output.contains("Permission denied"));
    }

    #[test]
    fn test_running_node_has_spinner() {
        let mut tree = TreeRenderer::new();
        tree.push("t1", "Thinking...");
        let output = render_to_string(&tree.roots);
        // Frame 0 = ⠋
        assert!(output.contains("⠋ Thinking..."));
    }

    #[test]
    fn test_has_running() {
        let mut tree = TreeRenderer::new();
        tree.push("t1", "task");
        assert!(tree.has_running());
        tree.complete("t1", None);
        assert!(!tree.has_running());
    }
}
