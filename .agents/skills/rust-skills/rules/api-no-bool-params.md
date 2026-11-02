# api-no-bool-params

> No bool parameters in public or shared signatures

## Why It Matters

`report(true)` is unreadable at the call site: the reader cannot tell what `true`
means without opening the definition. A `bool` also cannot grow a third mode - which
real options usually need eventually (`Summary` / `Detailed` / `Quiet`). A
two-variant enum costs one extra line and reads like prose at every call site.

## Bad

```rust
widget.render(true);          // what is 'true'?
widget.render(false);         // and 'false'?
Widget::new(true, false);     // two booleans, zero information
```

## Good

```rust
enum Verbosity { Summary, Full }

impl Widget {
    fn render(&self, verbosity: Verbosity) { /* ... */ }
}

widget.render(Verbosity::Full);        // reads like prose
Widget::new(Size::Small, Shape::Round); // nothing to decode
```

## Alternative: two methods

When the modes are exactly two and rarely grow, two focused methods also work:

```rust
impl Widget {
    pub fn render(&self) { /* default */ }
    pub fn render_verbose(&self) { /* with details */ }
}
```

## The one exception

A private helper whose call sites sit a few lines away may keep a `bool`, because the
definition is in view while reading the call. When you refactor someone else's code, an
existing `bool` parameter is one of the smells to remove, not a detail to preserve.

## Related smells in the same family

`report(true)` unreadable; missing named `Debug`/`Eq` methods; the "please read the
definition" tax. Apply the same reasoning to other ambiguous scalar parameters: don't
pass `u8` or `Option<_>` where the meaning is unclear - use an enum/newtype.

## See Also

- [api-non-exhaustive](api-non-exhaustive.md) - Future-proofing enums and structs
- [type-enum-states](type-enum-states.md) - Enums for mutually exclusive states
- [type-no-stringly](type-no-stringly.md) - Avoid stringly-typed APIs
- [api-newtype-safety](api-newtype-safety.md) - Newtypes for type-safe distinctions
