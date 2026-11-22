//! Cached muted-text tone computation.
//!
//! Every visible transcript row asks for this tone several times per frame,
//! and each call runs a 24-step contrast binary search (two luminance
//! measurements per step, three `powf` calls each). One of the two colors is
//! always the raised popover fill, so the result depends only on the
//! `(popover, foreground)` pair.
//! Memoizing that pure pair keeps the per-row hot path at one hash lookup —
//! the same pattern `hairline` uses for swatch borders.

use super::{contrast_ratio, text_at_contrast};
use gpui_kit::Hsla;
use gpui_kit::component::theme::ThemeColor;
use std::cell::RefCell;
use std::collections::HashMap;

/// Secondary copy targets 7:1 on the hardest surface (the raised popover).
const TARGET_RATIO: f32 = 7.0;

thread_local! {
    static CACHE: RefCell<HashMap<(Hsla, Hsla), Hsla>> = RefCell::new(HashMap::new());
}

/// Secondary copy (row descriptions, hints) one step brighter than `aa_muted`:
/// small muted text at the muted floor reads washed, so this targets 7:1 on
/// the popover fill (other surfaces have at least as much headroom). Falls
/// back toward the palette foreground on low-headroom themes instead of
/// collapsing below AA.
///
/// Cached per thread by the exact `(popover, foreground)` pair: `Hsla` is
/// `Copy + Eq + Hash`, and thread-local state keeps this lock-free with no
/// cross-thread theme bleed.
pub(in crate::desktop) fn muted_text(colors: &ThemeColor) -> Hsla {
    let key = (colors.popover, colors.foreground);
    if let Some(cached) = CACHE.with(|cache| cache.borrow().get(&key).copied()) {
        return cached;
    }
    let resolved = muted_text_uncached(key.0, key.1);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        // Bound the cache: contrast drags and custom themes walk many pairs.
        // A clear costs one recompute, never correctness.
        if cache.len() > 4096 {
            cache.clear();
        }
        let _ = cache.insert(key, resolved);
    });
    resolved
}

fn muted_text_uncached(popover: Hsla, foreground: Hsla) -> Hsla {
    let headroom = contrast_ratio(foreground, popover);
    text_at_contrast(popover, foreground, TARGET_RATIO.min(headroom))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The inline formula from before the cache, kept as an oracle so the
    /// memoized path is proven bit-identical rather than merely close.
    fn reference(popover: Hsla, foreground: Hsla) -> Hsla {
        let headroom = contrast_ratio(foreground, popover);
        text_at_contrast(popover, foreground, 7.0_f32.min(headroom))
    }

    #[test]
    fn muted_text_cache_returns_the_same_value_per_pair() {
        let mut colors = ThemeColor::default();
        colors.popover = gpui_kit::rgb(0x112233).into();
        colors.foreground = gpui_kit::rgb(0xF0F0F0).into();
        assert_eq!(muted_text(&colors), muted_text(&colors));
    }

    #[test]
    fn muted_text_key_includes_both_colors_and_matches_the_inline_formula() {
        let palette: Vec<Hsla> = [0x000000, 0x112233, 0x778899, 0xDDEEFF, 0xFFFFFF]
            .into_iter()
            .map(gpui_kit::rgb)
            .map(Hsla::from)
            .collect();
        for popover in &palette {
            for foreground in &palette {
                let mut colors = ThemeColor::default();
                colors.popover = *popover;
                colors.foreground = *foreground;
                // Asymmetric pairs exercise the full search; a same-colour
                // pair exercises the low-headroom early return.
                assert_eq!(
                    muted_text(&colors),
                    reference(*popover, *foreground),
                    "popover={popover:?} foreground={foreground:?}"
                );
            }
        }
    }
}
