# vtcode-acp

`vtcode-acp` is the canonical ACP crate for VT Code.

It contains:

- The ACP client library now lives in this crate
- The VT Code Zed bridge and ACP connection registration helpers

<!-- cargo-rdme start -->

ACP (Agent Communication Protocol) support for VT Code.

This crate exposes both the ACP client library and the VT Code Zed bridge.
Downstream crates should treat this as the canonical ACP entrypoint.

<!-- cargo-rdme end -->

## Public entrypoints

- `AcpClientV2` for protocol-compliant ACP clients
- `AcpClient` for the deprecated V1 client API
- `StandardAcpAdapter` and `ZedAcpAdapter` for launching VT Code over ACP

## API reference

See [docs.rs/vtcode-acp](https://docs.rs/vtcode-acp).

## Related docs

- [ACP integration guide](../docs/acp/ACP_INTEGRATION.md)
- [ACP quick reference](../docs/acp/ACP_QUICK_REFERENCE.md)
