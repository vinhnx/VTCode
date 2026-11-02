# test-refactor-pin-behavior

> Pin current behavior with tests before refactoring someone else's code

## Why It Matters

A refactor is only safe when something can detect a change in behavior. Refactoring a
module with zero tests means your only specification is your own reading of the code -
and ported code often contains quirks that are load-bearing for someone. A few
characterization tests convert "trust me, it's behavior-preserving" into "here is the
diff summarized; here is what proves it".

## Bad

```rust
// Jump straight into restructuring... then change this by hand:
fn parse_line(line: &str) -> Entry {
    entry_from_parts(line.split_once(':').unwrap()) // was 2 args before my edit
}
```

```rust
// Silent refactor that changes observable behavior: malformed lines now
// silently vanish (a panic->empty) without anyone deciding that.
fn into_entry(line: &str) -> Option<Entry> {
    line.split_once(':').and_then(|(k, v)| /* ... */ Some(Entry::new(k, v)))
}
```

## Good

```rust
// Step 1: pin behavior BEFORE touching internals (characterization tests).
#[test]
fn parses_simple_line() {
    assert_eq!(parse_line("host:80"), Entry::new("host", 80));
}

#[test]
fn malformed_line_behavior_is_documented() {
    // Whatever it does TODAY (panic, discard, error) - freeze it, then decide.
    assert!(std::panic::catch_unwind(|| parse_line("garbage")).is_err());
}

// Step 2: refactor, keeping observable semantics identical.

// Step 3: every signature change goes in the change list, before/after.
```

## Refactoring Rules

| Rule | Why |
|------|-----|
| Fix every smell the request is about | Not just the spots convenient to change (`&String` params, `unwrap` on fallible paths, `get_` getters, check-then-unwrap, `+` string concat, glob re-exports). |
| Keep public behavior identical | Same operations, same success/failure semantics, same output bytes - unless changing behavior is *the point*. |
| Turning a panic into `Err` is welcome | Say it explicitly in the change list; silently dropping an error variant is not. |
| Signature changes: before/after | `old: fn load(&PathBuf) -> T` -> `new: fn load(impl AsRef<Path>) -> Result<T>` + why. |
| Preserve unexplained quirks | A strange `else` branch you don't understand may be load-bearing; leave it, flag it in the PR. |
| No silent rewrites | If the module needed `Debug` derived and got `Display` added, both go in the notes. |
| Nondeterministic iteration in output | Fix it (sort keys) AND note it - output ordering is observable. |

## Change-list format

Each non-obvious behavior change gets one line: **what changed, before, after, why**.
When a signature changes, show both forms; if the change is behavior-preserving, say
that term, too.

## See Also

- [test-cfg-test-module](test-cfg-test-module.md) - Unit test module conventions
- [test-integration-dir](test-integration-dir.md) - Integration tests over the public API
- [err-no-unwrap-prod](err-no-unwrap-prod.md) - `unwrap` is not a public-API contract
- [anti-unwrap-abuse](anti-unwrap-abuse.md) - `unwrap` usage anti-patterns
