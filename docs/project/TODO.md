fix techdebt

- Merging duplicated LLM provider stacks between vtcode-core and vtcode-llm.
- Splitting very large modules (structural_search.rs, subagents/mod.rs).
- Converting stringly-typed IDs (PluginId, MarketplaceId, etc.) to newtypes.
- Systematically removing the remaining ~8,128 unwrap/expect/panic sites.
- Enabling #![warn(missing_docs)] project-wide.
