pub(crate) struct PatchContextMatcher<'a> {
    lines: &'a [String],
}

impl<'a> PatchContextMatcher<'a> {
    pub(crate) fn new(lines: &'a [String]) -> Self {
        Self { lines }
    }

    pub(crate) fn seek(&self, pattern: &[String], start: usize, eof: bool) -> Option<usize> {
        if pattern.is_empty() {
            return Some(start);
        }

        if pattern.len() > self.lines.len() {
            return None;
        }

        let search_start = if eof && self.lines.len() >= pattern.len() {
            self.lines.len().saturating_sub(pattern.len())
        } else {
            start
        };

        let max_start = self.lines.len().saturating_sub(pattern.len());

        for idx in search_start..=max_start {
            if self.lines[idx..idx + pattern.len()] == *pattern {
                return Some(idx);
            }
        }

        for idx in search_start..=max_start {
            let mut ok = true;
            for (offset, pat) in pattern.iter().enumerate() {
                if self.lines[idx + offset].trim_end() != pat.trim_end() {
                    ok = false;
                    break;
                }
            }
            if ok {
                return Some(idx);
            }
        }

        for idx in search_start..=max_start {
            let mut ok = true;
            for (offset, pat) in pattern.iter().enumerate() {
                if self.lines[idx + offset].trim() != pat.trim() {
                    ok = false;
                    break;
                }
            }
            if ok {
                return Some(idx);
            }
        }

        for idx in search_start..=max_start {
            let mut ok = true;
            for (offset, pat) in pattern.iter().enumerate() {
                if normalise(&self.lines[idx + offset]) != normalise(pat) {
                    ok = false;
                    break;
                }
            }
            if ok {
                return Some(idx);
            }
        }

        None
    }
}

fn normalise(input: &str) -> String {
    input
        .trim()
        .chars()
        .map(|c| match c {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}'
            | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200A}' | '\u{202F}' | '\u{205F}'
            | '\u{3000}' => ' ',
            other => other,
        })
        .collect()
}
