#![allow(clippy::needless_lifetimes)]

use allocative::Allocative;
use derive_more::derive::Display;
use starlark::any::ProvidesStaticType;
use starlark::values::AllocValue;
use starlark::values::Heap;
use starlark::values::NoSerialize;
use starlark::values::StarlarkValue;
use starlark::values::UnpackValue;
use starlark::values::Value;
use starlark::values::ValueLike;
use starlark::values::starlark_value;

use super::arg_type::ArgType;

/// Command line option that takes a value.
#[derive(Clone, Debug, Display, PartialEq, Eq, ProvidesStaticType, NoSerialize, Allocative)]
#[display("opt({})", opt)]
pub struct Opt {
    /// The option as typed on the command line, e.g., `-h` or `--help`. If
    /// it can be used in the `--name=value` format, then this should be
    /// `--name` (though this is subject to change).
    pub opt: String,
    pub meta: OptMeta,
    pub required: bool,
}

/// When defining an Opt, use as specific an OptMeta as possible.
#[derive(Clone, Debug, Display, PartialEq, Eq, ProvidesStaticType, NoSerialize, Allocative)]
#[display("{}", self)]
pub enum OptMeta {
    /// Option does not take a value.
    Flag,

    /// Option takes a single value matching the specified type.
    Value(ArgType),
}

impl Opt {
    pub fn new(opt: String, meta: OptMeta, required: bool) -> Self {
        Self {
            opt,
            meta,
            required,
        }
    }

    pub fn name(&self) -> &str {
        &self.opt
    }
}

#[starlark_value(type = "Opt")]
impl<'v> StarlarkValue<'v> for Opt {
    type Canonical = Opt;
}

impl<'v> UnpackValue<'v> for Opt {
    fn unpack_value(value: Value<'v>) -> Option<Self> {
        // TODO(mbolin): It feels like this should be doable without cloning?
        // Cannot simply consume the value?
        value.downcast_ref::<Opt>().cloned()
    }
}

impl<'v> AllocValue<'v> for Opt {
    fn alloc_value(self, heap: &'v Heap) -> Value<'v> {
        heap.alloc_simple(self)
    }
}

#[starlark_value(type = "OptMeta")]
impl<'v> StarlarkValue<'v> for OptMeta {
    type Canonical = OptMeta;
}
