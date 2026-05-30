use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vtcode_vim::{Editor, VimState, handle_key};

struct BenchEditor {
    content: String,
    cursor: usize,
}

impl BenchEditor {
    fn new(content: &str, cursor: usize) -> Self {
        Self {
            content: content.to_string(),
            cursor,
        }
    }
}

impl Editor for BenchEditor {
    fn content(&self) -> &str {
        &self.content
    }

    fn cursor(&self) -> usize {
        self.cursor
    }

    fn set_cursor(&mut self, pos: usize) {
        self.cursor = pos.min(self.content.len());
    }

    fn move_left(&mut self) {
        self.cursor = vtcode_vim::prev_char_boundary(&self.content, self.cursor);
    }

    fn move_right(&mut self) {
        self.cursor = vtcode_vim::next_char_boundary(&self.content, self.cursor);
    }

    fn delete_char_forward(&mut self) {
        if self.cursor >= self.content.len() {
            return;
        }
        let end = vtcode_vim::next_char_boundary(&self.content, self.cursor);
        self.content.drain(self.cursor..end);
    }

    fn insert_text(&mut self, text: &str) {
        self.content.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    fn replace(&mut self, content: String, cursor: usize) {
        self.content = content;
        self.cursor = cursor.min(self.content.len());
    }

    fn replace_range(&mut self, start: usize, end: usize, text: &str) {
        self.content.replace_range(start..end, text);
        self.cursor = (start + text.len()).min(self.content.len());
    }
}

fn make_multiline_buffer(line_count: usize) -> String {
    let mut s = String::new();
    for i in 0..line_count {
        s.push_str(&format!("line {i} with some content here\n"));
    }
    s
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn enable_normal(state: &mut VimState, editor: &mut BenchEditor, clipboard: &mut String) {
    state.set_enabled(true);
    let _ = handle_key(state, editor, clipboard, &key(KeyCode::Esc));
}

fn key_normal_navigation(c: &mut Criterion) {
    let buffer = make_multiline_buffer(100);
    let mut group = c.benchmark_group("key_normal_navigation");

    for &ch in &['w', 'b', 'h', 'l', 'j', 'k'] {
        group.bench_function(BenchmarkId::from_parameter(ch.to_string()), |b| {
            b.iter_batched(
                || {
                    let mut state = VimState::new(true);
                    let mut editor = BenchEditor::new(&buffer, 50);
                    let mut clipboard = String::new();
                    enable_normal(&mut state, &mut editor, &mut clipboard);
                    (state, editor, clipboard)
                },
                |(mut state, mut editor, mut clipboard)| {
                    let _ = handle_key(
                        &mut state,
                        &mut editor,
                        &mut clipboard,
                        &key(KeyCode::Char(ch)),
                    );
                    black_box((&state, &editor));
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn key_insert_typing(c: &mut Criterion) {
    c.bench_function("key_insert_typing", |b| {
        b.iter_batched(
            || {
                let mut state = VimState::new(true);
                let mut editor = BenchEditor::new("", 0);
                let mut clipboard = String::new();
                enable_normal(&mut state, &mut editor, &mut clipboard);
                let _ = handle_key(
                    &mut state,
                    &mut editor,
                    &mut clipboard,
                    &key(KeyCode::Char('i')),
                );
                (state, editor, clipboard)
            },
            |(mut state, mut editor, mut clipboard)| {
                for ch in "the quick brown fox jumps over the lazy dog".chars() {
                    let _ = handle_key(
                        &mut state,
                        &mut editor,
                        &mut clipboard,
                        &key(KeyCode::Char(ch)),
                    );
                }
                black_box((&state, &editor));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn mutation_dd(c: &mut Criterion) {
    let mut group = c.benchmark_group("mutation_dd");

    for &line_count in &[10, 100, 1000] {
        group.bench_function(
            BenchmarkId::from_parameter(format!("{line_count}_lines")),
            |b| {
                b.iter_batched(
                    || {
                        let buffer = make_multiline_buffer(line_count);
                        let mut state = VimState::new(true);
                        let mut editor = BenchEditor::new(&buffer, buffer.len() / 2);
                        let mut clipboard = String::new();
                        enable_normal(&mut state, &mut editor, &mut clipboard);
                        (state, editor, clipboard)
                    },
                    |(mut state, mut editor, mut clipboard)| {
                        let _ = handle_key(
                            &mut state,
                            &mut editor,
                            &mut clipboard,
                            &key(KeyCode::Char('d')),
                        );
                        let _ = handle_key(
                            &mut state,
                            &mut editor,
                            &mut clipboard,
                            &key(KeyCode::Char('d')),
                        );
                        black_box((&state, &editor));
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn mutation_paste_linewise(c: &mut Criterion) {
    let mut group = c.benchmark_group("mutation_paste_linewise");

    for &line_count in &[10, 100, 1000] {
        group.bench_function(
            BenchmarkId::from_parameter(format!("{line_count}_lines")),
            |b| {
                b.iter_batched(
                    || {
                        let buffer = make_multiline_buffer(line_count);
                        let mut state = VimState::new(true);
                        let mut editor = BenchEditor::new(&buffer, 0);
                        let mut clipboard = "pasted line one\npasted line two\n".to_string();
                        enable_normal(&mut state, &mut editor, &mut clipboard);
                        let _ = handle_key(
                            &mut state,
                            &mut editor,
                            &mut clipboard,
                            &key(KeyCode::Char('Y')),
                        );
                        (state, editor, clipboard)
                    },
                    |(mut state, mut editor, mut clipboard)| {
                        let _ = handle_key(
                            &mut state,
                            &mut editor,
                            &mut clipboard,
                            &key(KeyCode::Char('p')),
                        );
                        black_box((&state, &editor));
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

fn mutation_join_lines(c: &mut Criterion) {
    let buffer = make_multiline_buffer(100);
    c.bench_function("mutation_join_lines", |b| {
        b.iter_batched(
            || {
                let mut state = VimState::new(true);
                let mut editor = BenchEditor::new(&buffer, 50);
                let mut clipboard = String::new();
                enable_normal(&mut state, &mut editor, &mut clipboard);
                (state, editor, clipboard)
            },
            |(mut state, mut editor, mut clipboard)| {
                let _ = handle_key(
                    &mut state,
                    &mut editor,
                    &mut clipboard,
                    &key(KeyCode::Char('J')),
                );
                black_box((&state, &editor));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn text_object_word(c: &mut Criterion) {
    let buffer = make_multiline_buffer(100);
    let mut group = c.benchmark_group("text_object_word");

    for &(label, around) in &[("viw", false), ("vaw", true)] {
        group.bench_function(BenchmarkId::from_parameter(label), |b| {
            b.iter_batched(
                || {
                    let mut state = VimState::new(true);
                    let mut editor = BenchEditor::new(&buffer, 50);
                    let mut clipboard = String::new();
                    enable_normal(&mut state, &mut editor, &mut clipboard);
                    (state, editor, clipboard)
                },
                |(mut state, mut editor, mut clipboard)| {
                    let _ = handle_key(
                        &mut state,
                        &mut editor,
                        &mut clipboard,
                        &key(KeyCode::Char('c')),
                    );
                    let obj_key = if around { 'a' } else { 'i' };
                    let _ = handle_key(
                        &mut state,
                        &mut editor,
                        &mut clipboard,
                        &key(KeyCode::Char(obj_key)),
                    );
                    let _ = handle_key(
                        &mut state,
                        &mut editor,
                        &mut clipboard,
                        &key(KeyCode::Char('w')),
                    );
                    black_box((&state, &editor));
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn find_char(c: &mut Criterion) {
    let line = "the quick brown fox jumps over the lazy dog";
    c.bench_function("find_char", |b| {
        b.iter_batched(
            || {
                let mut state = VimState::new(true);
                let mut editor = BenchEditor::new(line, 0);
                let mut clipboard = String::new();
                enable_normal(&mut state, &mut editor, &mut clipboard);
                (state, editor, clipboard)
            },
            |(mut state, mut editor, mut clipboard)| {
                let _ = handle_key(
                    &mut state,
                    &mut editor,
                    &mut clipboard,
                    &key(KeyCode::Char('f')),
                );
                let _ = handle_key(
                    &mut state,
                    &mut editor,
                    &mut clipboard,
                    &key(KeyCode::Char('x')),
                );
                black_box((&state, &editor));
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    key_normal_navigation,
    key_insert_typing,
    mutation_dd,
    mutation_paste_linewise,
    mutation_join_lines,
    text_object_word,
    find_char,
);
criterion_main!(benches);
