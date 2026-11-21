/// One classified line of a unified diff.
pub(in crate::desktop::view) struct DiffLine<'a> {
    pub(in crate::desktop::view) kind: DiffLineKind,
    pub(in crate::desktop::view) text: &'a str,
}

pub(in crate::desktop::view) const MAX_DIFF_LINES: usize = 2_000;

/// Transcript-visible lines of a unified diff: git framing (`diff --git`,
/// `---`/`+++`) is skipped because the card's file rows already name each
/// path, and only yielded lines count toward [`MAX_DIFF_LINES`].
pub(in crate::desktop::view) struct ClassifiedDiffLines<'a> {
    lines: std::str::Lines<'a>,
    remaining: usize,
    checked_limit: bool,
    pub(in crate::desktop::view) truncated: bool,
}

impl<'a> ClassifiedDiffLines<'a> {
    pub(in crate::desktop::view) fn new(patch: &'a str) -> Self {
        Self {
            lines: patch.lines(),
            remaining: MAX_DIFF_LINES,
            checked_limit: false,
            truncated: false,
        }
    }
}

impl<'a> Iterator for ClassifiedDiffLines<'a> {
    type Item = DiffLine<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.remaining == 0 {
                if !self.checked_limit {
                    // Framing past the budget must not mark a fully-rendered
                    // patch truncated: only visible lines count.
                    self.truncated = self.lines.clone().any(|text| classify_diff_line(text) != DiffLineKind::Header);
                    self.checked_limit = true;
                }
                return None;
            }

            let Some(text) = self.lines.next() else {
                self.remaining = 0;
                self.checked_limit = true;
                return None;
            };
            let kind = classify_diff_line(text);
            if kind == DiffLineKind::Header {
                continue;
            }
            self.remaining -= 1;
            return Some(DiffLine { kind, text });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::desktop::view) enum DiffLineKind {
    Header,
    Hunk,
    Added,
    Removed,
    Context,
}

fn classify_diff_line(line: &str) -> DiffLineKind {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") {
        DiffLineKind::Header
    } else if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with('+') {
        DiffLineKind::Added
    } else if line.starts_with('-') {
        DiffLineKind::Removed
    } else {
        DiffLineKind::Context
    }
}
