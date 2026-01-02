//! Models Manager Module
//!
//! This module provides centralized model discovery, caching, and management
//! following patterns from OpenAI Codex. It coordinates:
//!
//! - **Remote Model Discovery**: Fetching available models from provider APIs
//! - **Local Model Presets**: Built-in model configurations for offline use
//! - **Caching**: TTL-based disk and memory caching for model metadata
//! - **Model Families**: Grouping models by capabilities and characteristics
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │  ModelsManager  │
//! ├─────────────────┤
//! │ - local_models  │──────┐
//! │ - remote_models │      │
//! │ - cache         │      ▼
//! └────────┬────────┘  ┌─────────────┐
//!          │           │ ModelFamily │
//!          │           └─────────────┘
//!          ▼
//!    ┌───────────┐
//!    │ ModelsCache │
//!    └───────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use vtcode_core::models_manager::ModelsManager;
//!
//! let manager = ModelsManager::new();
//!
//! // List available models
//! let models = manager.list_models().await;
//!
//! // Get a specific model's family
//! let family = manager.construct_model_family("gemini-2.5-flash").await;
//! ```

pub mod cache;
pub mod manager;
pub mod model_family;
pub mod model_presets;

pub use cache::ModelsCache;
pub use manager::{ModelsManager, SharedModelsManager, new_shared_models_manager};
pub use model_family::{ModelFamily, find_family_for_model};
pub use model_presets::{ModelInfo, ModelPreset, builtin_model_presets, presets_for_provider};
