//! Definition of the `model_id_table!` macro used by the model catalog table.
//!
//! Kept separate from the table rows so the generated accessors and the row
//! data are independent concerns. The macro is imported into the parent module
//! with `#[macro_use]`, so the invocation there is unchanged and all resolved
//! paths (`ModelId`, `Provider`, `models::*`) still bind at the invocation site.

macro_rules! model_id_table {
    (
        $(
            $variant:ident {
                provider: $provider:ident,
                id: $id:expr,
                $( parse: [ $( $parse:expr ),* $(,)? ], )?
                display: $display:expr,
                description: $description:expr $(,)?
            }
        ),* $(,)?
    ) => {
        impl ModelId {
            /// Canonical id string for table-backed variants; `None` for
            /// OpenRouter and `Custom` variants.
            pub(super) fn table_id(&self) -> Option<&'static str> {
                match self {
                    $( ModelId::$variant => Some($id), )*
                    _ => None,
                }
            }

            /// Display name for table-backed variants.
            pub(super) fn table_display(&self) -> Option<&'static str> {
                match self {
                    $( ModelId::$variant => Some($display), )*
                    _ => None,
                }
            }

            /// Description for table-backed variants.
            pub(super) fn table_description(&self) -> Option<&'static str> {
                match self {
                    $( ModelId::$variant => Some($description), )*
                    _ => None,
                }
            }

            /// Provider for table-backed variants.
            pub(super) fn table_provider(&self) -> Option<Provider> {
                match self {
                    $( ModelId::$variant => Some(Provider::$provider), )*
                    _ => None,
                }
            }

            /// Resolve a bare model string to a table-backed variant.
            /// Checked in row order so shared strings keep legacy resolution.
            pub(super) fn parse_table(s: &str) -> Option<ModelId> {
                $(
                    $( $( if s == $parse { return Some(ModelId::$variant); } )* )*
                    model_id_table!(@default_parse s, $variant, $id $(, [$($parse),*] )? );
                )*
                None
            }
        }
    };

    // No `parse` field: the canonical `id` is the only string that resolves.
    (@default_parse $s:ident, $variant:ident, $id:expr) => {
        if $s == $id { return Some(ModelId::$variant); }
    };
    // Explicit `parse: []`: picker-only, never resolved from the table.
    (@default_parse $s:ident, $variant:ident, $id:expr, []) => {
        let _ = ($s, $id);
    };
    // Explicit parse list: those strings already cover resolution.
    (@default_parse $s:ident, $variant:ident, $id:expr, [$($parse:expr),+ $(,)?]) => {
        let _ = ($s, $id);
    };
}
