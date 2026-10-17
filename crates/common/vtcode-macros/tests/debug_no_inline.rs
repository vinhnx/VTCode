//! Output-equivalence tests for `DebugNoInline`.
//!
//! The derive must produce byte-identical output to the standard `Debug` derive
//! for every type shape; only the inlining hint differs. Each shell below defines
//! the same types twice — once with `#[derive(Debug)]`, once with
//! `#[derive(DebugNoInline)]` — so the printed type/variant names also match, and
//! compares them in both compact and alternate (`{:#?}`) modes.

#![allow(
    dead_code,
    reason = "twin type definitions exist only to exercise derived Debug output"
)]

fn assert_same<Expected: std::fmt::Debug, Actual: std::fmt::Debug>(expected: &Expected, actual: &Actual) {
    assert_eq!(format!("{expected:?}"), format!("{actual:?}"), "compact Debug output diverged from the std derive");
    assert_eq!(
        format!("{expected:#?}"),
        format!("{actual:#?}"),
        "alternate Debug output diverged from the std derive"
    );
}

mod std_types {
    #[derive(Debug)]
    pub struct Unit;

    #[derive(Debug)]
    pub struct Named {
        pub alpha: u32,
        pub beta: String,
    }

    #[derive(Debug)]
    pub struct EmptyNamed {}

    #[derive(Debug)]
    pub struct Tuple(pub u8, pub i64);

    #[derive(Debug)]
    pub struct EmptyTuple();

    #[derive(Debug)]
    pub struct Raw {
        pub r#type: String,
        pub r#match: u8,
    }

    #[derive(Debug)]
    pub enum Mixed {
        Unit,
        Named { x: i32, y: Vec<u8> },
        Tuple(Option<u8>, &'static str),
        EmptyNamed {},
        EmptyTuple(),
    }

    #[derive(Debug)]
    pub enum Never {}

    #[derive(Debug)]
    pub struct Generic<T, U> {
        pub first: T,
        pub second: U,
    }

    #[derive(Debug)]
    pub enum GenericEnum<T> {
        None,
        Some(T),
    }

    #[derive(Debug)]
    pub struct WithLifetime<'a> {
        pub name: &'a str,
    }

    #[derive(Debug)]
    pub struct WithWhereClause<T>
    where
        T: Clone,
    {
        pub value: T,
    }

    #[derive(Debug)]
    pub enum RawEnum {
        Named { r#type: String },
    }

    #[derive(Debug)]
    pub struct Inner {
        pub value: u8,
    }

    #[derive(Debug)]
    pub struct Outer {
        pub inner: Inner,
        pub name: String,
    }
}

mod no_inline_types {
    use vtcode_macros::DebugNoInline;

    #[derive(DebugNoInline)]
    pub struct Unit;

    #[derive(DebugNoInline)]
    pub struct Named {
        pub alpha: u32,
        pub beta: String,
    }

    #[derive(DebugNoInline)]
    pub struct EmptyNamed {}

    #[derive(DebugNoInline)]
    pub struct Tuple(pub u8, pub i64);

    #[derive(DebugNoInline)]
    pub struct EmptyTuple();

    #[derive(DebugNoInline)]
    pub struct Raw {
        pub r#type: String,
        pub r#match: u8,
    }

    #[derive(DebugNoInline)]
    pub enum Mixed {
        Unit,
        Named { x: i32, y: Vec<u8> },
        Tuple(Option<u8>, &'static str),
        EmptyNamed {},
        EmptyTuple(),
    }

    #[derive(DebugNoInline)]
    pub enum Never {}

    #[derive(DebugNoInline)]
    pub struct Generic<T, U> {
        pub first: T,
        pub second: U,
    }

    #[derive(DebugNoInline)]
    pub enum GenericEnum<T> {
        None,
        Some(T),
    }

    #[derive(DebugNoInline)]
    pub struct WithLifetime<'a> {
        pub name: &'a str,
    }

    #[derive(DebugNoInline)]
    pub struct WithWhereClause<T>
    where
        T: Clone,
    {
        pub value: T,
    }

