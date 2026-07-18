pub mod patch;

pub use patch::{
    Patch, PatchError, PatchHunk, PatchLine, PatchOperation, looks_like_unified_diff, looks_like_vte_patch,
};
