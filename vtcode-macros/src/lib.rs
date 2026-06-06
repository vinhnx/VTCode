use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

/// Derive macro that generates the same boilerplate as the `string_newtype!`
/// declarative macro. Apply to a tuple struct wrapping a single `String` field.
///
/// Generates:
/// - Inherent methods: `new()`, `as_str()`, `into_inner()`
/// - `Deref<Target = str>`
/// - `Borrow<str>`
/// - `AsRef<str>`
/// - `Display`
/// - `From<String>`, `From<&str>`, `From<Self> for String`
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone, Serialize, Deserialize, StringNewtype)]
/// #[serde(transparent)]
/// pub struct SessionId(String);
/// ```
#[proc_macro_derive(StringNewtype)]
pub fn derive_string_newtype(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    impl_string_newtype(&input).unwrap_or_else(|err| err.to_compile_error().into())
}

fn impl_string_newtype(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;

    // Validate: must be a tuple struct with exactly one String field.
    let field_type = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unnamed(fields) => {
                if fields.unnamed.len() != 1 {
                    return Err(syn::Error::new_spanned(
                        name,
                        "StringNewtype requires a tuple struct with exactly one field",
                    ));
                }
                let field = fields.unnamed.first().unwrap();
                &field.ty
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    name,
                    "StringNewtype can only be derived for tuple structs",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "StringNewtype can only be derived for structs",
            ));
        }
    };

    // Verify the inner type is String.
    if !is_string_type(field_type) {
        return Err(syn::Error::new_spanned(
            field_type,
            "StringNewtype requires the inner type to be String",
        ));
    }

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let output = quote! {
        impl #impl_generics #name #ty_generics #where_clause {
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

        impl #impl_generics std::ops::Deref for #name #ty_generics #where_clause {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl #impl_generics std::borrow::Borrow<str> for #name #ty_generics #where_clause {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl #impl_generics AsRef<str> for #name #ty_generics #where_clause {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl #impl_generics std::fmt::Display for #name #ty_generics #where_clause {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl #impl_generics From<String> for #name #ty_generics #where_clause {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl #impl_generics From<&str> for #name #ty_generics #where_clause {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl #impl_generics From<#name #ty_generics> for String #where_clause {
            fn from(value: #name #ty_generics) -> Self {
                value.0
            }
        }
    };

    Ok(output.into())
}

fn is_string_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty {
        if type_path.qself.is_none() && type_path.path.segments.len() == 1 {
            return type_path.path.segments[0].ident == "String";
        }
    }
    false
}
