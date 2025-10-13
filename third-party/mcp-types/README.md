# mcp-types

Rust type bindings for the [Model Context Protocol (MCP) specification](https://spec.modelcontextprotocol.io/specification/2024-11-05/), automatically generated from the protocol's [JSON schema](https://github.com/modelcontextprotocol/specification/blob/main/schema/2024-11-05/schema.json) using the `typify` crate.

## Overview

This crate provides strongly-typed Rust structs and enums that represent the Model Context Protocol, a JSON-RPC based protocol for communication between LLM clients and servers. The types are automatically generated at build time from the official MCP JSON schema specification.

## Motivation

To provide auto-generated and correct bindings for the MCP spec in a crate separate from any client/server implementation code.

## Features

- Complete type definitions for the MCP protocol
- Serde serialization/deserialization support
- Generated from the latest MCP schema specification
- Type-safe interaction with MCP messages
- Schema validation test coverage

## Usage

Add this to your `Cargo.toml`:

```tomle
mcp-types = "0.1.0"
```

or run this from inside your crate directory tree:

```bash
cargo add mcp-types
```

## Contributing

Contributions are welcome! Please note that this crate's types are automatically generated from the MCP schema, so most changes will have to do with how the bindings are generated, how types are re-exported, documentation, and other things of that nature. Please do not manually edit generated code.

To contribute, please fork the `mcp` repo, make a new branch off of `main`, and make a PR from your fork to `main` again.

