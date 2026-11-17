//! Cached hairline (divider) tone computation.
//!
//! Every visible theme swatch asks for a hairline on every scroll frame, and
//! card borders ask for one on most frames too. The binary search evaluates
//! `contrast_ratio` 25 times, each measuring two luminances with three `powf`
//! calls. One of those two colors is always the same `fill`, so its luminance
//! is measured once and reused — halving the cold-path `powf` count. Memoizing
//! the pure result per `(fill, foreground)` pair keeps the hot path at one
//! hash lookup.

use super::{contrast_ratio_luminance, mix, relative_luminance};
use gpui_kit::Hsla;
use std::cell::RefCell;
use std::collections::HashMap;

/// A 1px separator tone for `fill`: the smallest blend toward the
/// foreground that stays visible. Contrast grows monotonically along the
/// lightness-only blend, so binary search lands just inside the threshold;
/// when even the full blend falls short (extreme hues), the foreground
/// itself keeps the divider visible.
///
/// Cached per thread by the exact `(fill, foreground)` pair: `Hsla` is
/// `Copy + Eq + Hash`, so it is a direct key, and thread-local state keeps the
/// hot path lock-free with no cross-thread theme bleed.
pub(crate) fn hairline(fill: Hsla, foreground: Hsla) -> Hsla {
    thread_local! {
        static CACHE: RefCell<HashMap<(Hsla, Hsla), Hsla>> = RefCell::new(HashMap::new());
    }
    let key = (fill, foreground);
    if let Some(cached) = CACHE.with(|cache| cache.borrow().get(&key).copied()) {
        return cached;
    }
    let resolved = hairline_uncached(fill, foreground);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        // Bound the cache: contrast drags walk many foreground/fill pairs and
        // user themes add more. A clear costs one recompute, never correctness.
        if cache.len() > 4096 {
            cache.clear();
        }
        let _ = cache.insert(key, resolved);
    });
    resolved
}

fn hairline_uncached(fill: Hsla, foreground: Hsla) -> Hsla {
    const MINIMUM: f32 = 1.6;
    // `fill` is the fixed side of every comparison in the search below; measure
    // its luminance once instead of 25 times. The ratio math is identical to
    // `contrast_ratio(mix(..), fill)` because the helper takes the same two
    // luminances in the same order.
    let fill_lum = relative_luminance(fill);
    if contrast_ratio_luminance(relative_luminance(mix(fill, foreground, 1.0)), fill_lum) < MINIMUM {
        return foreground;
    }
    let mut low = 0.0_f32;
    let mut high = 1.0_f32;
    for _ in 0..24 {
        let middle = (low + high) / 2.0;
        let blend_lum = relative_luminance(mix(fill, foreground, middle));
        if contrast_ratio_luminance(blend_lum, fill_lum) >= MINIMUM {
            high = middle;
        } else {
            low = middle;
        }
    }
    mix(fill, foreground, high)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::themes::contrast_ratio;

    /// The pre-optimization algorithm, kept as an oracle so the hoisted
    /// luminance is proven bit-identical rather than merely close.
    fn reference(fill: Hsla, foreground: Hsla) -> Hsla {
        const MINIMUM: f32 = 1.6;
        if contrast_ratio(mix(fill, foreground, 1.0), fill) < MINIMUM {
            return foreground;
        }
        let mut low = 0.0_f32;
        let mut high = 1.0_f32;
        for _ in 0..24 {
            let middle = (low + high) / 2.0;
            if contrast_ratio(mix(fill, foreground, middle), fill) >= MINIMUM {
                high = middle;
            } else {
                low = middle;
            }
        }
        mix(fill, foreground, high)
    }

    #[test]
    fn hairline_cache_returns_the_same_value_per_pair() {
        let fill = gpui_kit::rgb(0x112233).into();
        let foreground = gpui_kit::rgb(0xF0F0F0).into();
        assert_eq!(hairline(fill, foreground), hairline(fill, foreground));
    }

    #[test]
    fn hoisted_luminance_is_bit_identical_to_the_reference() {
        let colors: Vec<Hsla> = [0x000000, 0x112233, 0x445566, 0x778899, 0xAABBCC, 0xDDEEFF, 0xFFFFFF]
            .into_iter()
            .map(gpui_kit::rgb)
            .map(Hsla::from)
            .collect();
        // Include blended pairs (not just primaries) so the search runs the
        // full 24 iterations, plus a same-luminance pair for the early return.
        for fill in &colors {
            for foreground in &colors {
                assert_eq!(
                    hairline_uncached(*fill, *foreground),
                    reference(*fill, *foreground),
                    "fill={fill:?} foreground={foreground:?}"
                );
            }
        }
    }
}
