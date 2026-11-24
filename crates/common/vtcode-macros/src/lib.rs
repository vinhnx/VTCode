#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Data, DataEnum, DeriveInput, Fields, parse_macro_input};

/// Minimum variant count before a unit-only enum uses the concatenated-name
/// `Debug` strategy. The strategy has a fixed cost (name string + byte scan), so
/// it only wins once the per-arm costs of the built-in derive dominate.
const C_LIKE_ENUM_THRESHOLD: usize = 8;

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
                let Some(field) = fields.unnamed.first() else {
                    return Err(syn::Error::new_spanned(
                        name,
                        "StringNewtype requires a tuple struct with exactly one field",
                    ));
                };
                &field.ty
            }
            _ => {
                return Err(syn::Error::new_spanned(name, "StringNewtype can only be derived for tuple structs"));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(name, "StringNewtype can only be derived for structs"));
        }
    };

    // Verify the inner type is String.
    if !is_string_type(field_type) {
        return Err(syn::Error::new_spanned(field_type, "StringNewtype requires the inner type to be String"));
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
    if let syn::Type::Path(type_path) = ty
        && type_path.qself.is_none()
        && type_path.path.segments.len() == 1
    {
        return type_path.path.segments.first().is_some_and(|segment| segment.ident == "String");
    }
    false
}

/// Derive macro equivalent to `#[derive(Debug)]` but with `#[inline(never)]` on
/// the generated `fmt` implementation.
///
/// Rust's built-in `Debug` derive emits `#[inline]` on `fmt`. For large or deeply
/// nested types — typically error enums formatted on fan-out paths — that lets
/// `rustc` inline the whole `Debug` tree into every `{:?}` / `?err` call site,
/// which can bloat binary size. This derive preserves the exact `Debug` output;
/// it changes the inlining hint and, for large unit-only enums, the shape of the
/// generated body (see below).
///
/// C-like enums (all unit variants) with at least `C_LIKE_ENUM_THRESHOLD`
/// variants additionally emit one space-delimited name string and a match that
/// yields byte offsets, instead of one `write_str("<name>")` call per arm. This
/// removes the per-arm string literals and temporary locals that make derived
/// `Debug` expensive to compile and large in the binary; the output is still
/// byte-identical to the built-in derive.
///
/// Use it for large/nested types whose `Debug` is formatted in hot or fan-out
/// paths; keep `#[derive(Debug)]` for small, hot, leaf types. See
/// `docs/development/rust-performance-principles.md`, "Derived trait impls are
/// `#[inline]`".
///
/// # Example
///
/// ```rust,ignore
/// use vtcode_macros::DebugNoInline;
///
/// #[derive(DebugNoInline)]
/// pub struct Widgets {
///     foo: u32,
///     bar: usize,
/// }
///
/// assert_eq!(
///     format!("{:?}", Widgets { foo: 1, bar: 2 }),
///     "Widgets { foo: 1, bar: 2 }"
/// );
/// ```
#[proc_macro_derive(DebugNoInline)]
pub fn derive_debug_no_inline(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match impl_debug_no_inline(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn impl_debug_no_inline(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;

    // Mirror the built-in derive: bound every type parameter on `Debug`.
    let mut generics = input.generics.clone();
    for param in generics.type_params_mut() {
        param.bounds.push(syn::parse_quote!(::core::fmt::Debug));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let body = match &input.data {
        Data::Struct(data) => fmt_body_struct(&name.to_string(), &data.fields),
        Data::Enum(data) => fmt_body_enum(data),
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(name, "DebugNoInline cannot be derived for unions"));
        }
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::core::fmt::Debug for #name #ty_generics #where_clause {
            #[inline(never)]
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                #body
            }
        }
    })
}

/// Debug name for a field: strip a raw-identifier prefix so `r#type` renders as
/// `type`, matching the built-in derive.
fn field_name(ident: &syn::Ident) -> String {
    let raw = ident.to_string();
    raw.trim_prefix("r#").to_string()
}

