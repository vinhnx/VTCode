//! Generative (swarm) tests for the text truncation and wrapping helpers.
//!
//! Matklad's "Finding Bugs": small, overlapping inputs drawn from a swarmed
//! alphabet beat large uniform ones, and every property is checked against an
//! independent char-based oracle. The alphabet mixes ASCII, `/`, whitespace and
//! multi-byte chars so byte-vs-char confusion cannot hide.

use super::*;

const ALPHABET: [&str; 9] = ["a", "b", "/", " ", "é", "日", "👋", "\t", "."];

/// Deterministic SplitMix64 so failures reproduce without new dependencies.
struct SwarmRng(u64);

impl SwarmRng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    /// Samples `[low, high)`; `high` must exceed `low`.
    fn range(&mut self, low: usize, high: usize) -> usize {
        let span = u64::try_from(high - low).expect("range width fits in u64");
        low + usize::try_from(self.next_u64() % span).expect("remainder is below a usize span")
    }

    /// Picks a non-empty random subset of `ALPHABET` into the reused buffer.
    fn swarm_alphabet(&mut self, pick: &mut Vec<&'static str>) {
        pick.clear();
        pick.extend(ALPHABET);
        for index in (1..pick.len()).rev() {
            pick.swap(index, self.range(0, index + 1));
        }
        let keep = self.range(1, pick.len() + 1);
        pick.truncate(keep);
    }

    fn gen_text(&mut self, alphabet: &[&str], max_chars: usize, out: &mut String) {
        out.clear();
        for _ in 0..self.range(0, max_chars + 1) {
            out.push_str(alphabet[self.range(0, alphabet.len())]);
        }
    }
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

/// Runs `check` on 4096 swarmed texts plus hand-picked boundary seeds.
fn for_each_text(seed: u64, mut check: impl FnMut(&str, usize)) {
    let mut rng = SwarmRng(seed);
    let mut alphabet = Vec::with_capacity(ALPHABET.len());
    let mut text = String::with_capacity(64);
    for seed_text in [
        "",
        "/",
        "日本/日本語/日本語日本語/xyz",
        "é/é/é/é/é/é/é/é",
        "👋/👋/👋/👋/👋",
    ] {
        for budget in 0..14 {
            check(seed_text, budget);
        }
    }
    for _ in 0..4096 {
        rng.swarm_alphabet(&mut alphabet);
        rng.gen_text(&alphabet, 16, &mut text);
        let budget = rng.range(0, 14);
        check(&text, budget);
    }
}

#[test]
fn truncate_path_middle_never_exceeds_char_budget() {
    for_each_text(1, |text, budget| {
        let out = truncate_path_middle(text, budget);
        assert!(chars(&out) <= budget, "path={text:?} budget={budget} out={out:?}");
        if chars(text) <= budget {
            assert_eq!(out, text);
        }
    });
}

#[test]
fn truncate_middle_matches_char_oracle() {
    for_each_text(2, |text, budget| {
        let sanitized: Vec<char> = text
            .chars()
            .map(|c| if matches!(c, '\n' | '\r' | '\t') { ' ' } else { c })
            .collect();
        let expected: String = if budget == 0 {
            String::new()
        } else if sanitized.len() <= budget {
            sanitized.iter().collect()
        } else if budget == 1 {
            "…".to_string()
        } else {
            let head = budget / 2;
            let tail = budget - head - 1;
            let mut out: String = sanitized[..head].iter().collect();
            out.push('…');
            out.extend(&sanitized[sanitized.len() - tail..]);
            out
        };
        assert_eq!(truncate_middle(text, budget), expected, "text={text:?} budget={budget}");
    });
}

#[test]
fn truncate_within_and_text_match_char_oracle() {
    for_each_text(3, |text, budget| {
        for ellipsis in ["", "…", "..."] {
            let all: Vec<char> = text.chars().collect();
            let expected_within = if all.len() <= budget {
                text.to_string()
            } else {
                let keep = budget.saturating_sub(chars(ellipsis));
                all[..keep].iter().collect::<String>() + ellipsis
            };
            assert_eq!(truncate_within(text, budget, ellipsis), expected_within, "text={text:?} budget={budget}");

            let expected_text = if all.len() <= budget {
                text.to_string()
            } else {
                all[..budget].iter().collect::<String>() + ellipsis
            };
            assert_eq!(truncate_text(text, budget, ellipsis), expected_text, "text={text:?} budget={budget}");
        }
    });
}

#[test]
fn truncate_utf8_prefix_is_the_longest_boundary_prefix() {
    for_each_text(4, |text, budget| {
        let out = truncate_utf8_prefix(text, budget);
        let expected = text
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .filter(|end| *end <= budget)
            .max()
            .unwrap_or(0);
        assert_eq!(out, &text[..expected], "text={text:?} budget={budget}");
    });
}

#[test]
fn head_tail_truncate_respects_budget_and_passthrough() {
    for_each_text(5, |text, budget| {
        // Budgets near the marker length exercise the prefix fallback boundary.
        for max_chars in [budget, budget + 14, budget + 30] {
            let (out, truncated) = head_tail_truncate(text, max_chars, " ... ");
            assert_eq!(truncated, chars(text) > max_chars, "text={text:?} max={max_chars}");
            if truncated {
                assert!(chars(&out) <= max_chars, "text={text:?} max={max_chars} out={out:?}");
            } else {
                assert_eq!(out, text);
            }
        }
    });
}

#[test]
fn wrap_text_words_lines_fit_and_preserve_content() {
    let mut rng = SwarmRng(6);
    let mut alphabet = Vec::with_capacity(ALPHABET.len());
    let mut text = String::with_capacity(64);
    for _ in 0..4096 {
        rng.swarm_alphabet(&mut alphabet);
        rng.gen_text(&alphabet, 24, &mut text);
        let first = rng.range(0, 9);
        let continuation = rng.range(0, 9);
        let lines = wrap_text_words(&text, first, continuation);

        for (index, line) in lines.iter().enumerate() {
            let budget = if index == 0 { first } else { continuation }.max(1);
            assert!(chars(line) <= budget, "text={text:?} widths={first}/{continuation} lines={lines:?}");
            assert!(!line.trim().is_empty(), "blank line: text={text:?} lines={lines:?}");
        }
        // Wrapping only moves whitespace: the non-whitespace stream is intact.
        let strip = |value: &str| value.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        assert_eq!(strip(&lines.concat()), strip(&text), "text={text:?} widths={first}/{continuation}");
    }
}