    #[derive(DebugNoInline)]
    pub enum RawEnum {
        Named { r#type: String },
    }

    #[derive(DebugNoInline)]
    pub struct Inner {
        pub value: u8,
    }

    #[derive(DebugNoInline)]
    pub struct Outer {
        pub inner: Inner,
        pub name: String,
    }
}

#[test]
fn unit_struct_matches() {
    assert_same(&std_types::Unit, &no_inline_types::Unit);
}

#[test]
fn named_struct_matches() {
    let expected = std_types::Named { alpha: 7, beta: "x".to_string() };
    let actual = no_inline_types::Named { alpha: 7, beta: "x".to_string() };
    assert_same(&expected, &actual);
}

#[test]
fn empty_named_struct_matches() {
    assert_same(&std_types::EmptyNamed {}, &no_inline_types::EmptyNamed {});
}

#[test]
fn tuple_struct_matches() {
    assert_same(&std_types::Tuple(1, -2), &no_inline_types::Tuple(1, -2));
}

#[test]
fn empty_tuple_struct_matches() {
    assert_same(&std_types::EmptyTuple(), &no_inline_types::EmptyTuple());
}

#[test]
fn raw_identifier_fields_match() {
    let expected = std_types::Raw { r#type: "t".into(), r#match: 3 };
    let actual = no_inline_types::Raw { r#type: "t".into(), r#match: 3 };
    assert_same(&expected, &actual);
}

#[test]
fn enum_variants_match() {
    assert_same(&std_types::Mixed::Unit, &no_inline_types::Mixed::Unit);
    assert_same(
        &std_types::Mixed::Named { x: 1, y: vec![2, 3] },
        &no_inline_types::Mixed::Named { x: 1, y: vec![2, 3] },
    );
    assert_same(&std_types::Mixed::Tuple(Some(1), "s"), &no_inline_types::Mixed::Tuple(Some(1), "s"));
    assert_same(&std_types::Mixed::EmptyNamed {}, &no_inline_types::Mixed::EmptyNamed {});
    assert_same(&std_types::Mixed::EmptyTuple(), &no_inline_types::Mixed::EmptyTuple());
}

#[test]
fn generic_struct_matches() {
    assert_same(
        &std_types::Generic { first: 1u8, second: "s" },
        &no_inline_types::Generic { first: 1u8, second: "s" },
    );
}

#[test]
fn generic_enum_matches() {
    assert_same(&std_types::GenericEnum::<u8>::None, &no_inline_types::GenericEnum::<u8>::None);
    assert_same(&std_types::GenericEnum::Some(5u8), &no_inline_types::GenericEnum::Some(5u8));
}

#[test]
fn lifetime_struct_matches() {
    assert_same(&std_types::WithLifetime { name: "hi" }, &no_inline_types::WithLifetime { name: "hi" });
}

#[test]
fn where_clause_struct_matches() {
    assert_same(&std_types::WithWhereClause { value: 3u8 }, &no_inline_types::WithWhereClause { value: 3u8 });
}

#[test]
fn raw_identifier_enum_field_matches() {
    let expected = std_types::RawEnum::Named { r#type: "t".into() };
    let actual = no_inline_types::RawEnum::Named { r#type: "t".into() };
    assert_same(&expected, &actual);
}

#[test]
fn nested_struct_matches() {
    let expected = std_types::Outer {
        inner: std_types::Inner { value: 9 },
        name: "n".into(),
    };
    let actual = no_inline_types::Outer {
        inner: no_inline_types::Inner { value: 9 },
        name: "n".into(),
    };
    assert_same(&expected, &actual);
}

/// An empty (uninhabited) enum generates `match self {}`; assert the derive still
/// produces a valid `Debug` impl even though no value can be formatted.
#[test]
fn empty_enum_compiles_with_debug() {
    fn assert_debug<T: std::fmt::Debug>() {}
    assert_debug::<std_types::Never>();
    assert_debug::<no_inline_types::Never>();
}
