# test-miri-fuzz

> Miri for every `unsafe`; `cargo fuzz` for every untrusted-input parser

## Why It Matters

`unsafe` code and input parsers are the two places where ordinary tests are nearly
useless: a normal test exercises one path through a search space with billions of
invalid states. Miri (an interpreter) checks undefined behavior on the executed paths;
fuzzing explores the invalid-input space systematically. Both are cheap to wire in and
find bugs panics cannot.

## Bad

```rust
#[test]
fn parse_header_ok() {
    let h = parse_header(b"\x01\x02len=...").unwrap(); // one happy path
}

// And no check that parse_header(garbage) cannot read out of bounds -
// exactly the bug class that gets CVEs assigned.
```

## Good

```toml
# Cargo.toml - fuzz target for the parser surfaces
[dependencies]
fuzz-targets = []   # (respect the tool's own setup; artemis example)

# fuzz/fuzz_targets/parse_header.rs:
fuzz_target!(|data: &[u8]| {
    let _ = parse_header(data); // must not panic, OOB, or UB - return is fine
});
```

```rust
// Miri over the unsafe core:
// $ cargo +nightly miri test --lib unsafe_parser
#[cfg(test)]
mod tests {
    #[test]
    fn aligned_reads_stay_in_bounds() {
        // SAFETY-adjacent: runs under Miri in CI to detect UB.
        let mut buf = vec![0u8; 32];
        for len in [0, 1, 15, 16, 31, 32] {
            unsafe { unsafe_parser::read_chunk(buf.as_mut_ptr(), len) };
        }
    }
}
```

## The Checklist

| Item | Tool | Where |
|------|------|-------|
| Every `unsafe` block/impl exercised | `cargo +nightly miri test` | CI on the crates owning `unsafe` |
| Every byte/lower parse surface | `cargo fuzz` (libFuzzer + ASan) | nightly/smoke jobs; keep the seeds |
| Memory sanitizer accidents | Miri or ASan found via fuzz | CI flags `RUSTFLAGS="-Z sanitizer=address"` |
| Benchmarks, not assertions | `criterion` + `std::hint::black_box` | bench targets (see `test-criterion-bench`) |
| Property invariants (`any(x) => roundtrip(x)`) | `proptest`/`quickcheck` | unit tests (see `test-proptest-properties`) |

## Adversarial Inputs

Fuzz targets treat *input* as hostile: truncation at every point, high-bit bytes,
over-large lengths declared in-band, mixed encodings. For unsafe cores, add tests that
attempt to violate the invariant *through the safe API* (`anti-unsound-api`) - if one
succeeds, the boundary leaks.

## See Also

- [test-proptest-properties](test-proptest-properties.md) - Property-based testing
- [test-criterion-bench](test-criterion-bench.md) - Criterion benchmarks with `black_box`
- [anti-unsound-api](anti-unsound-api.md) - Safe APIs must not allow UB
- [doc-safety-section](doc-safety-section.md) - Documenting unsafe contracts
