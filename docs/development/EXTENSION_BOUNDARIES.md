# Extension Boundaries in VT Code

This document records VT Code's extension policy for contributors.

## Why This Exists

Rust traits are a strong internal composition tool, but they are a poor default
boundary for ecosystem-style extension. When a third party must add support for
VT Code by implementing or waiting on a crate-local trait, we create the same
kind of "got there first" pressure that shows up in broader Rust coherence and
orphan-rule discussions. In practice, that pressure makes alternatives harder to
ship: whichever compile-time trait surface lands first tends to become the one
every integration must target.

For VT Code, the practical rule is simple:

**Use Rust traits inside the VT Code workspace. Use config, manifests, and
protocols at the boundary.**

This is a design constraint, not just a style preference. If an external
integration should work without patching VT Code or coordinating a new upstream
impl, do not make a public Rust trait the default integration story.

## Default Extension Order

When adding a new extension point, prefer these seams in order:

1. `vtcode.toml` configuration for routing or provider selection
2. `[[custom_providers]]` for OpenAI-compatible model endpoints
3. MCP for external tools, resources, and prompts
4. Skills or plugin manifests for packaged behavior and discovery
5. New Rust traits only when the integration is internal to VT Code runtime code

If a feature must work for third parties without patching VT Code itself, it
should usually not start as a new trait in `vtcode-core`.

Two concrete red flags:

- The main expected adopters live outside the VT Code workspace.
- Two independent integrations could reasonably want to provide the same kind of
  capability without depending on each other.

If either is true, prefer a protocol or data boundary first.

## What Counts As Internal

These are valid internal trait seams:

- `LLMProvider` for VT Code's built-in provider implementations
- `Tool`, `ModeTool`, and CGP provider traits for runtime composition
- `McpToolExecutor` and related adapters between VT Code subsystems

These traits are useful because VT Code owns both sides of the boundary inside
the workspace.

## What Counts As External

These should stay protocol- or data-driven:

- Connecting a new hosted model endpoint
- Exposing external tools or resources
- Packaging reusable skills or plugins
- Shipping org-specific behavior without recompiling VT Code

Current paved paths:

- `[[custom_providers]]` for named OpenAI-compatible endpoints
- `[mcp]` providers for external tool ecosystems
- Skill directories and manifests for reusable agent guidance
- Plugin manifests for runtime-discovered extensions

## Review Checklist

Before adding a new trait-based extension point, ask:

1. Does a third party need to adopt this without changing VT Code core?
2. Would two independent integrations reasonably want to provide the same kind
   of capability?
3. Can a config file, manifest, MCP server, or schema describe this boundary
   well enough?
4. Are we creating a new "foundation trait" that every downstream integration
   would now need to implement?

If the answer to any of the first three is "yes", prefer a protocol/data
boundary. If the fourth is "yes", stop and redesign.

## Consequences For VT Code

- Document built-in traits as internal implementation details, not the default
  external integration story.
- Prefer schema-carrying registrations and manifests over compile-time wiring
  for third-party capabilities.
- Keep adding built-in providers and tools when VT Code must own the runtime
  behavior, but avoid making that the only path for extension.
- Treat new public traits as a review burden: the proposer should explain why a
  manifest, config schema, MCP server, or plugin surface is insufficient.
