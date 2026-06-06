/// Generate a string newtype with standard trait implementations.
///
/// This macro eliminates the repeated boilerplate found in ID wrapper types
/// that wrap `String` and delegate common traits to the inner value. It
/// generates:
///
/// - The struct definition (tuple struct wrapping `String`)
/// - Inherent methods: `new()`, `as_str()`, `into_inner()`
/// - `Deref<Target = str>`
/// - `Borrow<str>`
/// - `AsRef<str>`
/// - `Display`
/// - `From<String>`, `From<&str>`, `From<Self> for String`
///
/// Derive attributes and any custom constructors should be applied in the
/// attribute position before the struct, since each type may need different
/// derives (e.g., `Hash`, `Serialize`, `Deserialize`) or construction logic.
///
/// # Example
///
/// ```rust
/// use vtcode_commons::string_newtype;
///
/// string_newtype! {
///     #[derive(Debug, Clone, PartialEq, Eq, Hash)]
///     /// A unique request identifier.
///     pub struct RequestId
/// }
///
/// let id = RequestId::new("req-123");
/// assert_eq!(id.as_str(), "req-123");
/// assert_eq!(&*id, "req-123");
/// assert_eq!(id.to_string(), "req-123");
/// ```
#[macro_export]
macro_rules! string_newtype {
    ($(#[$attr:meta])* $vis:vis struct $name:ident) => {
        $(#[$attr])*
        $vis struct $name(String);

        impl $name {
            /// Create a new instance from any value that converts to `String`.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Borrow the inner string as a `&str`.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume the wrapper and return the inner `String`.
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}
