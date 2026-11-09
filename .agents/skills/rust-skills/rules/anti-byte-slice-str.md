# anti-byte-slice-str

> Don't cut text at a byte offset; cut on a char boundary, and decode byte streams across chunk boundaries

## Why It Matters

Byte budgets (prompt caps, log limits, preview sizes, snapshot shrinking) are measured in bytes, but `str` and
`String` must stay valid UTF-8. A cut that lands inside a multibyte character (accented letters, CJK, emoji, smart
quotes) either panics or corrupts the text:

- `&s[..n]` and `String::truncate(n)` **panic** when `n` is not a char boundary. Tests written with ASCII never hit it;
  the first Vietnamese transcript or emoji in production does.
- `String::from_utf8_lossy(&bytes[..n])` never panics, but turns the split character into `U+FFFD`, which then leaks
  into prompts, logs, and UI.
- Decoding each `read()` chunk or network frame on its own has the same `U+FFFD` problem at every chunk boundary,
  so long output gets randomly corrupted.

`clippy::string_slice` flags `&s[..n]` but not `String::truncate`, `split_at`, or lossy decoding, so this is a review
rule as much as a lint.

## Bad

```rust
// Panics if byte 397 is inside a character.
fn shrink(value: &mut String) {
    let keep = value.len() / 2 - 3;
    value.truncate(keep);
    value.push_str("...");
}

// U+FFFD at the cut when the cap splits a character.
let text = String::from_utf8_lossy(&bytes[..cap]).into_owned();

// U+FFFD wherever a read splits a character.
while let Some(chunk) = stream.next().await {
    out.push_str(&String::from_utf8_lossy(&chunk?));
}

// Hand-rolled boundary search: correct, but O(n) and easy to get wrong.
let mut end = max;
while !s.is_char_boundary(end) {
    end -= 1;
}
```

## Good

```rust
// Round the cut down to a char boundary (stable since Rust 1.91; clamps n > len).
fn shrink(value: &mut String) {
    let keep = value.floor_char_boundary((value.len() / 2).saturating_sub(3));
    value.truncate(keep);
    value.push_str("...");
}

// Prefix of a borrowed str within a byte budget.
fn prefix_within(s: &str, max_bytes: usize) -> &str {
    &s[..s.floor_char_boundary(max_bytes)]
}

// Streams: carry an incomplete trailing sequence into the next chunk, and flush it at end of stream.
// In VT Code use `vtcode_commons::Utf8StreamDecoder` (`push`, `push_cow` for hot loops, `finish`).
let mut decoder = Utf8StreamDecoder::new();
while let Some(chunk) = stream.next().await {
    out.push_str(&decoder.push_cow(&chunk?));
}
out.push_str(&decoder.finish());

// Byte-capped read: drop a split trailing character only when the cap actually cut the data.
let mut decoder = Utf8StreamDecoder::new();
let mut text = decoder.push(&bytes[..bytes.len().min(cap)]);
if !truncated {
    text.push_str(&decoder.finish()); // genuinely invalid tail bytes stay visible as U+FFFD
}
```

## Choosing the Unit

| Budget is about | Cut by | Tool |
| --- | --- | --- |
| Storage, wire, or token-adjacent size | Bytes | `floor_char_boundary` |
| What a person reads (titles, previews) | Characters or display width | `chars().take(n)`, `unicode-width` |
| Bytes arriving in pieces | Stream decoding | a stateful UTF-8 decoder, never per-chunk lossy decoding |

## Test It

Use input where the naive cut is guaranteed to land mid-character, and assert the exact output, not just "no panic":

```rust
#[test]
fn shrink_cuts_multibyte_text_on_a_char_boundary() {
    let mut value = "é".repeat(400); // 2 bytes each: a cut at 397 lands mid-char
    shrink(&mut value);
    assert_eq!(value, format!("{}...", "é".repeat(198)));
}
```

## See Also

- [own-slice-over-vec](own-slice-over-vec.md) - Borrow `&str` and return sub-slices where possible
- [mem-zero-copy](mem-zero-copy.md) - `Cow` lets a decoder borrow valid chunks without copying
- [test-proptest-properties](test-proptest-properties.md) - Property-test cuts over arbitrary Unicode strings
