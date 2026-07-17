//! Shared macro for defining string-backed identifier newtypes.
//!
//! Each generated type wraps a [`compact_str::CompactString`] and implements the
//! traits needed to use it as a `HashMap` key (`Hash` + `Eq` + `Borrow<str>`),
//! serialize transparently, and compare against string slices.

/// Define a string-backed identifier newtype with the full set of ergonomic
/// trait implementations (`Display`, `Deref<Target = str>`, `Borrow<str>`,
/// serde transparent, and `PartialEq` against string types).
macro_rules! define_id_newtype {
    ($(#[$meta:meta])* $vis:vis struct $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Default,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        $vis struct $name(compact_str::CompactString);

        #[cfg(feature = "schema")]
        impl schemars::JsonSchema for $name {
            fn inline_schema() -> bool {
                <String as schemars::JsonSchema>::inline_schema()
            }

            fn schema_name() -> std::borrow::Cow<'static, str> {
                <String as schemars::JsonSchema>::schema_name()
            }

            fn schema_id() -> std::borrow::Cow<'static, str> {
                <String as schemars::JsonSchema>::schema_id()
            }

            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                <String as schemars::JsonSchema>::json_schema(generator)
            }
        }

        impl $name {
            /// Create a new identifier from anything string-like.
            pub fn new(value: impl Into<compact_str::CompactString>) -> Self {
                Self(value.into())
            }

            /// Borrow the identifier as a string slice.
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }

            /// Consume the identifier, returning the inner string.
            pub fn into_inner(self) -> compact_str::CompactString {
                self.0
            }

            /// True when the identifier is empty.
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0.as_str())
            }
        }

        impl std::hash::Hash for $name {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.0.as_str().hash(state);
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.0.as_str()
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;

            fn deref(&self) -> &str {
                self.0.as_str()
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.0.as_str()
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value.into())
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.into())
            }
        }

        impl From<compact_str::CompactString> for $name {
            fn from(value: compact_str::CompactString) -> Self {
                Self(value)
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0.as_str() == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0.as_str() == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                self.0.as_str() == other.as_str()
            }
        }
    };
}

pub(crate) use define_id_newtype;
