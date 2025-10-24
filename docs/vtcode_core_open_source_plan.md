# Comprehensive Plan to Open Source VTCode Core Components

## Overview
This plan outlines the steps to open source the VTCode core components as separate GitHub repositories and Rust crates. All of these components are part of a larger Rust workspace and serve different aspects of the VTCode ecosystem.

## Component Analysis

### 1. vtcode-acp-client
**Status**: Relatively self-contained
- **Dependencies**: Only `agent-client-protocol = "0.4.5"` (external)
- **Complexity**: Low
- **Size**: Small (~656 bytes source code)
- **Functionality**: Global ACP (Agent Client Protocol) client registration and retrieval
- **Directory**: Contains `Cargo.toml` and `src` directory

### 2. vtcode-commons
**Status**: Shared utilities
- **Dependencies**: Minimal, mostly standard library
- **Complexity**: Low to Medium
- **Size**: Small (~7 files in src)
- **Functionality**: Shared error handling, path utilities, and telemetry functionality
- **Directory**: Contains `Cargo.toml` and 7 files in `src` including error handling, path management, and telemetry utilities

### 3. vtcode-config
**Status**: Configuration management
- **Dependencies**: More complex, with build scripts and examples
- **Complexity**: Medium
- **Size**: Medium (17 files in src, build.rs, examples, tests)
- **Functionality**: Configuration system with build-time data generation
- **Directory**: Contains `Cargo.toml`, build.rs, examples, tests, and 17 files in src

### 4. vtcode-core
**Status**: Core functionality
- **Dependencies**: Complex with multiple internal dependencies
- **Complexity**: High
- **Size**: Large (28 files in src, includes examples, tests)
- **Functionality**: Core VTCode functionality, LLM client implementations, providers
- **Directory**: Contains `Cargo.toml`, build.rs, examples, prompts, tests, and 28 files in src

### 5. vtcode-indexer
**Status**: Indexing functionality
- **Dependencies**: Minimal
- **Complexity**: Low
- **Size**: Small
- **Functionality**: Indexing related utilities
- **Directory**: Contains `Cargo.toml` and `src` directory

### 6. vtcode-llm
**Status**: LLM client layer
- **Dependencies**: Highly dependent on vtcode-commons and vtcode-core
- **Complexity**: High
- **Size**: Medium (15KB source code across 3 files)
- **Functionality**: Unified LLM client layer supporting multiple providers
- **Directory**: Contains `Cargo.toml` and 5 files in `src`

### 7. vtcode-markdown-store
**Status**: Markdown storage
- **Dependencies**: Unknown from directory structure
- **Complexity**: Low to Medium
- **Size**: Small
- **Functionality**: Markdown data storage
- **Directory**: Contains `Cargo.toml`, src, and tests

### 8. vtcode-tools
**Status**: Tool utilities
- **Dependencies**: Unknown from directory structure
- **Complexity**: Low to Medium
- **Size**: Small
- **Functionality**: Tool-related utilities
- **Directory**: Contains `Cargo.toml`, src, examples

## Recommended Strategy

### Option A: Selective Open Sourcing (Recommended)
Focus on the most suitable components for open sourcing:

1. **Primary candidates**: `vtcode-acp-client`, `vtcode-commons`, `vtcode-indexer`
2. **Secondary candidates**: `vtcode-llm`, `vtcode-tools` (with dependency management)
3. **Advanced candidates**: `vtcode-config`, `vtcode-core`, `vtcode-markdown-store` (require more refactoring)

### Option B: Hierarchical Open Sourcing
Create repositories in dependency order:
1. `vtcode-commons` (foundational utilities)
2. `vtcode-acp-client` (depends on external ACP library)
3. `vtcode-indexer` (depends on commons)
4. `vtcode-tools` (depends on commons/core)
5. `vtcode-llm` (depends on commons/core)
6. `vtcode-config` (configuration layer)
7. `vtcode-core` (core functionality)
8. `vtcode-markdown-store` (storage layer)

## Detailed Implementation Plan

### Phase 1: Preparation (Days 1-2)

1. **Create GitHub repositories**
   - Create repositories for each component you plan to open source
   - Set up appropriate repository settings (public, add descriptions, license)

2. **Legal and Licensing**
   - Review current LICENSE file (appears to be MIT)
   - Ensure all dependencies are compatible with open-sourcing
   - Check for any proprietary code that should not be shared

3. **Dependency mapping**
   - Create a dependency graph of all components
   - Identify which components can be extracted independently

