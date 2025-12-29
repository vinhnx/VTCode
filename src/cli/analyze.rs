//! Analysis command implementations for VT Code

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum AnalysisType {
    Full,
    Structure,
    Security,
    Performance,
    Dependencies,
    Complexity,
}
