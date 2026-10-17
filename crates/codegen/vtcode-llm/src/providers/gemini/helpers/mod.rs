use super::*;

mod cache;
mod capabilities;
mod convert;
mod request;

pub(super) use convert::InteractionStreamState;
pub use convert::serialize_gemini_tools;
pub(super) use request::{collect_gemini_tool_spec, preserved_gemini_parts_detail};

const GEMINI_PRESERVED_PARTS_PREFIX: &str = "__vtcode_gemini_parts__:";
