use assert_fs::TempDir;
use assert_fs::prelude::*;
use vtcode_core::tools::tree_sitter::analyzer::Position;
use vtcode_core::tools::tree_sitter::refactoring::{
    CodeChange, RefactoringEngine, RefactoringKind, RefactoringOperation, TextRange,
};

fn make_range(offset: usize, len: usize) -> TextRange {
    TextRange {
        start: Position {
            row: 0,
            column: offset,
            byte_offset: offset,
        },
        end: Position {
            row: 0,
            column: offset + len,
            byte_offset: offset + len,
        },
    }
}

#[test]
fn rename_conflict_detected() {
    let dir = TempDir::new().unwrap();
    let file = dir.child("conflict.rs");
    let content = "let x = 1;\nlet y = 2;\nprintln!(\"{}\", x);\n";
    file.write_str(content).unwrap();
    let start = content.find("x = 1").unwrap();
    let range = make_range(start, 1);
    let op = RefactoringOperation {
        kind: RefactoringKind::Rename,
        description: "rename x to y".to_string(),
        changes: vec![CodeChange {
            file_path: file.path().to_string_lossy().into(),
            old_range: range,
            new_text: "y".to_string(),
            description: String::new(),
        }],
        preview: vec![],
    };
    let mut engine = RefactoringEngine::new();
    let result = engine.apply_refactoring(&op).unwrap();
    assert!(!result.success);
    assert!(!result.conflicts.is_empty());
}

#[test]
fn rename_applies_change() {
    let dir = TempDir::new().unwrap();
    let file = dir.child("rename.rs");
    let content = "let x = 1;\nprintln!(\"{}\", x);\n";
    file.write_str(content).unwrap();
    let start = content.find("x = 1").unwrap();
    let range = make_range(start, 1);
    let op = RefactoringOperation {
        kind: RefactoringKind::Rename,
        description: "rename x to z".to_string(),
        changes: vec![CodeChange {
            file_path: file.path().to_string_lossy().into(),
            old_range: range,
            new_text: "z".to_string(),
            description: String::new(),
        }],
        preview: vec![],
    };
    let mut engine = RefactoringEngine::new();
    let result = engine.apply_refactoring(&op).unwrap();
    assert!(result.success);
    let updated = file.read_string().unwrap();
    assert!(updated.contains("let z = 1"));
}
