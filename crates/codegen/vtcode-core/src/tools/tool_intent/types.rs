use serde_json::Value;

pub type ToolIntentClassifier = fn(&Value) -> ToolIntent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSurfaceKind {
    Function,
    ApplyPatch,
}

#[derive(Debug, Clone, Copy)]
pub enum ToolMutationModel {
    ReadOnly,
    Mutating,
    ByArgs(ToolIntentClassifier),
}

impl ToolMutationModel {
    pub fn classify(self, args: &Value) -> ToolIntent {
        match self {
            Self::ReadOnly => ToolIntent::read_only(),
            Self::Mutating => ToolIntent::mutating(),
            Self::ByArgs(classifier) => classifier(args),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ToolBehavior {
    pub surface_kind: ToolSurfaceKind,
    pub mutation_model: ToolMutationModel,
    pub supports_parallel_calls: bool,
    pub safe_mode_prompt: bool,
}

impl ToolBehavior {
    pub const fn function(
        mutation_model: ToolMutationModel,
        supports_parallel_calls: bool,
        safe_mode_prompt: bool,
    ) -> Self {
        Self {
            surface_kind: ToolSurfaceKind::Function,
            mutation_model,
            supports_parallel_calls,
            safe_mode_prompt,
        }
    }

    pub const fn apply_patch(
        mutation_model: ToolMutationModel,
        supports_parallel_calls: bool,
        safe_mode_prompt: bool,
    ) -> Self {
        Self {
            surface_kind: ToolSurfaceKind::ApplyPatch,
            mutation_model,
            supports_parallel_calls,
            safe_mode_prompt,
        }
    }

    /// Classifies the tool's intent for the given arguments by delegating to the mutation model.
    pub fn classify(self, args: &Value) -> ToolIntent {
        self.mutation_model.classify(args)
    }
}

/// Describes whether a tool invocation is mutating, destructive, or safe to retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolIntent {
    /// Whether the tool modifies state or files.
    pub mutating: bool,
    /// Whether the tool performs potentially destructive operations.
    pub destructive: bool,
    /// Whether the tool is a read-only unified action (e.g. `file_operation` read).
    pub readonly_unified_action: bool,
    /// Whether the tool call is safe to retry on failure.
    pub retry_safe: bool,
}

impl ToolIntent {
    /// Returns a read-only, non-destructive, retry-safe intent.
    pub const fn read_only() -> Self {
        Self {
            mutating: false,
            destructive: false,
            readonly_unified_action: false,
            retry_safe: true,
        }
    }

    pub const fn read_only_unified_action() -> Self {
        Self {
            mutating: false,
            destructive: false,
            readonly_unified_action: true,
            retry_safe: true,
        }
    }

    pub const fn mutating() -> Self {
        Self {
            mutating: true,
            destructive: true,
            readonly_unified_action: false,
            retry_safe: false,
        }
    }
}