### Phase 2: Foundational Components (Days 2-5)

1. **vtcode-commons** (highest priority)
   ```bash
   mkdir -p ~/open-source-projects/vtcode-commons
   cd ~/open-source-projects/vtcode-commons
   git init
   git remote add origin https://github.com/your-username/vtcode-commons.git
   ```
   - Copy `Cargo.toml` and `src` directory
   - Update version and dependencies as needed
   - Add documentation and examples

2. **vtcode-acp-client** (second priority)
   ```bash
   mkdir -p ~/open-source-projects/vtcode-acp-client
   cd ~/open-source-projects/vtcode-acp-client
   git init
   git remote add origin https://github.com/your-username/vtcode-acp-client.git
   ```
   - Copy source and update appropriately
   - This should be straightforward to extract

3. **vtcode-indexer** (third priority)
   ```bash
   mkdir -p ~/open-source-projects/vtcode-indexer
   cd ~/open-source-projects/vtcode-indexer
   git init
   git remote add origin https://github.com/your-username/vtcode-indexer.git
   ```
   - Copy and prepare source with minimal dependencies

### Phase 3: Medium Complexity Components (Days 5-10)

1. **vtcode-tools** (requires some refactoring)
   - Copy source and dependencies
   - Update to work independently
   - Add documentation

2. **vtcode-markdown-store** (requires some refactoring)
   - Copy and adjust dependencies
   - Add examples and tests

### Phase 4: Complex Components (Days 10-20)

1. **vtcode-llm** (highest dependency complexity)
   - This requires extracting parts of vtcode-commons and vtcode-core
   - Option A: Create a self-contained version with necessary dependencies included
   - Option B: Abstract dependencies behind traits
   - Requires careful dependency management

2. **vtcode-config** (has build scripts)
   - Extract configuration functionality
   - Handle build-time dependencies
   - Preserve examples and tests where possible

3. **vtcode-core** (most complex)
   - This is the most complex to extract due to dependencies
   - May require creating minimal versions of internal dependencies
   - Consider extracting only most useful parts (LLM providers, etc.)

### Phase 5: Documentation and Quality (Days 15-25)

1. **Create comprehensive README files for all repositories**
2. **Add documentation in code**
3. **Add examples and usage guides**
4. **Create contribution guidelines**
5. **Set up CI/CD workflows**
6. **Add badges to READMEs (build status, license, crates.io)**

### Phase 6: Publishing (Days 20-25)

1. **Publish to crates.io**
   - Start with simpler crates: vtcode-commons, vtcode-acp-client, vtcode-indexer
   - Then publish intermediate complexity: vtcode-tools, vtcode-markdown-store
   - Finally publish complex ones: vtcode-llm, vtcode-config, vtcode-core

2. **Update GitHub releases**
   - Create GitHub releases matching crates.io versions
   - Add release notes and changelogs

## Implementation Timeline
- **Days 1-2**: Preparation and dependency analysis
- **Days 2-5**: Foundational components (`vtcode-commons`, `vtcode-acp-client`, `vtcode-indexer`)
- **Days 5-10**: Medium complexity components (`vtcode-tools`, `vtcode-markdown-store`)
- **Days 10-20**: Complex components (`vtcode-llm`, `vtcode-config`, `vtcode-core`)
- **Days 15-25**: Documentation and quality improvements (overlapping with extraction)
- **Days 20-25**: Final publishing and release

## Risk Mitigation

1. **Dependency Complexity**: Complex components like `vtcode-llm` and `vtcode-core` have significant internal dependencies. Consider creating minimal self-contained versions initially.

2. **Breaking Changes**: Ensure API compatibility is maintained for existing users where possible.

3. **Maintenance**: Consider how to maintain these libraries going forward as part of the larger VTCode ecosystem.

4. **Code Quality**: Some internal code may not be fully documented or tested for public consumption. Plan for additional work to prepare code for public consumption.

## Success Metrics
- All targeted crates successfully published to crates.io
- Users can add the crates to their projects without dependency issues
- Proper documentation and examples are available
- CI/CD pipelines are set up and passing
- Each repository has appropriate README and contribution guidelines

## Next Steps
1. Create the GitHub repositories for each component you plan to release
2. Review the current license and any proprietary code that should not be shared
3. Start with the simpler components: `vtcode-commons`, `vtcode-acp-client`, `vtcode-indexer`
4. Work on medium-complexity components with appropriate dependency management
5. Tackle the most complex components last, with careful consideration of dependencies