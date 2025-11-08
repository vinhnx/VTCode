# LLM Generation Configuration

This document describes the configurable LLM generation parameters in VTCode.

## Overview

VTCode now supports configurable temperature and max_tokens settings for different LLM generation tasks, replacing previously hardcoded values. All settings are customizable in `vtcode.toml`.

## Main LLM Generation

### Temperature

**Config Key**: `temperature`  
**Type**: `f32`  
**Default**: `0.7`  
**Range**: `0.0` - `1.0`

Controls the randomness/creativity of main LLM responses:
- `0.0` = Deterministic (always the same output)
- `0.7` = Balanced (recommended default for most use cases)
- `1.0` = Maximum randomness/creativity

**Example**:
```toml
[agent]
temperature = 0.7
```

### Max Tokens

**Config Key**: `max_tokens`  
**Type**: `u32`  
**Default**: `2000`

Controls the maximum length of main LLM responses.

**Recommended values**:
- `2000` = Default for standard tasks
- `4000` = For longer code generation tasks
- `16384` = For models with 128k context window
- `32768` = For models with 256k context window

**Example**:
```toml
[agent]
max_tokens = 2000
```

## Prompt Refinement

### Refine Temperature

**Config Key**: `refine_temperature`  
**Type**: `f32`  
**Default**: `0.3`  
**Range**: `0.0` - `1.0`

Controls the creativity of prompt refinement. Lower temperature ensures more deterministic/consistent improvements to user prompts before sending to main LLM.

**Why lower?** Prompt refinement should be conservative and consistent, not creative. Lower values prevent unpredictable transformations.

**Example**:
```toml
[agent]
refine_temperature = 0.3
```

### Refine Max Tokens

**Config Key**: `refine_max_tokens`  
**Type**: `u32`  
**Default**: `800`

Controls the maximum length of refined prompts. Prompts are typically shorter than full responses, so 800 tokens is usually sufficient.

**Example**:
```toml
[agent]
refine_max_tokens = 800
```

## Constants

Related constants are defined in `vtcode-config/src/constants.rs` under the `llm_generation` module:

```rust
pub mod llm_generation {
    // Main LLM generation
    pub const DEFAULT_TEMPERATURE: f32 = 0.7;
    pub const DEFAULT_MAX_TOKENS: u32 = 2_000;

    // Prompt refinement
    pub const DEFAULT_REFINE_TEMPERATURE: f32 = 0.3;
    pub const DEFAULT_REFINE_MAX_TOKENS: u32 = 800;

    // Context window guidelines
    pub const MAX_TOKENS_256K_CONTEXT: u32 = 32_768;
    pub const MAX_TOKENS_128K_CONTEXT: u32 = 16_384;
}
```

## Implementation

Temperature and max_tokens are now:

1. **Configurable in vtcode.toml** - Set once and apply globally to each task type
2. **Task-aware** - Different settings for main generation vs prompt refinement
3. **Validated** - Config validation ensures temperature is in [0, 1] range
4. **Centralized** - All values stored in `AgentConfig` struct
5. **Runtime override capable** - Session-specific settings can still override if needed

### Code Changes

#### Configuration Structure
- Added `temperature: f32` field to `AgentConfig` (main generation)
- Added `max_tokens: u32` field to `AgentConfig` (main generation)
- Added `refine_temperature: f32` field to `AgentConfig` (prompt refinement)
- Added `refine_max_tokens: u32` field to `AgentConfig` (prompt refinement)
- Added `validate_llm_params()` method to validate ranges

#### LLM Request Generation Updates

**File**: `src/agent/runloop/unified/turn/session.rs`
- Main LLM request reads from `vt_cfg.agent.temperature` and `vt_cfg.agent.max_tokens`
- Self-review passes use config values instead of hardcoded values
- Runtime overrides still take precedence

**File**: `src/agent/runloop/prompt.rs`
- Prompt refinement now uses `vt_cfg.agent.refine_temperature` and `vt_cfg.agent.refine_max_tokens`
- Replaced hardcoded `0.3` temp and `800` tokens with config values

**Before**:
```rust
// Main generation (hardcoded)
temperature: Some(0.7),
max_tokens: Some(2000),

// Self-review (hardcoded)
temperature: Some(0.5),
max_tokens: Some(2000),

// Prompt refinement (hardcoded)
temperature: Some(0.3),
max_tokens: Some(800),
```

**After**:
```rust
// Main generation (configurable)
temperature: config_temp,  // from vt_cfg.agent.temperature
max_tokens: max_tokens_opt.or(config_max_tokens),

// Self-review (configurable, uses same as main)
temperature: review_temp,  // from vt_cfg.agent.temperature
max_tokens: review_max_tokens,  // from vt_cfg.agent.max_tokens

// Prompt refinement (configurable)
temperature: Some(vtc.agent.refine_temperature),
max_tokens: Some(vtc.agent.refine_max_tokens),
```

## Migration Guide

### For existing users
If you don't have `temperature` or `max_tokens` in your vtcode.toml, they will use the defaults:
- `temperature = 0.7`
- `max_tokens = 2000`

### To customize
Add to your `[agent]` section in `vtcode.toml`:

```toml
[agent]
# ... other settings ...
temperature = 0.5  # More deterministic
max_tokens = 4000  # Allow longer responses
```

## Notes

- Temperature and max_tokens apply to both regular responses and self-review passes
- These are global defaults; individual requests may override them at runtime
- For best results with large context windows, adjust max_tokens accordingly
