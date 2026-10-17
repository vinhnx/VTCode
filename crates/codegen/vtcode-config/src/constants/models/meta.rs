//! Official Meta AI Muse models exposed by VT Code.

/// Meta Muse Spark 1.1.
pub const MUSE_SPARK_1_1: &str = "muse-spark-1.1";
/// Meta Muse Spark 1.3.
pub const MUSE_SPARK_1_3: &str = "muse-spark-1.3";
/// Meta Muse Spark 1.3 Contributor-tier variant.
pub const MUSE_SPARK_1_3_CONTRIBUTOR: &str = "muse-spark-1.3-contributor";
/// Meta Muse Wonder 1.0.
pub const MUSE_WONDER_1_0: &str = "muse-wonder-1.0";
/// Meta Muse Wonder Full 1.0 (Muse Code subscription only).
pub const MUSE_WONDER_FULL_1_0: &str = "muse-wonder-full-1.0";

/// Default Meta AI model.
pub const DEFAULT_MODEL: &str = MUSE_SPARK_1_3;

/// Official Meta AI models supported by VT Code.
pub const SUPPORTED_MODELS: &[&str] = &[
    MUSE_SPARK_1_1,
    MUSE_SPARK_1_3,
    MUSE_SPARK_1_3_CONTRIBUTOR,
    MUSE_WONDER_1_0,
    MUSE_WONDER_FULL_1_0,
];

/// Meta Muse models use always-on reasoning.
pub const REASONING_MODELS: &[&str] = SUPPORTED_MODELS;
