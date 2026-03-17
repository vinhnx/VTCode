# Model Addition Workflow Summary

This document provides a high-level overview of the model addition process and automation approach.

## Why This Workflow?

Adding a new LLM model to VT Code requires updates across **three architectural layers**:

```
┌─────────────────────────────────────────────┐
│ Application Layer (Runtime)                  │
│  - /model palette & selection                │
│  - Model capabilities discovery              │
├─────────────────────────────────────────────┤
│ Configuration Layer (Resolution)             │
│  - ModelId enum & match arms                 │
│  - Provider assignment                       │
│  - Capability flags                          │
├─────────────────────────────────────────────┤
│ Constants Layer (Database)                   │
│  - String constants (openai.rs)              │
│  - Metadata (docs/models.json)               │
└─────────────────────────────────────────────┘
```

Each layer must be independently coherent AND logically connected. This is why a simple script can't fully automate the process—it must be intentional and verifiable at each step.

## The 10-Step Process

### Layer 1: Constants (2 files)
1. **openai.rs** - Add to `SUPPORTED_MODELS` array + define convenience constant
2. **models.json** - Complete metadata entry (context, capabilities, modalities)

### Layer 2: Configuration (8 files)
3. **model_id.rs** - Add enum variant with doc comment
4. **as_str.rs** - Map enum to constant string
5. **display.rs** - Map enum to display name
6. **description.rs** - Map enum to description text
7. **parse.rs** - Map string to enum (enables CLI parsing)
8. **provider.rs** - Assign to provider (OpenAI, Anthropic, etc.)
9. **collection.rs** - Add to all_models() discovery list
10. **capabilities.rs** - Add to generation() and optional trait methods

## Automation Level: 60% (Guided, Not Fully Automated)

### Why Not 100% Automation?

Full automation would require:
- Complex AST parsing and code generation
- Maintaining sync between 10 disparate file formats
- No opportunity for human verification
- Brittle brittle if files change format

### What We Provide Instead

✅ **Documentation** (`ADDING_MODELS.md`)
- Detailed instructions for each file
- Examples for copy-paste
- Verification steps

✅ **Checklist** (`MODEL_ADDITION_CHECKLIST.md`)
- Step-by-step verification
- Testing template
- Common issues & fixes

✅ **Helper Script** (`scripts/add_model.sh`)
- Interactive prompts for model details
- Generates code snippets for all 10 files
- Guides you through manual insertion points

✅ **Quick Template**
```bash
# Run this to get guided prompts + code generation
./scripts/add_model.sh
```

## Typical Workflow

### Option A: Manual (Recommended First Time)

1. Read `docs/development/ADDING_MODELS.md`
2. Open all 10 files in editor split-view
3. Follow detailed step-by-step guide
4. Verify with checklist
5. Run cargo check

**Time:** 15-20 min first time, 5-10 min afterwards

### Option B: Script-Guided (Recommended for Speed)

```bash
./scripts/add_model.sh
# Prompts you for model details
# Generates code snippets for all files
# Shows you exact insertion points
# Copy-paste into files
# Run cargo check
```

**Time:** 5-10 min

### Option C: Future Full Automation (if needed)

Could build:
- Declarative model registry (YAML)
- Code generator in `build.rs`
- Auto-inserts in files with markers

But trades off:
- Explicitness (good for auditing)
- Flexibility (adding new provider types)
- Learning value (understanding all layers)

## File Dependency Graph

```
models.json (metadata source)
    ↓
    ├→ as_str.rs (string mapping)
    ├→ display.rs (names)
    ├→ description.rs (descriptions)
    └→ capabilities.rs (introspection)
    
model_id.rs (enum definition)
    ↓
    ├→ parse.rs (CLI parsing)
    ├→ provider.rs (provider assignment)
    ├→ collection.rs (discovery)
    └→ capabilities.rs (trait implementations)

openai.rs (constants)
    ↓
    └→ as_str.rs (constant references)
```

## Verification Strategy

### Compile Check
```bash
cargo check --package vtcode-config
cargo check --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

### Functional Check
```bash
# Model appears in palette
vtcode ask --model gpt-5.4-nano "test"

# String parsing works
echo 'gpt-5.4-nano' | cargo test -- --nocapture
```

### Test Template
```rust
#[test]
fn test_new_model() {
    let model = "gpt-5.4-nano".parse::<ModelId>().unwrap();
    assert_eq!(model.provider(), Provider::OpenAI);
    assert_eq!(model.generation(), "5.4");
}
```

## Next Steps

### For Next Model Addition

1. **Check the docs first:**
   ```bash
   open docs/development/ADDING_MODELS.md
   ```

2. **Use the script for guidance:**
   ```bash
   ./scripts/add_model.sh
   ```

3. **Follow the checklist:**
   ```bash
   open docs/development/MODEL_ADDITION_CHECKLIST.md
   ```

4. **Run verification:**
   ```bash
   cargo check --package vtcode-config
   cargo clippy --workspace --all-targets -- -D warnings
   ```

### Ideas for Further Improvement

- [ ] Add GitHub issue template for new models
- [ ] Create editor snippet for each file type
- [ ] Build Rust macro for boilerplate generation
- [ ] Add git hook to verify all 10 files updated
- [ ] Create test generator from models.json

## Key Principles

1. **Explicit over implicit** - All 10 files updated, all locations visible
2. **Verifiable at each step** - Cargo check after phase 1, 2, 3
3. **Documented patterns** - Follow existing model patterns exactly
4. **Single source of truth** - models.json is metadata source
5. **Type-safe resolution** - Enum catches missing cases

## Related Documentation

- [Complete Guide](./ADDING_MODELS.md)
- [Checklist](./MODEL_ADDITION_CHECKLIST.md)
- [Provider Setup](../providers/PROVIDER_GUIDES.md)
- [Models Metadata](../models.json)
