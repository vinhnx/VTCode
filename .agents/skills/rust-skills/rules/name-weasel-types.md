# name-weasel-types

> No `Service`, `Manager`, `Factory`, `Helper`, `Util` suffixes

## Why It Matters

`XxxManager` names a relationship, not a thing: it promises "I will manage something"
without saying what. `Helper` and `Util`try to express "miscellaneous" - which is not a
job, and becomes a dumping ground. Weasel suffixes obscure the design, produce worse
grep results in errors, and are hard to delete later because they organize nothing.

Name the *thing itself*: what the type is, or the verb it owns.

## Bad

```rust
pub struct UserManager;      // manages... what? sessions? permissions?
pub struct FileHelper;       // helps with what?
pub mod string_util;         // a grab-bag nobody maintains
pub struct PriceFactory;     // inheritance-era vocabulary, not Rust
pub struct DatabaseService;  // what does it DO?
// in crate `foo`:
pub struct FooId;            // reads as foo::FooId - module name repeated
```

## Good

```rust
pub struct Sessions;         // owns session state
pub struct Crawler;          // the thing itself
pub mod str_ext;             // extension methods, honestly named
pub struct Price;            // constructed via From<Parts> or a builder
pub struct MailSender;       // the verb it owns
// in crate `foo`:  pub struct Id;   // path reads foo::Id
```

## The Rules

| Smell | Fix |
|-------|-----|
| `XxxManager` | Name what it manages (`Bookings`, `Registry`) or the verb (`Dispatcher`, `Scheduler`) |
| `XxxFactory` | If it builds one thing, name the thing (`Decoder`, `Price`); prototyping many is `Foo::builder()` (rust-styled `Builder` pattern) |
| `XxxService` | Name the responsibility: `PriceCalculator`, `WebhookDispatcher`, or the domain noun |
| `XxxUtil` / `XxxHelper` | Split into focused free functions with searchable names (`percent_encode`), group under a topic module (`mod format`) |
| `XxxImpl` / `XxxBase` | Design traits instead of implying inheritance |
| `foo::FooId` inside `mod foo` | `foo::Id` - don't repeat the module name in the type |
| Unrecognizable abbreviations | Spell them out; keep only ecosystem-common ones (`IpAddr`, `Uuid`) |

## Repeatable instantiation

The rare case that genuinely is a "factory" over call-heavy types: take an
`impl Fn() -> Foo` instead of inventing a `FooFactory` type.

## See Also

- [name-no-get-prefix](name-no-get-prefix.md) - No `get_` prefix on getters
- [name-acronym-word](name-acronym-word.md) - Acronyms read as one word
- [api-builder-pattern](api-builder-pattern.md) - Builder over constructor sprawl
- [api-extension-trait](api-extension-trait.md) - Extension traits for foreign types