fn fmt_body_struct(type_name: &str, fields: &Fields) -> TokenStream2 {
    match fields {
        Fields::Unit => quote! { f.write_str(#type_name) },
        Fields::Named(named) => {
            let calls = named.named.iter().filter_map(|field| {
                let ident = field.ident.as_ref()?;
                let field_name = field_name(ident);
                Some(quote! { __debug_builder.field(#field_name, &self.#ident); })
            });
            let builder = debug_builder_init("debug_struct", type_name);
            quote! {
                #builder
                #(#calls)*
                __debug_builder.finish()
            }
        }
        Fields::Unnamed(unnamed) => {
            let calls = unnamed.unnamed.iter().enumerate().map(|(index, _)| {
                let index = syn::Index::from(index);
                quote! { __debug_builder.field(&self.#index); }
            });
            let builder = debug_builder_init("debug_tuple", type_name);
            quote! {
                #builder
                #(#calls)*
                __debug_builder.finish()
            }
        }
    }
}

fn fmt_body_enum(data: &DataEnum) -> TokenStream2 {
    // An uninhabited enum has no variants to match. `match self {}` is not
    // exhaustive because `&Never` is inhabited, so dereference: `match *self {}`.
    if data.variants.is_empty() {
        return quote! { match *self {} };
    }

    // Large unit-only enums are the pathological case for the built-in derive:
    // match checking is quadratic in arm count and borrow checking allocates a
    // temp local for every string-literal arm. Emit one name string plus a match
    // of integer offsets instead (the strategy upstream adopted for
    // `derive(Debug)`). Keep a match rather than using `discriminant_value`:
    // attribute macros such as `bitflags` can rewrite discriminants after the
    // derive runs, which would make offsets point at the wrong names.
    if data.variants.len() >= C_LIKE_ENUM_THRESHOLD
        && data.variants.iter().all(|variant| matches!(variant.fields, Fields::Unit))
    {
        return fmt_body_c_like_enum(data);
    }

    let arms = data.variants.iter().map(|variant| {
        let variant_ident = &variant.ident;
        let variant_name = field_name(variant_ident);
        match &variant.fields {
            Fields::Unit => quote! {
                Self::#variant_ident => f.write_str(#variant_name),
            },
            Fields::Named(named) => {
                let pairs: Vec<(syn::Ident, syn::Ident)> = named
                    .named
                    .iter()
                    .enumerate()
                    .filter_map(|(index, field)| {
                        let ident = field.ident.as_ref()?.clone();
                        Some((ident, format_ident!("__self_{index}")))
                    })
                    .collect();
                let pattern = pairs.iter().map(|(ident, binding)| quote! { #ident: #binding });
                let calls = pairs.iter().map(|(ident, binding)| {
                    let field_name = field_name(ident);
                    quote! { __debug_builder.field(#field_name, #binding); }
                });
                let builder = debug_builder_init("debug_struct", &variant_name);
                quote! {
                    Self::#variant_ident { #(#pattern),* } => {
                        #builder
                        #(#calls)*
                        __debug_builder.finish()
                    }
                }
            }
            Fields::Unnamed(unnamed) => {
                let bindings: Vec<syn::Ident> = (0..unnamed.unnamed.len())
                    .map(|index| format_ident!("__self_{index}"))
                    .collect();
                let calls = bindings.iter().map(|binding| quote! { __debug_builder.field(#binding); });
                let builder = debug_builder_init("debug_tuple", &variant_name);
                quote! {
                    Self::#variant_ident( #(#bindings),* ) => {
                        #builder
                        #(#calls)*
                        __debug_builder.finish()
                    }
                }
            }
        }
    });
    quote! {
        match self {
            #(#arms)*
        }
    }
}

/// `Debug` body for a unit-only enum: write a name sliced out of one
/// space-delimited string. Offsets are byte offsets computed from the variant
/// names, and the delimiter is found with an ASCII byte scan (no UTF-8 iterator
/// or panic-formatting machinery). Raw-identifier prefixes are stripped so the
/// output matches the built-in derive.
fn fmt_body_c_like_enum(data: &DataEnum) -> TokenStream2 {
    let mut names = String::new();
    let mut offset = 0usize;
    let mut arms = Vec::with_capacity(data.variants.len());
    for variant in &data.variants {
        let variant_ident = &variant.ident;
        let name = field_name(variant_ident);
        if !names.is_empty() {
            names.push(' ');
        }
        names.push_str(&name);
        arms.push(quote! { Self::#variant_ident => #offset, });
        offset += name.len() + 1;
    }

    quote! {
        const __NAMES: &str = #names;
        let __start: usize = match self {
            #(#arms)*
        };
        let __tail = __NAMES.get(__start..).unwrap_or_default();
        let __end = __tail
            .as_bytes()
            .iter()
            .position(|byte| *byte == b' ')
            .unwrap_or(__tail.len());
        f.write_str(__tail.get(..__end).unwrap_or_default())
    }
}

/// Emit `let mut __debug_builder = f.<kind>("<name>");`. The builder is always
/// `mut` because `field(...)` and `finish(...)` both take `&mut self`.
fn debug_builder_init(kind: &str, name: &str) -> TokenStream2 {
    let kind = format_ident!("{kind}");
    quote! { let mut __debug_builder = f.#kind(#name); }
}
